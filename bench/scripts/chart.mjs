// Draws the README benchmark chart from a results file:
//   node scripts/chart.mjs results/<file>.json ../assets/benchmark.svg
// A horizontal bar chart of mean CLI wall time for the real-world corpora.
// The SVG follows the viewer's light or dark theme on its own.
import { readFileSync, writeFileSync } from 'node:fs';

const [input, output] = process.argv.slice(2);
if (!input || !output) {
  console.error('usage: node scripts/chart.mjs <results.json> <chart.svg>');
  process.exit(2);
}
const results = JSON.parse(readFileSync(input, 'utf8'));

const CORPORA = { bootstrap: 'Bootstrap 5.3', bulma: 'Bulma 1.0', tailwind: 'Tailwind v4 (4.5 MB)' };
const TOOLS = [
  { key: 'prunecss', label: 'prunecss', accent: true },
  { key: 'purgecss', label: 'PurgeCSS 9.1', accent: false },
];

const rows = [];
for (const corpus of results.corpora.filter((c) => c.name in CORPORA)) {
  for (const tool of TOOLS) {
    const mean = corpus.tools[tool.key]?.cli?.mean;
    if (mean) rows.push({ corpus: CORPORA[corpus.name], ...tool, ms: mean * 1000 });
  }
}
if (rows.length === 0) throw new Error('no timing data for the chart corpora');

const max = Math.max(...rows.map((r) => r.ms));
const width = 720, left = 110, right = 80, barHeight = 18, gap = 5, groupGap = 14, top = 32;
const plot = width - left - right;
let y = top;
let lastCorpus = null;
const parts = [];
for (const row of rows) {
  if (row.corpus !== lastCorpus) {
    if (lastCorpus !== null) y += groupGap;
    // The corpus name gets its own line above its bars.
    parts.push(`<text class="corpus" x="0" y="${y + 13}">${row.corpus}</text>`);
    y += 20;
    lastCorpus = row.corpus;
  }
  const w = Math.max(2, (row.ms / max) * plot);
  const label = row.ms >= 1000 ? `${(row.ms / 1000).toFixed(2)} s` : `${row.ms.toFixed(0)} ms`;
  parts.push(
    `<text class="tool" x="${left - 10}" y="${y + barHeight * 0.72}" text-anchor="end">${row.label}</text>`,
    `<rect class="${row.accent ? 'bar accent' : 'bar'}" x="${left}" y="${y}" width="${w.toFixed(1)}" height="${barHeight}" rx="3"/>`,
    `<text class="value" x="${left + w + 8}" y="${y + barHeight * 0.72}">${label}</text>`,
  );
  y += barHeight + gap;
}
const height = y + 26;
const date = (results.environment?.date ?? input.match(/\d{4}-\d{2}-\d{2}/)?.[0] ?? '').slice(0, 10);

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" role="img" aria-label="Benchmark: prunecss vs PurgeCSS 9.1 wall time (lower is better)">
<style>
  text { font: 13px -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; fill: #24292f; }
  .title { font-weight: 600; font-size: 14px; }
  .corpus { font-weight: 600; }
  .value, .note { fill: #57606a; }
  .note { font-size: 11px; }
  .bar { fill: #afb8c1; }
  .accent { fill: #0969da; }
  @media (prefers-color-scheme: dark) {
    text { fill: #e6edf3; }
    .value, .note { fill: #8b949e; }
    .bar { fill: #484f58; }
    .accent { fill: #2f81f7; }
  }
</style>
<text class="title" x="0" y="16">Wall time to prune, lower is better</text>
${parts.join('\n')}
<text class="note" x="0" y="${height - 6}">Mean of 10 runs of each CLI on a shared cloud VM (indicative), ${date}. Reproduce: make -C bench</text>
</svg>
`;
writeFileSync(output, svg);
console.log(`wrote ${output}`);
