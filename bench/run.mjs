#!/usr/bin/env node
// One entry point for the whole benchmark: `npm run bench` (or `make -C bench`).
//
//   node run.mjs [--quick] [--only bootstrap,tailwind] [--runs 10] [--warmup 3]
//                [--rss-runs 10] [--skip-build] [--no-tinybench] [--label text]
//
// For every corpus it: checks both tools succeed and write non-empty output;
// times the CLIs with hyperfine; times PurgeCSS's JS API in-process with
// tinybench; measures peak RSS; minifies every output with the same
// minifier and measures raw/gzip/brotli; and diffs the kept selectors.
// Results go to results/<date>-<host>.json and .md.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, relative, basename } from 'node:path';
import { parseArgs } from 'node:util';
import os from 'node:os';
import { BENCH_DIR, REPO_DIR, sha256 } from './lib/util.mjs';
import { corpora, contentFiles } from './lib/corpora.mjs';
import { environment } from './lib/env.mjs';
import { minify, sizes } from './lib/sizes.mjs';
import { selectorSet, diffSets, classify, analyse, CATEGORIES } from './lib/selectors.mjs';
import { Oracle, stripPseudos } from './lib/oracle.mjs';
import { renderMarkdown } from './lib/report.mjs';
import tailwindConfig from './config/purgecss-tailwind.config.mjs';

const { values: argv } = parseArgs({
  options: {
    quick: { type: 'boolean', default: false },
    only: { type: 'string' },
    runs: { type: 'string' },
    warmup: { type: 'string' },
    'rss-runs': { type: 'string' },
    'skip-build': { type: 'boolean', default: false },
    'no-tinybench': { type: 'boolean', default: false },
    label: { type: 'string', default: '' },
    examples: { type: 'string', default: '8' },
  },
});
const QUICK = argv.quick;
const RUNS = Number(argv.runs ?? (QUICK ? 3 : 10));
const WARMUP = Number(argv.warmup ?? (QUICK ? 1 : 3));
const RSS_RUNS = Number(argv['rss-runs'] ?? (QUICK ? 3 : 10));
const TINYBENCH = QUICK ? { time: 300, warmupTime: 100, iterations: 3 } : { time: 5000, warmupTime: 1000, iterations: 10 };
const EXAMPLES = Number(argv.examples);
const ONLY = argv.only?.split(',');

const WORK = join(BENCH_DIR, '.work');
const PRUNECSS = join(REPO_DIR, 'target', 'release', 'prunecss');
const PURGECSS_BIN = join(BENCH_DIR, 'node_modules', 'purgecss', 'bin', 'purgecss.js');
const PURGECSS_TW_CONFIG = join(BENCH_DIR, 'config', 'purgecss-tailwind.config.mjs');
const r = (p) => relative(BENCH_DIR, p);
const log = (...a) => console.log(...a);

// ---------------------------------------------------------------- tools ---

