#!/usr/bin/env node
// In-process timing of the PurgeCSS JavaScript API: PurgeCSS's best case.
// Inputs are read into memory first and the JIT is warmed up, so this leaves
// out Node startup, module loading and file I/O that the CLI pays.
//
//   node scripts/tinybench-purgecss.mjs <spec.json>
// spec: { css, content: [paths], variant: "default"|"tailwind-extractor", time, warmupTime, iterations }
// Prints one JSON object on stdout.
import { readFileSync } from 'node:fs';
import { extname } from 'node:path';
import { Bench } from 'tinybench';
import { PurgeCSS } from 'purgecss';
import tailwindConfig from '../config/purgecss-tailwind.config.mjs';

const spec = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const css = readFileSync(spec.css, 'utf8');
const content = spec.content.map((p) => ({ raw: readFileSync(p, 'utf8'), extension: extname(p).slice(1) }));
const extra = spec.variant === 'tailwind-extractor' ? { defaultExtractor: tailwindConfig.defaultExtractor } : {};

let outBytes = 0;
const bench = new Bench({ time: spec.time, warmupTime: spec.warmupTime, iterations: spec.iterations, warmupIterations: 2, retainSamples: true });
bench.add('purgecss-api', async () => {
  const [result] = await new PurgeCSS().purge({ css: [{ raw: css }], content, ...extra });
  outBytes = result.css.length;
});
await bench.run();

const t = bench.tasks[0];
const r = t.result;
if (r.state !== 'completed') {
  console.error(r.error ?? r.state);
  process.exit(1);
}
const lat = r.latency;
console.log(JSON.stringify({
  // tinybench reports milliseconds; convert to seconds to match hyperfine.
  mean: lat.mean / 1e3,
  stddev: lat.sd / 1e3,
  median: lat.p50 / 1e3,
  min: lat.min / 1e3,
  max: lat.max / 1e3,
  samples: lat.samplesCount,
  times: (lat.samples ?? []).map((ms) => ms / 1e3),
  outputBytes: outBytes,
}));
