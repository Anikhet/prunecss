// PurgeCSS config used for the "purgecss (tailwind-extractor)" rows.
// Only the extractor changes; css/content come from the command line so
// every tool sees the same files. The regex is PurgeCSS/Tailwind's documented
// extractor for class names containing `:`, `/`, `[`, `]`.
export default {
  defaultExtractor: (content) => content.match(/[^<>"'`\s]*[^<>"'`\s:]/g) || [],
};
