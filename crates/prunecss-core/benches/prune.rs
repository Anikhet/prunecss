//! Time of [`prunecss_core::prune`] (parse, match, print) on framework-sized CSS.
//!
//! Run with `cargo bench -p prunecss-core --bench prune` (add `-- --quick`
//! for a fast smoke run).
//!
//! - `prune/bootstrap-like` always runs: a stylesheet generated here with
//!   Bootstrap's shapes (components x colour variants, pseudo-classes,
//!   responsive utilities in `@media`, custom properties, keyframes), about
//!   90 kB (a third of `bootstrap.css`), and a used set of ~15% of its classes.
//! - `prune/bootstrap-5.3.8` also runs when `bench/corpus/` has been fetched
//!   (`npm run fetch` in `bench/`): the real Bootstrap CSS, with the classes
//!   used by Bootstrap's own example pages.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use prunecss_core::{PruneOptions, extract_classes, prune};

const COMPONENTS: [&str; 20] = [
    "btn",
    "card",
    "alert",
    "badge",
    "nav",
    "navbar",
    "dropdown",
    "modal",
    "list-group",
    "table",
    "form-control",
    "toast",
    "tooltip",
    "popover",
    "progress",
    "accordion",
    "carousel",
    "offcanvas",
    "pagination",
    "breadcrumb",
];
const COLORS: [&str; 8] = [
    "primary",
    "secondary",
    "success",
    "danger",
    "warning",
    "info",
    "light",
    "dark",
];
const BREAKPOINTS: [(&str, u32); 5] = [
    ("sm", 576),
    ("md", 768),
    ("lg", 992),
    ("xl", 1200),
    ("xxl", 1400),
];
const SPACING: [&str; 14] = [
    "m", "mt", "mb", "ms", "me", "mx", "my", "p", "pt", "pb", "ps", "pe", "px", "py",
];
const DISPLAYS: [&str; 6] = ["none", "inline", "inline-block", "block", "flex", "grid"];

/// Generates a Bootstrap-shaped stylesheet and the list of class names in it.
fn bootstrap_like() -> (String, Vec<String>) {
    let mut css = String::with_capacity(300 << 10);
    let mut classes = Vec::new();
    css.push_str(":root{");
    for (i, color) in COLORS.iter().enumerate() {
        let _ = write!(
            css,
            "--bs-{color}:#{:06x};--bs-{color}-rgb:{i},{i},{i};",
            i * 0x0011_2233
        );
    }
    css.push_str("}\n*,::after,::before{box-sizing:border-box}\nbody{margin:0}\na{color:var(--bs-primary)}\n[hidden]{display:none!important}\n");
    css.push_str("@keyframes progress-bar-stripes{0%{background-position-x:1rem}}\n");

    for component in COMPONENTS {
        classes.push(component.to_owned());
        let _ = writeln!(
            css,
            ".{component}{{display:block;padding:.375rem .75rem;border:1px solid transparent;border-radius:.375rem}}\n\
             .{component}:hover{{text-decoration:none}}\n\
             .{component}:focus-visible{{outline:0;box-shadow:0 0 0 .25rem rgba(13,110,253,.25)}}\n\
             .{component}:disabled,.{component}.disabled,fieldset:disabled .{component}{{pointer-events:none;opacity:.65}}\n\
             .{component}>.{component}-body:not(:last-child){{margin-bottom:0}}\n\
             .{component}::before{{content:\"\"}}"
        );
        for part in [
            "header", "body", "footer", "title", "item", "link", "toggle",
        ] {
            classes.push(format!("{component}-{part}"));
            let _ = writeln!(
                css,
                ".{component}-{part}{{padding:.5rem 1rem;color:var(--bs-body-color)}}\n\
                 .{component} .{component}-{part}+.{component}-{part}{{border-top:0}}"
            );
        }
        for color in COLORS {
            classes.push(format!("{component}-{color}"));
            classes.push(format!("{component}-outline-{color}"));
            let _ = writeln!(
                css,
                ".{component}-{color}{{--bs-{component}-color:#fff;--bs-{component}-bg:var(--bs-{color});color:var(--bs-{component}-color);background-color:var(--bs-{component}-bg)}}\n\
                 .{component}-{color}:hover,.{component}-check:checked+.{component}-{color}{{filter:brightness(90%)}}\n\
                 .{component}-outline-{color}{{color:var(--bs-{color});border-color:var(--bs-{color})}}"
            );
        }
    }

    for prop in SPACING {
        for n in 0..=5 {
            classes.push(format!("{prop}-{n}"));
            let _ = writeln!(css, ".{prop}-{n}{{margin:{n}rem!important}}");
        }
    }
    for display in DISPLAYS {
        classes.push(format!("d-{display}"));
        let _ = writeln!(css, ".d-{display}{{display:{display}!important}}");
    }
    for (bp, width) in BREAKPOINTS {
        let _ = writeln!(css, "@media (min-width:{width}px){{");
        for n in 1..=12 {
            classes.push(format!("col-{bp}-{n}"));
            let _ = writeln!(
                css,
                ".col-{bp}-{n}{{flex:0 0 auto;width:{:.4}%}}",
                f64::from(n) * 100.0 / 12.0
            );
        }
        for prop in SPACING {
            for n in 0..=5 {
                classes.push(format!("{prop}-{bp}-{n}"));
                let _ = writeln!(css, ".{prop}-{bp}-{n}{{margin:{n}rem!important}}");
            }
        }
        for display in DISPLAYS {
            classes.push(format!("d-{bp}-{display}"));
            let _ = writeln!(css, ".d-{bp}-{display}{{display:{display}!important}}");
        }
        css.push_str("}\n");
    }
    (css, classes)
}