function sh(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, { stdio: 'inherit', cwd: BENCH_DIR, ...opts });
  if (res.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed (${res.status ?? res.error})`);
  return res;
}

function has(cmd, args = ['--version']) {
  const res = spawnSync(cmd, args, { stdio: 'ignore' });
  return res.status === 0;
}

function ensureTools() {
  if (!argv['skip-build']) {
    log('> cargo build --release -p prunecss');
    sh('cargo', ['build', '--release', '-p', 'prunecss'], { cwd: REPO_DIR });
  }
  if (!existsSync(PRUNECSS)) throw new Error(`missing ${PRUNECSS}; build it or drop --skip-build`);
  if (!has('hyperfine')) {
    log('> hyperfine not found: cargo install hyperfine --locked');
    sh('cargo', ['install', 'hyperfine', '--locked']);
  }
  if (!existsSync(join(BENCH_DIR, 'node_modules', 'purgecss'))) throw new Error('run `npm ci` in bench/ first');
  // GNU time reports the kernel's peak RSS; without it we poll /proc.
  return has('/usr/bin/time', ['-f', '%M', 'true']) ? 'GNU time (/usr/bin/time -f %M)' : 'fallback: /proc/<pid>/status VmHWM polled every 2 ms';
}

// ----------------------------------------------------------- commands ---

/** The content-file arguments both tools get. Switch to a glob here once prunecss supports globs. */
function contentArgs(files) {
  return files.map(r);
}

function toolCommands(corpus, files, work) {
  const css = r(corpus.css);
  const content = contentArgs(files);
  const tools = [{
    id: 'prunecss',
    label: 'prunecss',
    argv: [r(PRUNECSS), 'prune', css, '--content', ...content, '--output', r(join(work, 'prunecss.css'))],
    output: join(work, 'prunecss.css'),
  }];
  for (const variant of corpus.purgecssVariants) {
    const id = variant === 'default' ? 'purgecss' : `purgecss-${variant}`;
    const outDir = join(work, id);
    tools.push({
      id,
      variant,
      label: variant === 'default' ? 'purgecss' : `purgecss (${variant})`,
      argv: ['node', r(PURGECSS_BIN), ...(variant === 'tailwind-extractor' ? ['--config', r(PURGECSS_TW_CONFIG)] : []), '--css', css, '--content', ...content, '--output', r(outDir)],
      output: join(outDir, basename(corpus.css)),
      outDir,
    });
  }
  for (const t of tools) {
    if (t.argv.some((a) => /\s/.test(a))) throw new Error(`paths must not contain spaces: ${t.argv.join(' ')}`);
  }
  return tools;
}

/** Runs a tool once and checks it really did the work (exit 0, non-empty output). */
function validate(tool) {
  rmSync(tool.output, { force: true });
  if (tool.outDir) mkdirSync(tool.outDir, { recursive: true });
  const res = spawnSync(tool.argv[0], tool.argv.slice(1), { cwd: BENCH_DIR, encoding: 'utf8', maxBuffer: 1 << 28 });
  if (res.status !== 0) throw new Error(`${tool.label} exited ${res.status}: ${res.stderr}`);
  if (!existsSync(tool.output)) throw new Error(`${tool.label} wrote no output at ${tool.output}`);
  const css = readFileSync(tool.output, 'utf8');
  if (!css.length) throw new Error(`${tool.label} wrote an empty file`);
  return { css, sha256: sha256(css) };
}

function hyperfine(name, commands, extra = []) {
  const json = join(WORK, `hyperfine-${name}.json`);
  const args = ['-N', '--warmup', String(WARMUP), '--runs', String(RUNS), '--export-json', json, ...extra];
  for (const c of commands) args.push('-n', c.label, c.argv.join(' '));
  log(`> hyperfine (${name}): ${commands.map((c) => c.label).join(' vs ')}`);
  sh('hyperfine', args);
  const data = JSON.parse(readFileSync(json, 'utf8'));
  return data.results.map((x) => {
    if (x.summary) {
      // hyperfine 2.x schema: per-metric summaries plus raw measurements.
      const w = x.summary.time_wall_clock;
      const mem = x.summary.memory_peak_resident;
      return {
        command: x.name ?? x.command,
        mean: w.mean, stddev: w.stddev, median: w.median, min: w.min, max: w.max,
        user: x.summary.time_user.mean, system: x.summary.time_system.mean,
        runs: x.measurements.length,
        times: x.measurements.map((m) => m.time_wall_clock.value),
        hyperfinePeakRssMedianKB: mem ? Math.round(mem.median / 1024) : undefined,
      };
    }
    // hyperfine 1.x schema.
    return {
      command: x.command,
      mean: x.mean, stddev: x.stddev, median: x.median, min: x.min, max: x.max,
      user: x.user, system: x.system, runs: x.times.length, times: x.times,
    };
  });
}

function peakRssOnce(tool, timeTool) {
  if (timeTool.startsWith('GNU')) {
    const res = spawnSync('/usr/bin/time', ['-f', '%M', ...tool.argv], { cwd: BENCH_DIR, encoding: 'utf8', maxBuffer: 1 << 28 });
    if (res.status !== 0) throw new Error(`${tool.label} failed under /usr/bin/time: ${res.stderr}`);
    return Number(res.stderr.trim().split('\n').pop());
  }
  // Fallback: poll the high-water mark until the process exits. It can miss
  // a peak in the last 2 ms, so it slightly under-reports.
  const child = spawnSync('sh', ['-c', `"$@" & pid=$!; hwm=0; while kill -0 $pid 2>/dev/null; do v=$(sed -n 's/^VmHWM:[^0-9]*\\([0-9]*\\).*/\\1/p' /proc/$pid/status 2>/dev/null); [ -n "$v" ] && [ "$v" -gt "$hwm" ] && hwm=$v; sleep 0.002; done; wait $pid; echo $hwm >&2`, 'sh', ...tool.argv], { cwd: BENCH_DIR, encoding: 'utf8', maxBuffer: 1 << 28 });
  if (child.status !== 0) throw new Error(`${tool.label} failed: ${child.stderr}`);
  return Number(child.stderr.trim().split('\n').pop());
}

function peakRss(tool, timeTool) {
  const kb = Array.from({ length: RSS_RUNS }, () => peakRssOnce(tool, timeTool)).sort((a, b) => a - b);
  return { medianKB: kb[Math.floor(kb.length / 2)], minKB: kb[0], maxKB: kb.at(-1), samplesKB: kb };
}

function tinybench(corpus, files, variant) {
  const spec = join(WORK, `tinybench-${corpus.name}-${variant}.json`);
  writeFileSync(spec, JSON.stringify({ css: corpus.css, content: files, variant, ...TINYBENCH }));
  const res = spawnSync('node', [join(BENCH_DIR, 'scripts', 'tinybench-purgecss.mjs'), spec], { cwd: BENCH_DIR, encoding: 'utf8', maxBuffer: 1 << 28 });
  if (res.status !== 0) throw new Error(`tinybench failed: ${res.stderr}`);
  return JSON.parse(res.stdout);
}

// --------------------------------------------------------- correctness ---

function prunecssScan(files) {
  const res = spawnSync(PRUNECSS, ['scan', ...files], { encoding: 'utf8', maxBuffer: 1 << 28 });
  if (res.status !== 0) throw new Error(`prunecss scan failed: ${res.stderr}`);
  return new Set(res.stdout.split('\n').filter(Boolean));
}

function purgecssWords(files, variant) {
  const extract = variant === 'tailwind-extractor' ? tailwindConfig.defaultExtractor : (c) => c.match(/[A-Za-z0-9_-]+/g) || [];
  const words = new Set();
  for (const f of files) for (const w of extract(readFileSync(f, 'utf8'))) words.add(w);
  return words;
}

/** Describes, for one selector, which of its required names each tool's extractor saw. */
function names(entry, scan, words) {
  const f = analyse(entry.selector);
  const req = [...f.classes.map((c) => `.${c}`), ...f.ids.map((i) => `#${i}`)];
  return {
    // prunecss matches both .class and #id against the class names it scanned.
    notScannedByPrunecss: req.filter((n) => !scan.has(n.slice(1))),
    notExtractedByPurgecss: req.filter((n) => !words.has(n.slice(1))),
  };
}

