// Output size, measured like for like: every stylesheet (input and each
// tool's output) goes through the same pinned minifier first, so differences
// reflect what was pruned rather than how each tool formats its output.
import { transform } from 'lightningcss';
import { gzipSync, brotliCompressSync, constants } from 'node:zlib';

/** Minifies with lightningcss (pinned in package-lock). Returns the CSS and any warnings. */
export function minify(css, filename = 'input.css') {
  const { code, warnings } = transform({
    filename,
    code: Buffer.from(css),
    minify: true,
    // Real-world CSS contains a few rules Lightning CSS rejects; skip them
    // (in every tool's output alike) instead of failing the measurement.
    errorRecovery: true,
  });
  return { css: code.toString(), warnings: warnings.length };
}

export function sizes(rawCss, minified) {
  const buf = Buffer.from(minified);
  return {
    raw: Buffer.byteLength(rawCss),
    minified: buf.length,
    gzip: gzipSync(buf, { level: 9 }).length,
    brotli: brotliCompressSync(buf, {
      params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: buf.length },
    }).length,
  };
}
