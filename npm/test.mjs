// End-to-end test of the npm packages, as a user would get them:
// assemble them around a locally built binary, `npm pack` them, install the
// tarballs into an empty project, and run `npx prunecss` there.
//
//   cargo build -p prunecss
//   node npm/test.mjs target/debug/prunecss x86_64-unknown-linux-gnu
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const [binaryArg, target] = process.argv.slice(2);
if (!binaryArg || !target) {
  console.error('usage: node npm/test.mjs <path/to/prunecss> <rust-target>');
  process.exit(2);
}
const binary = resolve(binaryArg);
const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
const version = /\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/.exec(cargo)?.[1];
assert.ok(version, 'workspace version not found in Cargo.toml');

const work = mkdtempSync(join(tmpdir(), 'prunecss-npm-test-'));
const out = join(work, 'packages');
execFileSync(process.execPath, [join(here, 'assemble.mjs'), '--version', version, '--binary', binary, '--target', target, '--out', out], { stdio: 'inherit' });

// Pack every package the way `npm publish` would.
const tarballs = [];
const packageDirs = [...readdirSync(join(out, 'platforms')).map((p) => join(out, 'platforms', p)), join(out, 'prunecss')];
for (const dir of packageDirs) {
  const name = execFileSync('npm', ['pack', '--silent', '--pack-destination', work], { cwd: dir, encoding: 'utf8' }).trim().split('\n').pop();
  tarballs.push(join(work, name));
}

// Install into an empty project, offline: nothing may come from the registry.
const project = join(work, 'project');
execFileSync('mkdir', ['-p', project]);
writeFileSync(join(project, 'package.json'), '{ "name": "test-project", "private": true }\n');
execFileSync('npm', ['install', '--no-audit', '--no-fund', '--offline', ...tarballs], { cwd: project, stdio: 'inherit' });

const site = join(root, 'crates/prunecss-cli/tests/fixtures/site');
function run(command, args) {
  return spawnSync(command, args, { cwd: site, encoding: 'utf8' });
}
const launcher = join(project, 'node_modules', '.bin', process.platform === 'win32' ? 'prunecss.cmd' : 'prunecss');
let failures = 0;
function check(name, fn) {
  try {
    fn();
    console.log(`ok   ${name}`);
  } catch (error) {
    failures += 1;
    console.log(`FAIL ${name}\n${error.message}`);
  }
}

check('version matches the release', () => {
  const result = run(launcher, ['--version']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), `prunecss ${version}`);
});

check('output is identical to the binary run directly', () => {
  const args = ['prune', 'styles.css', '--content', 'index.html', 'App.jsx', '--no-config'];
  const viaNpm = run(launcher, args);
  const direct = run(binary, args);
  assert.equal(viaNpm.status, 0, viaNpm.stderr);
  assert.equal(viaNpm.stdout, direct.stdout);
  assert.ok(viaNpm.stdout.length > 0, 'no CSS printed');
});

check('exit code 1 from --check is passed through', () => {
  const result = run(launcher, ['prune', 'styles.css', '--content', 'index.html', '--check', '--no-config']);
  assert.equal(result.status, 1, result.stderr);
});

check('exit code 2 from an error is passed through', () => {
  const result = run(launcher, ['prune', 'no-such-file.css', '--content', 'index.html', '--no-config']);
  assert.equal(result.status, 2);
});

check('npx finds the command', () => {
  const result = spawnSync('npx', ['--no-install', 'prunecss', '--version'], { cwd: project, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), `prunecss ${version}`);
});

if (failures > 0) {
  console.log(`${failures} check(s) failed`);
  process.exit(1);
}
console.log('all npm package checks passed');