function correctness(corpus, files, outputs, minInput) {
  const inputSet = selectorSet(minInput);
  const oracle = new Oracle(files);
  const scan = prunecssScan(files);
  const sets = Object.fromEntries(Object.entries(outputs).map(([id, o]) => [id, selectorSet(o.minified)]));
  const result = { inputSelectors: inputSet.size, kept: {}, comparisons: {}, falseRemovals: {}, staticallyUnmatchedKept: {} };

  for (const [id, set] of Object.entries(sets)) {
    result.kept[id] = set.size;
    const words = id === 'prunecss' ? null : purgecssWords(files, id === 'purgecss' ? 'default' : 'tailwind-extractor');
    // Selectors this tool removed that match an element of the content.
    const removed = [...inputSet.keys()].filter((k) => !set.has(k));
    // A selector the tool printed differently (so it is not in the input)
    // is "rewritten", not removed: index those by their pseudo-stripped form.
    const strippedKey = (e) => `${e.context} | ${stripPseudos(e.selector).text}`;
    const rewritten = new Map();
    for (const [k, e] of set) if (!e.atRule && !inputSet.has(k)) rewritten.set(strippedKey(e), e.selector);
    const hits = [];
    for (const k of removed) {
      const e = inputSet.get(k);
      if (e.atRule || e.context.includes('&')) continue;
      const m = oracle.matches(e.selector);
      if (!m.match) continue;
      const rewrittenTo = rewritten.get(strippedKey(e));
      hits.push({ key: k, selector: e.selector, context: e.context, approx: m.approx, category: rewrittenTo ? 'rewritten' : classify(e, inputSet, k), rewrittenTo, ...(words ? { notExtractedByPurgecss: names(e, scan, words).notExtractedByPurgecss } : { notScannedByPrunecss: names(e, scan, new Set()).notScannedByPrunecss }) });
    }
    result.falseRemovals[id] = {
      removed: removed.length,
      matchingStaticDom: hits.length,
      exact: hits.filter((h) => !h.approx).length,
      rewrittenNotRemoved: hits.filter((h) => h.rewrittenTo).length,
      byCategory: countBy(hits, (h) => h.category),
      examples: hits.slice(0, EXAMPLES * 3),
    };
    if (!corpus.synthetic) {
      let n = 0;
      const cats = {};
      for (const [k, e] of set) {
        if (e.atRule || e.context.includes('&')) continue;
        if (!oracle.matches(e.selector).match) {
          n++;
          const c = classify(e, inputSet, k);
          cats[c] = (cats[c] ?? 0) + 1;
        }
      }
      result.staticallyUnmatchedKept[id] = { count: n, byCategory: cats };
    }
  }

  for (const id of Object.keys(sets).filter((x) => x !== 'prunecss')) {
    const words = purgecssWords(files, id === 'purgecss' ? 'default' : 'tailwind-extractor');
    const d = diffSets(sets.prunecss, sets[id]);
    const describe = (keys, set) => {
      const byCat = {};
      for (const k of keys) {
        const e = set.get(k);
        const cat = classify(e, inputSet, k);
        (byCat[cat] ??= []).push(k);
      }
      return Object.fromEntries(Object.entries(byCat).sort((a, b) => b[1].length - a[1].length).map(([cat, ks]) => [cat, {
        count: ks.length,
        matchStaticDom: ks.filter((k) => { const e = set.get(k); return !e.atRule && oracle.matches(e.selector).match; }).length,
        examples: ks.slice(0, EXAMPLES).map((k) => {
          const e = set.get(k);
          return { selector: e.selector, context: e.context || undefined, matchesStaticDom: e.atRule ? null : oracle.matches(e.selector).match, ...names(e, scan, words) };
        }),
      }]));
    };
    result.comparisons[`prunecss-vs-${id}`] = {
      keptByBoth: d.both,
      keptOnlyByPrunecss: { count: d.onlyA.length, byCategory: describe(d.onlyA, sets.prunecss) },
      other: id,
      keptOnlyByOther: { count: d.onlyB.length, byCategory: describe(d.onlyB, sets[id]) },
    };
  }
  return result;
}

