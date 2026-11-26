// Synthetic scaling corpus: N rules x M content files, fixed seed.
//
// Class names are `u<i>` for the "used" pool (referenced from content) and
// `x<i>` for the never-used pool, so roughly `USED_FRACTION` of the
// stylesheet survives. Rules mix simple classes, compounds, descendants,
// pseudo-classes, @media blocks and a few tag/attribute selectors, which are
// the shapes real framework CSS is made of.
import { join } from 'node:path';
import { rng, writeFile } from '../lib/util.mjs';

export const SYNTHETIC_SIZES = {
  small: { rules: 1_000, files: 10 },
  medium: { rules: 10_000, files: 100 },
  large: { rules: 50_000, files: 500 },
  huge: { rules: 200_000, files: 2_000 },
};

const USED_FRACTION = 0.2;
const ELEMENTS_PER_FILE = 150;
const TAGS = ['div', 'span', 'a', 'p', 'li', 'section', 'button'];
const PROPS = ['color:#333', 'margin:0 auto', 'padding:4px 8px', 'display:flex', 'width:50%', 'font-weight:600', 'border:1px solid #ccc'];

export function generateSynthetic(dir, { rules, files }, seed = 0x5eed) {
  const r = rng(seed ^ rules);
  // One class name per rule on average; the pools are sized so the used
  // fraction of rules stays roughly constant across sizes.
  const usedPool = Math.max(10, Math.round(rules * USED_FRACTION));
  const unusedPool = Math.max(10, rules - usedPool);
  const name = () => (r.chance(USED_FRACTION) ? `u${r.int(usedPool)}` : `x${r.int(unusedPool)}`);

  const css = [];
  for (let i = 0; i < rules; i++) {
    const decl = `{${r.pick(PROPS)};${r.pick(PROPS)}}`;
    const k = r.int(100);
    let sel;
    if (k < 55) sel = `.${name()}`;
    else if (k < 65) sel = `.${name()}.${name()}`;
    else if (k < 75) sel = `.${name()} .${name()}`;
    else if (k < 82) sel = `.${name()}:hover`;
    else if (k < 87) sel = `.${name()} > ${r.pick(TAGS)}`;
    else if (k < 90) sel = `${r.pick(TAGS)}.${name()}`;
    else if (k < 92) sel = `.${name()}[data-state="open"]`;
    else if (k < 94) sel = `.${name()}::before`;
    else sel = `.${name()}, .${name()}`;
    const rule = `${sel} ${decl}`;
    css.push(k >= 96 ? `@media (min-width: ${r.pick([640, 768, 1024])}px) { ${rule} }` : rule);
  }
  writeFile(join(dir, 'styles.css'), `${css.join('\n')}\n`);

  for (let f = 0; f < files; f++) {
    const els = [];
    for (let e = 0; e < ELEMENTS_PER_FILE; e++) {
      const tag = r.pick(TAGS);
      const n = 1 + r.int(4);
      const cls = Array.from({ length: n }, () => `u${r.int(usedPool)}`).join(' ');
      els.push(`  <${tag} class="${cls}" data-i="${e}">item ${e}</${tag}>`);
    }
    writeFile(join(dir, 'content', `f${String(f).padStart(5, '0')}.html`), `<!doctype html>\n<html><body>\n${els.join('\n')}\n</body></html>\n`);
  }
}
