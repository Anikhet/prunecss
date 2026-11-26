// Selector-level comparison of pruned stylesheets.
//
// Each stylesheet (already passed through the shared minifier, so both tools'
// selectors are printed the same way) is reduced to a set of keys:
//   "<at-rule context> | <one selector>"   for every selector of every style rule
//   "@keyframes spin", "@font-face Inter"  for rules without selectors
// Diffing those sets says exactly which selectors one tool kept and the other
// removed, independent of formatting and rule order.
import postcss from 'postcss';
import selectorParser from 'postcss-selector-parser';

const UNIT_AT_RULES = new Set(['keyframes', '-webkit-keyframes', '-moz-keyframes', 'font-face', 'property', 'counter-style', 'font-feature-values', 'page', 'font-palette-values', 'view-transition']);

/** Returns Map<key, {selector, context}> for a minified stylesheet. */
export function selectorSet(css) {
  const out = new Map();
  const root = postcss.parse(css);
  const contextOf = (node) => {
    const parts = [];
    for (let p = node.parent; p && p.type !== 'root'; p = p.parent) {
      if (p.type === 'atrule') parts.unshift(`@${p.name} ${p.params}`.trim());
      else if (p.type === 'rule') parts.unshift(p.selector);
    }
    return parts.join(' ');
  };
  root.walk((node) => {
    if (node.type === 'atrule') {
      if (UNIT_AT_RULES.has(node.name)) {
        let label = `@${node.name} ${node.params}`.trim();
        if (node.name === 'font-face') {
          const fam = node.nodes?.find((d) => d.prop === 'font-family');
          label += ` ${fam ? fam.value : ''}`;
        }
        const ctx = contextOf(node);
        out.set(`${ctx} | ${label}`, { selector: label, context: ctx, atRule: node.name });
      } else if (!node.nodes) {
        // Statement at-rules such as `@layer a, b;` or `@import`.
        const label = `@${node.name} ${node.params}`.trim();
        out.set(` | ${label}`, { selector: label, context: '', atRule: node.name });
      }
      return;
    }
    if (node.type !== 'rule') return;
    // Keyframe steps (`from`, `50%`) belong to their @keyframes unit.
    for (let p = node.parent; p && p.type !== 'root'; p = p.parent) {
      if (p.type === 'atrule' && UNIT_AT_RULES.has(p.name)) return;
    }
    const ctx = contextOf(node);
    for (const selector of node.selectors) {
      out.set(`${ctx} | ${selector}`, { selector, context: ctx });
    }
  });
  return out;
}

const FUNCTIONAL = new Set([':not', ':is', ':where', ':has', ':matches', ':-webkit-any', ':-moz-any', ':nth-child', ':nth-last-child']);

/** Parses a selector and returns the features the classifier and oracle need. */
export function analyse(selector) {
  const f = { escaped: selector.includes('\\'), classes: [], ids: [], tags: [], attributes: 0, pseudos: [], functional: [], universal: false, nestedParent: false };
  try {
    selectorParser((root) => {
      root.walk((n) => {
        // Names inside :not()/:has() etc. are not requirements for a match.
        let insideFunctional = false;
        for (let p = n.parent; p; p = p.parent) {
          if (p.type === 'pseudo' && FUNCTIONAL.has(p.value.toLowerCase())) insideFunctional = true;
        }
        switch (n.type) {
          case 'class': if (!insideFunctional) f.classes.push(n.value); break;
          case 'id': if (!insideFunctional) f.ids.push(n.value); break;
          case 'tag': if (!insideFunctional) f.tags.push(n.value.toLowerCase()); break;
          case 'attribute': f.attributes++; break;
          case 'universal': f.universal = true; break;
          case 'nesting': f.nestedParent = true; break;
          case 'pseudo':
            if (FUNCTIONAL.has(n.value.toLowerCase()) && n.nodes?.length) f.functional.push(n.value.toLowerCase());
            else f.pseudos.push(n.value.toLowerCase());
            break;
          default:
        }
      });
    }).processSync(selector);
  } catch {
    f.unparsable = true;
  }
  return f;
}

export const CATEGORIES = {
  'at-rule': 'Rule without a selector (@keyframes, @font-face, @property, @layer statement, ...)',
  rewritten: 'Selector text not present in the input: the tool rewrote it (PurgeCSS trims unused items out of :is() lists; prunecss\'s printer renames some vendor pseudo-classes). In the false-removal table: the input selector is missing because the tool printed it in a different form',
  escaped: 'Escaped class name (Tailwind-style `md\\:flex`, `w-1\\/2`, `w-\\[10px\\]`)',
  id: 'Contains an #id',
  'tag-only': 'Only tags / universal / pseudo-classes: no class, id or attribute (e.g. `table`, `h1`, `::selection`)',
  attribute: 'Contains an [attribute] selector',
  functional: 'Contains :not() / :is() / :where() / :has()',
  pseudo: 'Contains a pseudo-class or pseudo-element',
  class: 'Plain class selector(s): the tools found different class names in the content',
};

/** Picks the single most explanatory category for a selector that only one tool kept. */
export function classify(entry, inputKeys, key) {
  if (entry.atRule) return 'at-rule';
  if (!inputKeys.has(key)) return 'rewritten';
  const f = analyse(entry.selector);
  if (f.escaped) return 'escaped';
  if (f.ids.length) return 'id';
  if (f.attributes) return 'attribute';
  if (!f.classes.length) return 'tag-only';
  if (f.functional.length) return 'functional';
  if (f.pseudos.length) return 'pseudo';
  return 'class';
}

/** Diffs two selector maps. Returns arrays of keys. */
export function diffSets(a, b) {
  const onlyA = [];
  const onlyB = [];
  let both = 0;
  for (const k of a.keys()) (b.has(k) ? both++ : onlyA.push(k));
  for (const k of b.keys()) if (!a.has(k)) onlyB.push(k);
  return { both, onlyA: onlyA.sort(), onlyB: onlyB.sort() };
}
