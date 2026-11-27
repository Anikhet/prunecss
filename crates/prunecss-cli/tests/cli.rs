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
    assert_eq!(stdout(&output), "");
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
    assert!(stderr.contains("invalid pattern `/(/`"), "stderr: {stderr}");
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
    let theme = std::fs::read_to_string(dir.path().join("out/theme.css")).unwrap_or_default();
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
    assert_eq!(stdout(&output), "");
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
    // Mirrored from the inputs' common parent, `styles/`.
    assert!(dir.path().join("dist/app.css").is_file());
    assert!(dir.path().join("dist/theme.css").is_file());
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

// ---- Output safety (found in the pre-release review) ---------------------

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn output_directory_should_never_be_escaped_by_parent_paths() {
    let dir = project(&[
        ("site/index.html", r#"<p class="a b">"#),
        ("styles/a.css", ".a{color:red}.zz{color:red}"),
        ("styles/b.css", ".b{color:red}"),
    ]);
    let site = dir.path().join("site");
    let output = run_in(
        &site,
        &[
            "prune",
            "../styles/a.css",
            "../styles/b.css",
            "-c",
            "index.html",
            "-o",
            "../pruned",
            "--minify",
        ],
        None,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        read(&dir.path().join("styles/a.css")),
        ".a{color:red}.zz{color:red}",
        "input was overwritten"
    );
    assert_eq!(read(&dir.path().join("pruned/a.css")), ".a{color:red}");
}

#[test]
fn output_should_refuse_two_inputs_writing_the_same_file() {
    let dir = project(&[
        ("i.html", r#"<p class="a">"#),
        ("one/app.css", ".a{color:red}"),
        ("two/app.css", ".a{color:blue}"),
    ]);
    let one = dir.path().join("one/app.css");
    let two = dir.path().join("two/app.css");
    let output = run_in(
        dir.path(),
        &[
            "prune",
            path_arg(&one),
            path_arg(&two),
            "-c",
            "i.html",
            "-o",
            "out",
        ],
        None,
    );
    // Mirrored from their common parent, the two files get distinct paths.
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        dir.path().join("out/one/app.css").is_file()
            && dir.path().join("out/two/app.css").is_file()
    );
}

#[test]
fn output_should_refuse_to_overwrite_an_input() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "styles/app.css",
            "-c",
            "src",
            "-o",
            "styles/app.css",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("refusing to overwrite"),
        "{}",
        stderr(&output)
    );
    assert!(read(&dir.path().join("styles/app.css")).contains(".unused"));
}

#[test]
fn output_should_refuse_to_overwrite_an_input_through_a_directory() {
    let dir = sample_project();
    let output = run_in(
        dir.path(),
        &["prune", "styles/*.css", "-c", "src", "-o", "."],
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(read(&dir.path().join("styles/app.css")).contains(".unused"));
}

#[test]
fn output_directory_should_be_excluded_from_stylesheet_globs() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "css": ["**/*.css"], "content": ["src"], "output": "pruned/" }"#,
    )
    .unwrap();
    let first = run_in(dir.path(), &["prune"], None);
    let second = run_in(dir.path(), &["prune"], None);
    assert!(
        first.status.success() && second.status.success(),
        "{}",
        stderr(&second)
    );
    assert!(
        !dir.path().join("pruned/pruned").exists(),
        "previous output was pruned again"
    );
}

#[test]
fn config_output_dash_should_mean_standard_output() {
    let dir = sample_project();
    std::fs::write(
        dir.path().join("prunecss.json"),
        r#"{ "css": ["styles/app.css"], "content": ["src"], "output": "-", "minify": true }"#,
    )
    .unwrap();
    let output = run_in(dir.path(), &["prune"], None);
    assert_eq!(stdout(&output), ".used{color:red}.also-used{color:red}");
    assert!(!dir.path().join("-").exists());
}

