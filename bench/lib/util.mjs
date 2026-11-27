// Small helpers shared by the corpus scripts and the harness.
import { createHash } from 'node:crypto';
import { readFileSync, mkdirSync, writeFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

export const BENCH_DIR = join(dirname(fileURLToPath(import.meta.url)), '..');
export const REPO_DIR = join(BENCH_DIR, '..');
export const CORPUS_DIR = join(BENCH_DIR, 'corpus');

export function sha256(data) {
  return createHash('sha256').update(data).digest('hex');
}

export function sha256File(path) {
  return sha256(readFileSync(path));
}

export function writeFile(path, data) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data);
}

/** Deterministic PRNG (mulberry32) so generated corpora are byte-identical across runs. */
export function rng(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    int: (n) => Math.floor(next() * n),
    pick: (arr) => arr[Math.floor(next() * arr.length)],
    chance: (p) => next() < p,
  };
}

/** Lists files under `dir` (recursively) whose names match `re`, sorted, as absolute paths. */
export function listFiles(dir, re) {
  const out = [];
  const walk = (d) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (re.test(name)) out.push(p);
    }
  };
  walk(dir);
  return out;
}

export function rel(p) {
  return relative(BENCH_DIR, p);
}

/** Runs a command and returns trimmed stdout, or null if it fails or is missing. */
export function tryRun(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { encoding: 'utf8', ...opts });
  if (r.status !== 0 || r.error) return null;
  return r.stdout.trim();
}