function countBy(xs, f) {
  const out = {};
  for (const x of xs) out[f(x)] = (out[f(x)] ?? 0) + 1;
  return out;
}

// ---------------------------------------------------------------- main ---

async function main() {
  const timeTool = ensureTools();
  rmSync(WORK, { recursive: true, force: true });
  mkdirSync(WORK, { recursive: true });
  const env = environment({ prunecssBin: PRUNECSS, timeTool });
  log(`prunecss ${env.versions.prunecss} | purgecss ${env.versions.purgecss} | node ${env.versions.node} | ${env.versions.hyperfine}`);

  const results = {
    schema: 1,
    label: argv.label || 'indicative - shared cloud container',
    settings: { quick: QUICK, hyperfine: { warmup: WARMUP, runs: RUNS, shell: 'none (-N)' }, tinybench: TINYBENCH, rssRuns: RSS_RUNS },
    environment: env,
    nodeStartup: hyperfine('node-startup', [
      { label: 'node -e ""', argv: ['node', '-e', "''"] },
      { label: 'node -e "require(\'purgecss\')"', argv: ['node', '-e', '"require(\'purgecss\')"'] },
    ]),
    corpora: [],
  };

  const all = corpora();
  const missing = all.filter((c) => !c.available).map((c) => c.name);
  if (missing.length) log(`! not fetched/generated, skipping: ${missing.join(', ')} (run \`npm run fetch\`)`);
  const selected = all.filter((c) => c.available && (!ONLY || ONLY.includes(c.name)) && !(QUICK && c.name === 'synthetic-huge'));

  for (const corpus of selected) {
    log(`\n=== ${corpus.name}: ${corpus.description}`);
    const work = join(WORK, corpus.name);
    mkdirSync(work, { recursive: true });
    const files = contentFiles(corpus);
    const tools = toolCommands(corpus, files, work);
    const inputCss = readFileSync(corpus.css, 'utf8');
    const entry = {
      name: corpus.name,
      description: corpus.description,
      input: { cssBytes: Buffer.byteLength(inputCss), cssSha256: sha256(inputCss), contentFiles: files.length, contentBytes: files.reduce((s, f) => s + readFileSync(f).length, 0) },
      tools: {},
    };

    const outputs = {};
    for (const t of tools) {
      const { css, sha256: hash } = validate(t);
      const min = minify(css, `${t.id}.css`);
      outputs[t.id] = { minified: min.css };
      entry.tools[t.id] = { label: t.label, command: `${t.argv.slice(0, 6).join(' ')} ... (${t.argv.length} args)`, outputSha256: hash, size: sizes(css, min.css), minifierWarnings: min.warnings };
    }
    const minInput = minify(inputCss, 'input.css');
    entry.input.size = sizes(inputCss, minInput.css);

    if (!corpus.correctnessOnly) {
      const timing = hyperfine(corpus.name, tools);
      tools.forEach((t, i) => { entry.tools[t.id].cli = timing[i]; });
      for (const t of tools) {
        log(`> peak RSS x${RSS_RUNS}: ${t.label}`);
        entry.tools[t.id].peakRss = peakRss(t, timeTool);
      }
      if (!argv['no-tinybench']) {
        for (const t of tools.filter((x) => x.variant)) {
          log(`> tinybench: PurgeCSS API in-process (${t.variant})`);
          entry.tools[t.id].inProcessApi = tinybench(corpus, files, t.variant);
        }
      }
    }

    log('> correctness diff');
    entry.correctness = correctness(corpus, files, outputs, minInput.css);
    results.corpora.push(entry);
  }

  const date = new Date().toISOString().slice(0, 10);
  const host = os.hostname().replace(/[^A-Za-z0-9-]/g, '-').slice(0, 40);
  const base = join(BENCH_DIR, 'results', `${date}-${host}${QUICK ? '-quick' : ''}`);
  mkdirSync(join(BENCH_DIR, 'results'), { recursive: true });
  writeFileSync(`${base}.json`, `${JSON.stringify(results, null, 1)}\n`);
  writeFileSync(`${base}.md`, renderMarkdown(results, CATEGORIES));
  log(`\nwrote ${r(base)}.json and .md`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