#[test]
fn prune_should_refuse_content_without_any_names() {
    let dir = project(&[("empty.html", ""), ("s.css", ".a{color:red}")]);
    let output = run_in(dir.path(), &["prune", "s.css", "-c", "empty.html"], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("no class or id names"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn prune_should_warn_when_no_class_of_a_stylesheet_is_used() {
    let dir = project(&[
        ("i.html", "<p>hello</p>"),
        ("s.css", ".a{color:red}p{color:red}"),
    ]);
    let output = run_in(dir.path(), &["prune", "s.css", "-c", "i.html"], None);
    assert!(output.status.success());
    assert!(
        stderr(&output).contains("warning: none of the classes or ids in s.css"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn prune_should_accept_absolute_content_globs() {
    let dir = sample_project();
    let glob = format!(
        "{}/src/**/*.html",
        dir.path().to_string_lossy().replace('\\', "/")
    );
    let output = run_in(
        dir.path(),
        &["prune", "styles/app.css", "-c", &glob, "--minify"],
        None,
    );
    assert_eq!(stdout(&output), ".used{color:red}");
}

// ---- Unreadable rules are reported, never dropped silently ---------------

fn unreadable_project() -> tempfile::TempDir {
    project(&[
        ("i.html", r#"<p class="x y">"#),
        ("s.css", ".x{color:red}\n.x >>> .y{color:red}\n"),
    ])
}

#[test]
fn prune_should_warn_about_rules_it_drops_as_unreadable() {
    let dir = unreadable_project();
    let output = run_in(dir.path(), &["prune", "s.css", "-c", "i.html"], None);
    assert!(output.status.success());
    assert!(
        stderr(&output).contains("warning: s.css:2:"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn check_should_fail_when_unreadable_rules_would_be_dropped() {
    let dir = unreadable_project();
    let output = run_in(
        dir.path(),
        &["prune", "s.css", "-c", "i.html", "--check"],
        None,
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
}

#[test]
fn json_report_should_list_unreadable_rules() {
    let dir = unreadable_project();
    let output = run_in(
        dir.path(),
        &["prune", "s.css", "-c", "i.html", "--check", "--json"],
        None,
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["files"][0]["warnings"][0]["line"], 2);
}

// ---- Second review --------------------------------------------------------

#[test]
fn output_should_refuse_to_overwrite_a_named_input_inside_the_output_directory() {
    let dir = project(&[
        ("index.html", r#"<p class="x v">"#),
        ("src/x.css", ".x{color:red}"),
        ("src/y.css", ".y{color:red}"),
        ("src/vendor/x.css", "/* VENDOR ORIGINAL */ .v{color:red}"),
    ]);
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "src/x.css",
            "src/y.css",
            "src/vendor/x.css",
            "-c",
            "index.html",
            "-o",
            "src/vendor/",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(read(&dir.path().join("src/vendor/x.css")).contains("VENDOR ORIGINAL"));
}

#[test]
fn output_should_accept_a_named_input_inside_the_output_directory_when_safe() {
    let dir = project(&[
        ("i.html", r#"<p class="a">"#),
        ("build/css/main.css", ".a{color:red}"),
    ]);
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "build/css/main.css",
            "-c",
            "i.html",
            "-o",
            "build/",
        ],
        None,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(dir.path().join("build/main.css").is_file());
}

#[test]
fn check_should_pass_for_rules_kept_as_written() {
    let dir = project(&[
        ("i.html", r#"<p class="a">"#),
        ("ck.css", "@tailwind base;\n.a{color:red}\n"),
    ]);
    let output = run_in(
        dir.path(),
        &["prune", "ck.css", "-c", "i.html", "--check"],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!stderr(&output).contains("dropped"), "{}", stderr(&output));
}

#[test]
fn prune_should_survive_deep_nesting_on_the_main_thread() {
    let dir = project(&[("i.html", r#"<p class="a">"#)]);
    std::fs::write(
        dir.path().join("d.css"),
        ".a{b:url(x\")}".to_owned() + &".a{".repeat(5000),
    )
    .unwrap();
    let output = run_in(dir.path(), &["prune", "d.css", "-c", "i.html"], None);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
}

#[test]
fn prune_should_keep_tailwind_v4_css_variable_classes() {
    let dir = project(&[
        ("i.html", r#"<p class="bg-(--brand) supports-(--x):flex">"#),
        (
            "t.css",
            ".bg-\\(--brand\\){color:red}.supports-\\(--x\\)\\:flex{display:flex}.unused{color:blue}\n",
        ),
    ]);
    let output = run_in(dir.path(), &["prune", "t.css", "-c", "i.html"], None);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        ".bg-\\(--brand\\) {\n  color: red;\n}\n\n.supports-\\(--x\\)\\:flex {\n  display: flex;\n}\n"
    );
}

// ---- Third review: file identity, symlinks, pipes, precedence ------------

#[cfg(unix)]
fn symlink(target: &str, link: &Path) {
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn output_should_refuse_to_overwrite_a_hard_linked_input() {
    let dir = project(&[
        ("i.html", r#"<p class="a">"#),
        ("styles/a.css", ".a{color:red}.zz{color:red}"),
    ]);
    std::fs::create_dir(dir.path().join("out")).unwrap();
    std::fs::hard_link(
        dir.path().join("styles/a.css"),
        dir.path().join("out/a.css"),
    )
    .unwrap();
    let output = run_in(
        dir.path(),
        &["prune", "styles/*.css", "-c", "i.html", "-o", "out/"],
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(
        read(&dir.path().join("styles/a.css")),
        ".a{color:red}.zz{color:red}"
    );
}

#[test]
fn output_should_refuse_to_overwrite_a_content_file() {
    let dir = project(&[("i.html", r#"<p class="a">"#), ("c.css", ".a{color:red}")]);
    let output = run_in(
        dir.path(),
        &["prune", "c.css", "-c", "i.html", "-o", "i.html"],
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(read(&dir.path().join("i.html")), r#"<p class="a">"#);
}

#[test]
fn output_should_name_the_content_file_it_refuses_to_overwrite() {
    let dir = project(&[("i.html", r#"<p class="a">"#), ("c.css", ".a{color:red}")]);
    let output = run_in(
        dir.path(),
        &["prune", "c.css", "-c", "i.html", "-o", "i.html"],
        None,
    );
    assert_eq!(
        stderr(&output),
        "error: refusing to overwrite the content file i.html; choose a different --output\n"
    );
}

#[test]
fn output_should_replace_a_hard_linked_destination_instead_of_writing_through_it() {
    let dir = project(&[
        ("i.html", r#"<p class="a">"#),
        ("c.css", ".a{color:red}.zz{color:red}"),
        ("other.txt", "KEEP ME"),
    ]);
    std::fs::hard_link(dir.path().join("other.txt"), dir.path().join("out.css")).unwrap();
    let output = run_in(
        dir.path(),
        &[
            "prune", "c.css", "-c", "i.html", "-o", "out.css", "--minify",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(read(&dir.path().join("other.txt")), "KEEP ME");
    assert_eq!(read(&dir.path().join("out.css")), ".a{color:red}");
}

#[cfg(unix)]
#[test]
fn output_should_refuse_a_symlinked_subdirectory_inside_the_output_directory() {
    let dir = project(&[
        ("i.html", r#"<p class="a b">"#),
        ("styles/a.css", ".a{color:red}"),
        ("styles/sub/b.css", ".b{color:red}"),
        ("elsewhere/keep.txt", ""),
    ]);
    symlink("../elsewhere", &dir.path().join("out/sub"));
    let output = run_in(
        dir.path(),
        &[
            "prune",
            "styles/a.css",
            "styles/sub/b.css",
            "-c",
            "i.html",
            "-o",
            "out/",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(!dir.path().join("elsewhere/b.css").exists());
}

#[cfg(unix)]
#[test]
fn output_directory_should_be_excluded_from_globs_through_a_symlink() {
    let dir = sample_project();
    std::fs::create_dir(dir.path().join("pruned")).unwrap();
    symlink("pruned", &dir.path().join("link"));
    let args = ["prune", "**/*.css", "-c", "src", "-o", "link/"];
    let first = run_in(dir.path(), &args, None);
    let second = run_in(dir.path(), &args, None);
    assert_eq!(
        (first.status.code(), second.status.code()),
        (Some(0), Some(0)),
        "{}",
        stderr(&second)
    );
    assert!(
        !dir.path().join("pruned/pruned").exists(),
        "previous output was pruned again"
    );
}

#[test]
fn output_should_name_the_directory_when_every_stylesheet_is_inside_it() {
    let dir = project(&[
        ("i.html", r#"<p class="a">"#),
        ("out/a.css", ".a{color:red}"),
    ]);
    let output = run_in(
        dir.path(),
        &["prune", "out/*.css", "-c", "i.html", "-o", "out/"],
        None,
    );
    assert_eq!(
        stderr(&output),
        "note: skipping 1 stylesheet(s) inside the output directory\n\
         error: every matched stylesheet is inside the output directory out\n"
    );
}

fn symlinked_content_project() -> tempfile::TempDir {
    project(&[
        ("src/index.html", r#"<p class="a">"#),
        ("shared/Card.html", r#"<p class="c">"#),
        ("app.css", ".a{color:red}.c{color:red}.zz{color:red}"),
    ])
}

#[cfg(unix)]
#[test]
fn prune_should_scan_symlinked_content_files() {
    let dir = symlinked_content_project();
    symlink("../shared/Card.html", &dir.path().join("src/Card.html"));
    let output = run_in(
        dir.path(),
        &["prune", "app.css", "-c", "src/**/*.html", "--minify"],
        None,
    );
    assert_eq!(stdout(&output), ".a{color:red}.c{color:red}");
}

#[cfg(unix)]
#[test]
fn prune_should_scan_symlinked_content_directories() {
    let dir = symlinked_content_project();
    symlink("../shared", &dir.path().join("src/linked"));
    let output = run_in(
        dir.path(),
        &["prune", "app.css", "-c", "src", "--minify"],
        None,
    );
    assert_eq!(stdout(&output), ".a{color:red}.c{color:red}");
}

#[cfg(unix)]
#[test]
fn prune_should_warn_about_content_it_cannot_walk() {
    let dir = symlinked_content_project();
    symlink("../missing.html", &dir.path().join("src/broken.html"));
    let output = run_in(
        dir.path(),
        &["prune", "app.css", "-c", "src/**/*.html", "--minify"],
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("warning: content pattern `src/**/*.html`"),
        "{}",
        stderr(&output)
    );
}

/// Runs prunecss with its standard output closed before it writes.
fn run_with_closed_stdout(dir: &Path, args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prunecss"))
        .current_dir(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    child.wait_with_output().unwrap()
}

/// More output than a pipe buffers, so writing must hit the closed pipe.
fn big_project() -> tempfile::TempDir {
    let classes: Vec<String> = (0..20_000).map(|i| format!("c{i}")).collect();
    let html = format!(r#"<p class="{}">"#, classes.join(" "));
    let css: String = classes
        .iter()
        .flat_map(|class| [".", class, "{color:red}\n"])
        .collect();
    project(&[("src/i.html", &html), ("big.css", &css)])
}

#[test]
fn prune_should_exit_cleanly_when_standard_output_is_closed() {
    let dir = big_project();
    let output = run_with_closed_stdout(dir.path(), &["prune", "big.css", "-c", "src"]);
    assert_eq!(
        (output.status.code(), stderr(&output)),
        (Some(0), String::new())
    );
}

#[test]
fn scan_should_exit_cleanly_when_standard_output_is_closed() {
    let dir = big_project();
    let output = run_with_closed_stdout(dir.path(), &["scan", "src"]);
    assert_eq!(
        (output.status.code(), stderr(&output)),
        (Some(0), String::new())
    );
}

/// Runs `flag` against a config that turns `key` on, and the same CSS with
/// no config at all: the flag must win, giving the default output.
fn assert_flag_overrides_config(key: &str, flag: &str) {
    let dir = project(&[
        ("i.html", r#"<p class="a" style="font-family: x">"#),
        (
            "s.css",
            ":root{--unused:1}@keyframes spin{to{opacity:0}}\
             @font-face{font-family:nobody;src:url(n.woff)}.a{color:red}",
        ),
    ]);
    std::fs::write(
        dir.path().join("prunecss.json"),
        format!(r#"{{ "css": ["s.css"], "content": ["i.html"], "{key}": true }}"#),
    )
    .unwrap();
    let overridden = run_in(dir.path(), &["prune", flag], None);
    let default = run_in(
        dir.path(),
        &["prune", "--no-config", "s.css", "-c", "i.html"],
        None,
    );
    assert_eq!(
        stdout(&overridden),
        stdout(&default),
        "{}",
        stderr(&overridden)
    );
}

#[test]
fn no_minify_should_override_the_config() {
    assert_flag_overrides_config("minify", "--no-minify");
}

#[test]
fn no_keyframes_should_override_the_config() {
    assert_flag_overrides_config("keyframes", "--no-keyframes");
}

#[test]
fn no_font_face_should_override_the_config() {
    assert_flag_overrides_config("fontFace", "--no-font-face");
}

#[test]
fn no_variables_should_override_the_config() {
    assert_flag_overrides_config("variables", "--no-variables");
}

#[test]
fn the_last_of_minify_and_no_minify_should_win() {
    let dir = project(&[("i.html", r#"<p class="a">"#), ("s.css", ".a{color:red}")]);
    let output = run_in(
        dir.path(),
        &["prune", "s.css", "-c", "i.html", "--no-minify", "--minify"],
        None,
    );
    assert_eq!(stdout(&output), ".a{color:red}");
}
