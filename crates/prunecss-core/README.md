# prunecss-core

The engine behind [`prunecss`](https://github.com/Anikhet/prunecss): find the
class names a project uses, then remove the CSS rules that can never match.

```rust
use std::collections::BTreeSet;
use prunecss_core::{extract_classes, prune, PruneOptions};

let html = r#"<button class="btn btn-primary">Save</button>"#;
let css = ".btn { padding: 1rem } .btn-primary { color: white } .card { border: 1px solid }";

let used: BTreeSet<&str> = extract_classes(html);
let output = prune(css, &used, &PruneOptions::default().minify(true))?;

assert_eq!(output.css, ".btn{padding:1rem}.btn-primary{color:#fff}");
assert_eq!(output.removed_selectors, 1);
# Ok::<(), prunecss_core::PruneError>(())
```

Matching is conservative: a selector is kept when every class and id it needs
appears in the content. Tags, attributes, combinators and pseudo-classes never
cause a removal. See the [project README](https://github.com/Anikhet/prunecss)
for the full rules and known limits.

**Status:** early development, `0.x`. The API may change between minor
versions; see the changelog.

Minimum supported Rust version: 1.88.

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT](LICENSE-MIT) at your option.
