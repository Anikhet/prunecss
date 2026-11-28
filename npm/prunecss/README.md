# prunecss

**Remove unused CSS, fast.** A Rust alternative to
[PurgeCSS](https://github.com/FullHuman/purgecss): prunecss scans your HTML,
JSX and templates for the class names they use, then removes the CSS rules
that can never match. This package installs the prebuilt `prunecss` binary
for your platform; no Rust needed.

```bash
npx prunecss prune dist/app.css --content 'src/**/*.{html,jsx,tsx,vue,svelte,astro}' -o dist/app.pruned.css
```

Or add it to a project and run it after your build (prunecss never
overwrites the stylesheet it reads, so write the result to a new file):

```bash
npm install --save-dev prunecss
```

```json
{
  "scripts": {
    "postbuild": "prunecss prune dist/app.css --content \"src/**/*.{html,tsx}\" -o dist/app.pruned.css"
  }
}
```

Settings can also live in a `prunecss.json` file; option names follow
PurgeCSS. Full documentation, options and known limits:
**https://github.com/Anikhet/prunecss**

Prebuilt for macOS (Apple silicon and Intel), Linux (x64 glibc and musl,
arm64) and Windows (x64). On other platforms, install with Rust:
`cargo install prunecss --locked`.

Licensed under MIT or Apache-2.0, at your option.