fn bench_bootstrap_like(c: &mut Criterion) {
    let (css, classes) = bootstrap_like();
    // Every 7th class (~15%) counts as used, like one site using a slice of
    // a framework.
    let used: BTreeSet<&str> = classes.iter().step_by(7).map(String::as_str).collect();
    let options = PruneOptions::default();

    let mut group = c.benchmark_group("prune");
    group.throughput(Throughput::Bytes(css.len() as u64));
    group.bench_function("bootstrap-like", |b| {
        b.iter(|| {
            prune(black_box(&css), black_box(&used), &options).map(|out| out.removed_selectors)
        });
    });
    group.finish();
}

/// Loads the fetched Bootstrap corpus, if present: (css, example pages).
fn real_bootstrap() -> Option<(String, Vec<String>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/corpus/bootstrap");
    let css = std::fs::read_to_string(dir.join("bootstrap.css")).ok()?;
    let pages = std::fs::read_dir(dir.join("examples"))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("index.astro")).ok())
        .collect::<Vec<_>>();
    (!pages.is_empty()).then_some((css, pages))
}

fn bench_real_bootstrap(c: &mut Criterion) {
    let Some((css, pages)) = real_bootstrap() else {
        eprintln_skip();
        return;
    };
    let used: BTreeSet<std::borrow::Cow<'_, str>> =
        pages.iter().flat_map(|p| extract_classes(p)).collect();
    let options = PruneOptions::default();

    let mut group = c.benchmark_group("prune");
    group.throughput(Throughput::Bytes(css.len() as u64));
    group.bench_function("bootstrap-5.3.8", |b| {
        b.iter(|| {
            prune(black_box(&css), black_box(&used), &options).map(|out| out.removed_selectors)
        });
    });
    group.finish();
}

#[expect(
    clippy::print_stderr,
    reason = "tell the person running the bench why a group was skipped"
)]
fn eprintln_skip() {
    eprintln!(
        "prune/bootstrap-5.3.8 skipped: run `npm ci && npm run fetch` in bench/ to fetch the corpus"
    );
}

criterion_group!(benches, bench_bootstrap_like, bench_real_bootstrap);
criterion_main!(benches);
