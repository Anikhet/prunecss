//! End-to-end tests that run the real `prunecss` binary.

#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "test helpers should fail loudly"
)]

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

    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr.starts_with("error: no stylesheet matched `missing.css`"),
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

#[test]
fn prune_should_keep_safelisted_classes() {
    let output = prune_fixture_site(&["--safelist", "navbar-light", "--minify"]);
    assert!(stdout(&output).contains(".navbar-light{"));
}

#[test]
fn prune_should_accept_regex_safelist_patterns() {
    let output = prune_fixture_site(&["--safelist", "/^btn-/", "--minify"]);
    assert!(stdout(&output).contains(".btn-danger{"));
}

#[test]
fn prune_should_keep_greedy_safelisted_selectors() {
    let output = prune_fixture_site(&["--safelist-greedy", "card-footer", "--minify"]);
    assert!(stdout(&output).contains(".card .card-footer{"));
}

#[test]
fn prune_should_remove_unused_keyframes_when_asked() {
    let output = prune_fixture_site(&["--keyframes", "--minify"]);
    assert!(!stdout(&output).contains("@keyframes"));
}

#[test]
fn prune_should_keep_keyframes_by_default() {
    let output = prune_fixture_site(&["--minify"]);
    assert!(stdout(&output).contains("@keyframes spin"));
}

#[test]
fn prune_should_report_invalid_safelist_regexes() {
    let output = prune_fixture_site(&["--safelist", "/(/"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("invalid pattern `(`"), "stderr: {stderr}");
}

#[test]
fn prune_should_report_tracked_removals_in_stats() {
    let output = prune_fixture_site(&["--keyframes", "--stats"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1 @keyframes"), "stderr: {stderr}");
}

// ---- Project mode: config files, globs, check mode, exit codes ----------

/// A small project in a temporary directory.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    dir
}

fn sample_project() -> tempfile::TempDir {
    project(&[
        ("src/index.html", r#"<div class="used"></div>"#),
        ("src/app/App.jsx", r#"<p className={clsx("also-used")} />"#),
        (
            "styles/app.css",
            ".used{color:red}.also-used{color:red}.unused{color:red}",
        ),
        ("styles/theme.css", ".used{color:blue}.gone{color:blue}"),
    ])
}

fn run_in(dir: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    use std::io::Write as _;
    let mut command = Command::new(env!("CARGO_BIN_EXE_prunecss"));
    command.current_dir(dir).args(args);
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut pipe = child.stdin.take().unwrap();
    pipe.write_all(stdin.unwrap_or_default().as_bytes())
        .unwrap();
    drop(pipe);
    child.wait_with_output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn prune_should_expand_content_globs() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "styles/app.css",
            "--content",
            "src/**/*.{html,jsx}",
            "--minify",
        ],
        None,
    );
    assert_eq!(stdout(&output), ".used{color:red}.also-used{color:red}");
}

#[test]
fn prune_should_read_settings_from_prunecss_json() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "css": ["styles/*.css"], "content": ["src/**/*"], "output": "out/", "minify": true,
             "safelist": ["gone"] }"#,
    )
    .unwrap();
    let output = run_in(dir.path(), &["prune"], None);
    let theme =
        std::fs::read_to_string(dir.path().join("out/styles/theme.css")).unwrap_or_default();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(theme, ".used{color:#00f}.gone{color:#00f}");
}

#[test]
fn prune_should_find_the_config_from_a_subdirectory() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "css": ["styles/app.css"], "content": ["src"], "minify": true }"#,
    )
    .unwrap();
    let output = run_in(&dir.path().join("src/app"), &["prune"], None);
    assert_eq!(stdout(&output), ".used{color:red}.also-used{color:red}");
}

#[test]
fn prune_should_let_flags_override_the_config() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "css": ["styles/app.css"], "content": ["src/index.html"], "minify": true }"#,
    )
    .unwrap();
    let output = run_in(dir.path(), &["prune", "--content", "src/app/App.jsx"], None);
    assert_eq!(stdout(&output), ".also-used{color:red}");
}

#[test]
fn prune_should_ignore_the_config_with_no_config() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "css": ["styles/app.css"] }"#,
    )
    .unwrap();
    let output = run_in(dir.path(), &["prune", "--no-config"], None);
    assert!(
        stderr(&output).contains("no stylesheet given"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn prune_should_reject_unknown_config_keys() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "contnet": ["src"] }"#,
    )
    .unwrap();
    let output = run_in(dir.path(), &["prune", "styles/app.css"], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("unknown field `contnet`"),
        "{}",
        stderr(&output)
    );
}

// The most dangerous mistake: a typo in --content would make every class look
// unused and delete the whole stylesheet.
#[test]
fn prune_should_refuse_to_run_when_no_content_matches() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "styles/app.css", "--content", "scr/**/*.html"],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("refusing to prune"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn prune_should_warn_about_content_patterns_that_match_nothing() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "styles/app.css",
            "--content",
            "src/**/*.html",
            "src/**/*.vue",
        ],
        None,
    );
    assert!(output.status.success());
    assert!(stderr(&output).contains("warning: content pattern `src/**/*.vue` matched no files"));
}

#[test]
fn check_should_exit_with_one_when_css_would_be_removed() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "styles/app.css", "-c", "src", "--check"],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("would remove 1 selectors"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_should_exit_with_zero_when_nothing_is_unused() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "styles/app.css",
            "-c",
            "src",
            "--check",
            "--safelist",
            "unused",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn check_should_write_nothing() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "styles/app.css",
            "-c",
            "src",
            "--check",
            "-o",
            "out.css",
        ],
        None,
    );
    assert!(output.stdout.is_empty());
    assert!(!dir.path().join("out.css").exists());
}

#[test]
fn prune_should_read_css_from_standard_input() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "-", "-c", "src", "--minify"],
        Some(".used{color:red}.x{color:red}"),
    );
    assert_eq!(stdout(&output), ".used{color:red}");
}

#[test]
fn prune_should_require_an_output_directory_for_several_stylesheets() {
    let dir = sample_project();
    let output = run_in(dir.path(), &["prune", "styles/*.css", "-c", "src"], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("2 stylesheets matched"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn prune_should_mirror_paths_into_the_output_directory() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "styles/*.css", "-c", "src", "-o", "dist"],
        None,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(dir.path().join("dist/styles/app.css").is_file());
    assert!(dir.path().join("dist/styles/theme.css").is_file());
}

#[test]
fn prune_should_print_a_json_report() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "styles/app.css", "-c", "src", "--check", "--json"],
        None,
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files"][0]["removedSelectors"], 1);
    assert_eq!(report["contentFiles"], 2);
}

#[test]
fn prune_should_refuse_json_when_css_goes_to_standard_output() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "styles/app.css", "-c", "src", "--json"],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn scan_should_filter_names_by_a_stylesheet() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["scan", "src", "--css", "styles/app.css"],
        None,
    );
    assert_eq!(stdout(&output), "also-used\nused\n");
}

#[test]
fn schema_should_print_the_committed_schema() {
    let committed = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prunecss.schema.json"),
    )
    .unwrap();
    let output = prunecss(&["schema"]);
    assert_eq!(stdout(&output), committed);
}

#[test]
fn completions_should_print_a_script() {
    let output = prunecss(&["completions", "bash"]);
    assert!(stdout(&output).contains("prunecss"));
}

#[test]
fn invalid_arguments_should_exit_with_two() {
    let output = prunecss(&["prune", "--no-such-flag"]);
    assert_eq!(output.status.code(), Some(2));
}
