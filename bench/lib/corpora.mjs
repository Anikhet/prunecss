// Corpus definitions shared by fetch-corpus.mjs and run.mjs.
//
// Every corpus is one stylesheet plus a list of content files. The harness
// expands the content globs itself and hands both tools the same explicit
// file list (see `contentArgs` in run.mjs): prunecss cannot take globs yet.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { CORPUS_DIR, listFiles } from './util.mjs';
import { SYNTHETIC_SIZES } from '../scripts/gen-synthetic.mjs';

export const BOOTSTRAP_TAG = 'v5.3.8';
export const BULMA_TAG = '1.0.4';

// Bootstrap's own example pages at the tag matching the npm CSS. The RTL
// variants are left out because they use bootstrap.rtl.css.
export const BOOTSTRAP_EXAMPLES = [
  'album', 'badges', 'blog', 'breadcrumbs', 'buttons', 'carousel', 'cheatsheet', 'checkout', 'cover',
  'dashboard', 'dropdowns', 'features', 'footers', 'grid', 'headers', 'heroes', 'jumbotron', 'jumbotrons',
  'list-groups', 'masonry', 'modals', 'navbar-bottom', 'navbar-fixed', 'navbar-static', 'navbars',
  'navbars-offcanvas', 'offcanvas-navbar', 'pricing', 'product', 'sidebars', 'sign-in', 'starter-template',
  'sticky-footer', 'sticky-footer-navbar',
];

// Bulma's documentation pages (Jekyll sources) at the tag matching the npm CSS.
export const BULMA_DOCS = [
  'components/breadcrumb', 'components/card', 'components/dropdown', 'components/menu', 'components/message',
  'components/modal', 'components/navbar', 'components/pagination', 'components/panel', 'components/tabs',
  'elements/block', 'elements/box', 'elements/button', 'elements/content', 'elements/delete', 'elements/icon',
  'elements/image', 'elements/notification', 'elements/progress', 'elements/table', 'elements/tag', 'elements/title',
  'form/general', 'form/input', 'form/textarea', 'form/select', 'form/checkbox', 'form/radio', 'form/file',
  'columns/basics', 'columns/sizes', 'columns/responsiveness', 'columns/nesting', 'columns/gap', 'columns/options',
  'layout/container', 'layout/level', 'layout/media-object', 'layout/hero', 'layout/section', 'layout/footer',
  'grid/smart-grid', 'grid/fixed-grid', 'grid/grid-cells', 'helpers/color-helpers', 'helpers/spacing-helpers',
  'helpers/typography-helpers', 'helpers/visibility-helpers', 'helpers/flexbox-helpers', 'helpers/other-helpers',
];

const d = (...p) => join(CORPUS_DIR, ...p);

/** Returns every corpus definition; `available` says whether it has been fetched/generated. */
export function corpora() {
  const list = [
    {
      name: 'bootstrap',
      description: `Bootstrap ${BOOTSTRAP_TAG.slice(1)} dist/css/bootstrap.css vs the ${BOOTSTRAP_EXAMPLES.length} example pages (site/src/assets/examples/*/index.astro) from twbs/bootstrap@${BOOTSTRAP_TAG}`,
      css: d('bootstrap', 'bootstrap.css'),
      contentDir: d('bootstrap', 'examples'),
      contentRe: /\.astro$/,
    },
    {
      name: 'bulma',
      description: `Bulma ${BULMA_TAG} css/bulma.css vs ${BULMA_DOCS.length} documentation pages (docs/documentation/**/*.html) from jgthms/bulma@${BULMA_TAG}`,
      css: d('bulma', 'bulma.css'),
      contentDir: d('bulma', 'docs'),
      contentRe: /\.html$/,
    },
    {
      name: 'tailwind',
      description: 'Tailwind v4 build (@tailwindcss/cli, broad @source inline() safelist) vs 40 generated pages using variants, fractions and arbitrary values',
      css: d('tailwind', 'tailwind.css'),
      contentDir: d('tailwind', 'pages'),
      contentRe: /\.html$/,
      // Report PurgeCSS with its default extractor and with the Tailwind one.
      purgecssVariants: ['default', 'tailwind-extractor'],
    },
    {
      name: 'edge',
      description: 'Hand-written edge cases (ids, tags, attributes, JS-added classes, Vue/JSX/Svelte syntax, escaped names); committed in corpus/edge',
      css: d('edge', 'styles.css'),
      contentDir: d('edge'),
      contentRe: /\.(html|jsx|vue|svelte)$/,
      // Too small for timing to mean anything; correctness only.
      correctnessOnly: true,
    },
    ...Object.entries(SYNTHETIC_SIZES).map(([size, { rules, files }]) => ({
      name: `synthetic-${size}`,
      description: `Synthetic: ${rules.toLocaleString('en')} rules x ${files.toLocaleString('en')} HTML files (fixed seed)`,
      css: d('synthetic', size, 'styles.css'),
      contentDir: d('synthetic', size, 'content'),
      contentRe: /\.html$/,
      synthetic: { size, rules, files },
    })),
  ];
  return list.map((c) => ({
    purgecssVariants: ['default'],
    ...c,
    available: existsSync(c.css) && existsSync(c.contentDir),
  }));
}

/** Expands a corpus's content "glob" into a sorted list of absolute paths. */
export function contentFiles(corpus) {
  return listFiles(corpus.contentDir, corpus.contentRe);
}
