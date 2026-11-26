// Records everything needed to interpret (and reproduce) a result file.
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import os from 'node:os';
import { BENCH_DIR, REPO_DIR, tryRun } from './util.mjs';

// Read from node_modules directly: some packages do not export package.json.
const pkgVersion = (name) => JSON.parse(readFileSync(join(BENCH_DIR, 'node_modules', name, 'package.json'), 'utf8')).version;
const read = (p) => (existsSync(p) ? readFileSync(p, 'utf8').trim() : null);

export function environment({ prunecssBin, timeTool }) {
  const osRelease = read('/etc/os-release');
  const cpuinfo = read('/proc/cpuinfo') ?? '';
  return {
    date: new Date().toISOString(),
    host: os.hostname(),
    hardware: {
      cpuModel: cpuinfo.match(/model name\s*:\s*(.*)/)?.[1] ?? os.cpus()[0]?.model ?? 'unknown',
      logicalCpus: os.cpus().length,
      memoryGiB: +(os.totalmem() / 2 ** 30).toFixed(1),
      hypervisor: tryRun('sh', ['-c', "lscpu 2>/dev/null | sed -n 's/^Hypervisor vendor: *//p'"]) || null,
      governor: read('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor') ?? 'unavailable (no cpufreq: VM or container)',
      noTurbo: read('/sys/devices/system/cpu/intel_pstate/no_turbo') ?? 'unavailable',
      loadAvg: os.loadavg().map((x) => +x.toFixed(2)),
    },
    os: {
      platform: `${os.platform()} ${os.arch()}`,
      kernel: os.release(),
      distro: osRelease?.match(/PRETTY_NAME="?([^"\n]*)/)?.[1] ?? null,
    },
    versions: {
      prunecss: tryRun(prunecssBin, ['--version']),
      prunecssGitCommit: tryRun('git', ['-C', REPO_DIR, 'rev-parse', 'HEAD']),
      prunecssGitDirty: (tryRun('git', ['-C', REPO_DIR, 'status', '--porcelain', '--', 'crates', 'Cargo.lock', 'Cargo.toml']) ?? '') !== '',
      purgecss: pkgVersion('purgecss'),
      node: process.version,
      rustc: tryRun('rustc', ['--version']),
      cargo: tryRun('cargo', ['--version']),
      hyperfine: tryRun('hyperfine', ['--version']),
      tinybench: pkgVersion('tinybench'),
      minifier: `lightningcss ${pkgVersion('lightningcss')} (node API, minify: true, errorRecovery: true)`,
      postcss: pkgVersion('postcss'),
      postcssSelectorParser: pkgVersion('postcss-selector-parser'),
      cssSelect: pkgVersion('css-select'),
      htmlparser2: pkgVersion('htmlparser2'),
      tailwindcssCli: pkgVersion('@tailwindcss/cli'),
      bootstrap: pkgVersion('bootstrap'),
      bulma: pkgVersion('bulma'),
      peakRssTool: timeTool,
    },
  };
}
