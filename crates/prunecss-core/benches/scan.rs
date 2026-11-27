//! Throughput of [`prunecss_core::extract_classes`] on scaled-up markup.
//!
//! Run with `cargo bench -p prunecss-core --bench scan` (add `-- --quick`
//! for a fast smoke run). Input is generated here, so no corpus fetch is
//! needed. End-to-end comparisons with PurgeCSS live in `bench/`.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use prunecss_core::extract_classes;

/// The CLI test fixture's page body (`crates/prunecss-cli/tests/fixtures/site/index.html`),
/// with a `{n}` placeholder so repeated copies use different class names, as
/// a real site's pages would.
const FIXTURE_BODY: &str = r#"    <nav class="navbar navbar-dark md:flex nav-{n}">
      <a class="btn btn-primary btn-{n}" href="/" data-class="not-a-class">Home</a>
    </nav>
    <main class="container">
      <div class="card card--{n} hover:bg-blue-500 w-1/2">
        <h2 class="card-title text-{n}">Hello</h2>
        <p>Some prose with the word class in it and an <code>class=</code> example.</p>
        <button className="toggle toggle-{n}" type="button">JSX attribute</button>
      </div>
    </main>
"#;

/// Builds an HTML document of roughly `target` bytes from copies of the fixture.
fn page(target: usize) -> String {
    let mut html = String::with_capacity(target + 1024);
    html.push_str("<!doctype html>\n<html>\n  <body>\n");
    let mut n = 0usize;
    while html.len() < target {
        // 500 distinct suffixes keeps the class set realistic (a few
        // thousand names) instead of growing without bound.
        let body = FIXTURE_BODY.replace("{n}", &(n % 500).to_string());
        html.push_str(&body);
        n += 1;
    }
    let _ = writeln!(html, "  </body>\n</html>");
    html
}

fn bench_extract_classes(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_classes");
    for (label, size) in [("64KiB", 64 << 10), ("1MiB", 1 << 20), ("8MiB", 8 << 20)] {
        let html = page(size);
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &html, |b, html| {
            b.iter(|| extract_classes(black_box(html)).len());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_extract_classes);
criterion_main!(benches);
