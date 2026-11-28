// Builds the npm packages for one release into an output directory:
//   <out>/platforms/<target>/   one package per binary (@prunecss/<platform>,
//                               with os/cpu/libc fields)
//   <out>/prunecss/             the launcher, depending on all of them
//
// From the release archives (what the release workflow publishes):
//   node npm/assemble.mjs --version 0.1.0 --dist dist --out npm-out
// From one local binary (what CI uses to test the packages):
//   node npm/assemble.mjs --version 0.1.0 --binary target/debug/prunecss \
//     --target x86_64-unknown-linux-gnu --out npm-out
//
// Uses only Node built-ins and the system `tar`/`unzip`, called without a
// shell.
import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const { values: args } = parseArgs({
  options: {
    version: { type: 'string' },
    dist: { type: 'string' },
    binary: { type: 'string' },
    target: { type: 'string' },
    out: { type: 'string' },
  },
});

function fail(message) {
  console.error(`assemble: ${message}`);
  process.exit(2);
}

if (!args.version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(args.version)) fail('--version X.Y.Z is required');
if (!args.out) fail('--out DIR is required');
if (Boolean(args.dist) === Boolean(args.binary)) fail('give either --dist or --binary (with --target)');
if (args.binary && !args.target) fail('--binary needs --target');

const version = args.version;
const out = resolve(args.out);
const launcherDir = join(here, 'prunecss');
const platforms = JSON.parse(readFileSync(join(launcherDir, 'platforms.json'), 'utf8'));
const launcherManifest = JSON.parse(readFileSync(join(launcherDir, 'package.json'), 'utf8'));

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'platforms'), { recursive: true });

// Returns the path of the prunecss binary for `platform`, or null when it
// is not part of this build.
function binaryFor(platform) {
  const exe = platform.os === 'win32' ? 'prunecss.exe' : 'prunecss';
  if (args.binary) {
    return platform.target === args.target ? resolve(args.binary) : null;
  }
  const base = `prunecss-${version}-${platform.target}`;
  const tarball = join(resolve(args.dist), `${base}.tar.gz`);
  const zip = join(resolve(args.dist), `${base}.zip`);
  const scratch = mkdtempSync(join(tmpdir(), 'prunecss-npm-'));
  if (existsSync(tarball)) {
    execFileSync('tar', ['-xzf', tarball, '-C', scratch]);
  } else if (existsSync(zip)) {
    execFileSync('unzip', ['-q', zip, '-d', scratch]);
  } else {
    fail(`missing release archive for ${platform.target} (${base}.tar.gz or .zip)`);
  }
  const binary = join(scratch, base, exe);
  if (!existsSync(binary)) fail(`${base} does not contain ${exe}`);
  return binary;
}

const shared = {
  version,
  license: launcherManifest.license,
  homepage: launcherManifest.homepage,
  repository: launcherManifest.repository,
};

const built = [];
for (const platform of platforms) {
  const binary = binaryFor(platform);
  if (!binary) continue;
  const dir = join(out, 'platforms', platform.target);
  const exe = platform.os === 'win32' ? 'prunecss.exe' : 'prunecss';
  mkdirSync(join(dir, 'bin'), { recursive: true });
  copyFileSync(binary, join(dir, 'bin', exe));
  chmodSync(join(dir, 'bin', exe), 0o755);
  for (const file of ['LICENSE-MIT', 'LICENSE-APACHE']) copyFileSync(join(root, file), join(dir, file));
  const manifest = {
    name: platform.package,
    ...shared,
    description: `The prunecss binary for ${platform.os}-${platform.cpu}${platform.libc ? ` (${platform.libc})` : ''}. Install the \`prunecss\` package instead.`,
    os: [platform.os],
    cpu: [platform.cpu],
    ...(platform.libc ? { libc: [platform.libc] } : {}),
    files: ['bin/', 'LICENSE-MIT', 'LICENSE-APACHE'],
    preferUnplugged: true,
  };
  writeFileSync(join(dir, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(join(dir, 'README.md'), `# ${platform.package}\n\nThe prebuilt [prunecss](https://www.npmjs.com/package/prunecss) binary for ${manifest.os[0]}-${manifest.cpu[0]}${platform.libc ? ` (${platform.libc})` : ''}. Do not install this package directly: install \`prunecss\`, which picks the right binary for your platform.\n`);
  built.push(platform.package);
}
if (built.length === 0) fail('no platform package was built');

// The launcher package depends on every platform package of this release.
const launcherOut = join(out, 'prunecss');
cpSync(launcherDir, launcherOut, { recursive: true });
for (const file of ['LICENSE-MIT', 'LICENSE-APACHE']) copyFileSync(join(root, file), join(launcherOut, file));
const manifest = { ...launcherManifest, version, optionalDependencies: {} };
for (const platform of platforms) {
  // A local test build has one platform; a release must have all of them.
  if (args.dist || built.includes(platform.package)) manifest.optionalDependencies[platform.package] = version;
}
writeFileSync(join(launcherOut, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
chmodSync(join(launcherOut, 'bin', 'prunecss.js'), 0o755);

console.log(`assembled prunecss ${version}: ${built.join(', ')}`);
console.log(readdirSync(join(out, 'platforms')).map((p) => `  ${join(out, 'platforms', p)}`).join('\n'));
console.log(`  ${launcherOut}`);
