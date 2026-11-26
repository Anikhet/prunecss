//! End-to-end tests that run the real `prunecss` binary.

#![expect(clippy::panic, reason = "test helpers should fail loudly")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/site")
        .join(name)
}

fn prunecss(args: &[&str]) -> Output {
    match Command::new(env!("CARGO_BIN_EXE_prunecss"))
        .args(args)
        .output()
    {
        Ok(output) => output,
        Err(err) => panic!("could not run prunecss: {err}"),
    }
}

fn path_arg(path: &Path) -> &str {
    path.to_str()
        .unwrap_or_else(|| panic!("non-UTF-8 path: {}", path.display()))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn prune_fixture_site(extra: &[&str]) -> Output {
    let css = fixture("styles.css");
    let html = fixture("index.html");
    let jsx = fixture("App.jsx");
    let mut args = vec![
        "prune",
        path_arg(&css),
        "--content",
        path_arg(&html),
        path_arg(&jsx),
    ];
    args.extend_from_slice(extra);
    prunecss(&args)
}

#[test]
fn prune_should_remove_unused_rules_from_the_fixture_site() {
    let output = prune_fixture_site(&[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn prune_should_print_minified_css_when_asked() {
    let output = prune_fixture_site(&["--minify"]);
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn prune_should_report_stats_on_stderr() {
    let output = prune_fixture_site(&["--stats"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("removed 6 selectors"), "stderr: {stderr}");
}

#[test]
fn prune_should_write_to_the_output_file() {
    let out = std::env::temp_dir().join(format!("prunecss-test-{}.css", std::process::id()));
    let output = prune_fixture_site(&["--output", path_arg(&out)]);
    let written = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(written.contains(".btn-primary"));
}

#[test]
fn prune_should_fail_with_a_clear_message_for_a_missing_file() {
    let output = prunecss(&["prune", "missing.css", "--content", "missing.html"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.starts_with("error: reading missing.css"),
        "stderr: {stderr}"
    );
}

#[test]
fn scan_should_list_classes_from_every_file() {
    let html = fixture("index.html");
    let jsx = fixture("App.jsx");
    let output = prunecss(&["scan", path_arg(&html), path_arg(&jsx)]);
    insta::assert_snapshot!(stdout(&output));
}

fn framework_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/frameworks")
        .join(name)
}

fn prune_framework_fixture(extra: &[&str]) -> Output {
    let css = framework_fixture("styles.css");
    let components: Vec<PathBuf> = [
        "Button.jsx",
        "Card.tsx",
        "Alert.vue",
        "Item.svelte",
        "Badge.astro",
    ]
    .iter()
    .map(|name| framework_fixture(name))
    .collect();
    let mut args = vec!["prune", path_arg(&css), "--content"];
    args.extend(components.iter().map(|path| path_arg(path)));
    args.extend_from_slice(extra);
    prunecss(&args)
}

// Classes used through clsx, template literals, Vue/Svelte/Astro bindings,
// CSS modules and DOM APIs must all survive; only the two `unused-*` go.
#[test]
fn prune_should_keep_classes_used_by_framework_components() {
    let output = prune_framework_fixture(&["--minify"]);
    insta::assert_snapshot!(stdout(&output));
}

#[test]
fn prune_should_remove_only_unused_classes_from_framework_components() {
    let output = prune_framework_fixture(&["--stats"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("removed 2 selectors"), "stderr: {stderr}");
}
