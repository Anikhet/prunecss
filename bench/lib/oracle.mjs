// A static-DOM "oracle": does a selector match any element of the content?
//
// This is a lower bound on what the pages need, not a browser ground truth.
// Content files are parsed as HTML (Astro/Jekyll/JSX are close enough for
// their markup parts), pseudo-classes and pseudo-elements are stripped
// (`.btn:hover` counts as used if any `.btn` exists, because hover can
// happen), and the rest is matched with css-select. Classes added at runtime
// by JavaScript are invisible to it, exactly as they are to both tools.
import { readFileSync } from 'node:fs';
import { parseDocument } from 'htmlparser2';
import { compile } from 'css-select';
import selectorParser from 'postcss-selector-parser';
import { analyse } from './selectors.mjs';

export class Oracle {
  constructor(files) {
    this.docs = files.map((f) => {
      let text = readFileSync(f, 'utf8');
      // Fragments (Astro pages, Jekyll docs, components) are rendered inside
      // a layout, so give them the <html>/<body> the browser would.
      if (!/<body[\s>]/i.test(text)) text = `<html><body>${text}</body></html>`;
      return parseDocument(text, { lowerCaseAttributeNames: true });
    });
    this.classes = new Set();
    this.ids = new Set();
    this.tags = new Set();
    this.elements = [];
    const walk = (nodes) => {
      for (const n of nodes) {
        if (n.type === 'tag' || n.type === 'script' || n.type === 'style') {
          this.elements.push(n);
          this.tags.add(n.name.toLowerCase());
          const cls = n.attribs?.class;
          if (cls) for (const c of cls.split(/\s+/)) if (c) this.classes.add(c);
          if (n.attribs?.id) this.ids.add(n.attribs.id);
        }
        if (n.children) walk(n.children);
      }
    };
    for (const d of this.docs) walk(d.children);
    this.cache = new Map();
  }

  /**
   * Returns { match: boolean, approx: boolean, error?: string }.
   * `approx` means a functional pseudo-class (:not/:is/:has/...) was
   * dropped, which can only make the selector broader.
   */
  matches(selector) {
    const hit = this.cache.get(selector);
    if (hit) return hit;
    const result = this.#matches(selector);
    this.cache.set(selector, result);
    return result;
  }

  #matches(selector) {
    const f = analyse(selector);
    if (f.unparsable) return { match: false, approx: true, error: 'unparsable' };
    // Cheap pre-filter: every required class/id/tag must exist somewhere.
    if (!f.classes.every((c) => this.classes.has(c)) || !f.ids.every((i) => this.ids.has(i)) || !f.tags.every((t) => this.tags.has(t))) {
      return { match: false, approx: false };
    }
    const { text, approx } = stripPseudos(selector);
    try {
      // compile() returns a predicate over one element that checks
      // ancestors/siblings itself, so test every element of every page.
      const test = compile(text);
      return { match: this.elements.some((el) => test(el)), approx };
    } catch (err) {
      return { match: false, approx: true, error: String(err.message ?? err) };
    }
  }
}

// Pseudo-classes that depend only on the document tree, which css-select can
// evaluate. Everything else (:hover, :focus, :checked, ::before, :-moz-*, ...)
// is a state or a pseudo-element that *can* apply to the element, so it is
// dropped and the selector is matched without it.
const STRUCTURAL = new Set([':is', ':where', ':not', ':has', ':matches', ':-webkit-any', ':-moz-any', ':first-child', ':last-child', ':only-child', ':nth-child', ':nth-last-child', ':nth-of-type', ':nth-last-of-type', ':first-of-type', ':last-of-type', ':only-of-type', ':empty', ':root']);
const ALIASES = { ':matches': ':is', ':-webkit-any': ':is', ':-moz-any': ':is' };
const NEGATING = new Set([':not', ':has']);

const depth = (n) => {
  let d = 0;
  for (let p = n.parent; p; p = p.parent) d++;
  return d;
};

/**
 * Removes state pseudo-classes and pseudo-elements, keeping structural ones.
 * A `:not()`/`:has()` whose argument had to be changed is dropped entirely
 * (the result is broader, so `approx` is set). Emptied compounds become `*`.
 */
export function stripPseudos(selector) {
  let approx = false;
  const out = selectorParser((root) => {
    const pseudos = [];
    root.walkPseudos((p) => pseudos.push(p));
    root.walkNesting(() => {
      approx = true;
    });
    const changed = new Set();
    const markAncestors = (n) => {
      for (let p = n.parent; p; p = p.parent) if (p.type === 'pseudo') changed.add(p);
    };
    // Deepest first, so a :not() knows whether its argument was changed.
    for (const p of pseudos.sort((a, b) => depth(b) - depth(a))) {
      const name = p.value.toLowerCase();
      if (!STRUCTURAL.has(name) || (NEGATING.has(name) && changed.has(p))) {
        if (NEGATING.has(name) || p.nodes?.length) approx = true;
        markAncestors(p);
        p.remove();
      } else if (ALIASES[name]) {
        p.value = ALIASES[name];
      }
    }
    const selectors = [];
    root.walk((n) => {
      if (n.type === 'selector') selectors.push(n);
    });
    for (const sel of selectors) {
      const relative = sel.parent?.type === 'pseudo' && sel.parent.value.toLowerCase() === ':has';
      const nodes = [...sel.nodes];
      if (!nodes.length) {
        sel.append(selectorParser.universal());
        continue;
      }
      sel.removeAll();
      let open = !relative; // at start (outside :has) or right after a combinator
      for (const n of nodes) {
        if (n.type === 'combinator') {
          if (open) sel.append(selectorParser.universal());
          sel.append(n);
          open = true;
        } else {
          sel.append(n);
          open = false;
        }
      }
      if (open) sel.append(selectorParser.universal());
    }
  }).processSync(selector);
  return { text: out.trim() || '*', approx };
}
