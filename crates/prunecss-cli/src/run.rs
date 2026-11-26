//! What each command does, once arguments and config are resolved.

use std::borrow::Cow;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use prunecss_core::{PruneOptions, PruneOutput, defined_names, extract_classes};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use serde::Serialize;

use crate::files;

/// Patterns and the directory they are relative to.
#[derive(Debug, Clone)]
pub(crate) struct Patterns {
    pub(crate) patterns: Vec<String>,
    pub(crate) base: PathBuf,
}

/// Everything `prune` needs, with CLI flags and config already merged.
#[derive(Debug)]
#[expect(clippy::struct_excessive_bools, reason = "independent on/off settings")]
pub(crate) struct PruneSettings {
    pub(crate) css: Patterns,
    pub(crate) content: Patterns,
    /// `None` or `-` means standard output.
    pub(crate) output: Option<PathBuf>,
    pub(crate) options: PruneOptions,
    pub(crate) gitignore: bool,
    pub(crate) check: bool,
    pub(crate) stats: bool,
    pub(crate) json: bool,
}

/// How a successful run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing to report: pruned, or `--check` found nothing to remove.
    Clean,
    /// `--check` found CSS that pruning would remove.
    WouldRemove,
}

/// One pruned stylesheet, as reported by `--stats` and `--json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileReport {
    path: String,
    output: Option<String>,
    removed_selectors: usize,
    removed_keyframes: usize,
    removed_font_faces: usize,
    removed_variables: usize,
    bytes_before: usize,
    bytes_after: usize,
}

impl FileReport {
    fn removed_anything(&self) -> bool {
        self.removed_selectors
            + self.removed_keyframes
            + self.removed_font_faces
            + self.removed_variables
            > 0
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    files: Vec<FileReport>,
    content_files: usize,
    used_names: usize,
    duration_ms: u128,
}

/// A stylesheet to prune: from a file, or from standard input.
enum Input {
    File(PathBuf),
    Stdin,
}

impl Input {
    fn label(&self) -> String {
        match self {
            Self::File(path) => display(path),
            Self::Stdin => "<stdin>".to_owned(),
        }
    }
}

pub(crate) fn prune(settings: &PruneSettings) -> Result<Outcome> {
    let start = Instant::now();
    let output = settings
        .output
        .as_deref()
        .filter(|path| *path != Path::new("-"))
        .map(absolute);
    let inputs = css_inputs(settings, output.as_deref())?;
    let to_stdout = !settings.check && output.is_none();
    if to_stdout && inputs.len() > 1 {
        bail!(
            "{} stylesheets matched; use --output <DIR> to write them, or --check",
            inputs.len()
        );
    }
    if to_stdout && settings.json {
        bail!("--json needs the CSS to go elsewhere: add --output or --check");
    }
    // Decide every destination before reading or writing anything, so a bad
    // plan (a collision, an input about to be overwritten) changes nothing.
    let destinations = if settings.check {
        vec![None; inputs.len()]
    } else {
        plan_outputs(&inputs, output.as_deref(), settings.output.as_deref())?
    };

    let content_files = content_files(&settings.content, settings.gitignore)?;
    let sources = read_all(&content_files)?;
    let used = used_names(&sources);
    if used.is_empty() {
        bail!(
            "the content files contain no class or id names, so every class would look \
             unused; refusing to prune. Check the --content patterns"
        );
    }

    let results: Vec<(String, PruneOutput)> = inputs
        .par_iter()
        .map(|input| {
            let css = read_input(input)?;
            let pruned = prunecss_core::prune(&css, &used, &settings.options)
                .with_context(|| format!("pruning {}", input.label()))?;
            Ok((css, pruned))
        })
        .collect::<Result<_>>()?;

    let mut reports = Vec::with_capacity(results.len());
    for ((input, destination), (css, pruned)) in inputs.iter().zip(&destinations).zip(&results) {
        if pruned.removed_selectors > 0 && pruned.kept_named_selectors == 0 {
            eprintln!(
                "warning: none of the classes or ids in {} are used by the content files; \
                 check the --content patterns",
                input.label()
            );
        }
        if !settings.check {
            write_output(destination.as_deref(), &pruned.css)?;
        }
        reports.push(FileReport {
            path: input.label(),
            output: destination.as_deref().map(display),
            removed_selectors: pruned.removed_selectors,
            removed_keyframes: pruned.removed_keyframes,
            removed_font_faces: pruned.removed_font_faces,
            removed_variables: pruned.removed_variables,
            bytes_before: css.len(),
            bytes_after: pruned.css.len(),
        });
    }

    let report = Report {
        content_files: content_files.len(),
        used_names: used.len(),
        duration_ms: start.elapsed().as_millis(),
        files: reports,
    };
    if settings.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if settings.stats || settings.check {
        print_summary(&report, settings.check);
    }

    let would_remove = report.files.iter().any(FileReport::removed_anything);
    Ok(if settings.check && would_remove {
        Outcome::WouldRemove
    } else {
        Outcome::Clean
    })
}

pub(crate) fn scan(content: &Patterns, css: Option<&Path>, gitignore: bool) -> Result<()> {
    let files = content_files(content, gitignore)?;
    let sources = read_all(&files)?;
    let mut names: Vec<Cow<'_, str>> = used_names(&sources).into_iter().collect();
    if let Some(css) = css {
        let defined =
            defined_names(&read(css)?).with_context(|| format!("reading {}", display(css)))?;
        names.retain(|name| defined.contains(name.as_ref()));
    }
    // Hash-set order is random; print sorted so output is reproducible.
    names.sort_unstable();
    for name in names {
        println!("{name}");
    }
    Ok(())
}

fn css_inputs(settings: &PruneSettings, output: Option<&Path>) -> Result<Vec<Input>> {
    let patterns = &settings.css.patterns;
    if patterns.is_empty() {
        bail!("no stylesheet given: pass one, or set `css` in prunecss.json");
    }
    if patterns.iter().any(|pattern| pattern == "-") {
        if patterns.len() > 1 {
            bail!("`-` (standard input) can't be combined with other stylesheets");
        }
        return Ok(vec![Input::Stdin]);
    }
    let expanded = files::expand(patterns, &settings.css.base, settings.gitignore)?;
    if let Some(pattern) = expanded.unmatched.first() {
        bail!("no stylesheet matched `{pattern}`");
    }
    let mut files = expanded.files;
    // Leave earlier results alone: with `css: ["**/*.css"]` and an output
    // directory inside the project, the next run would prune its own output.
    if let Some(output) =
        output.filter(|output| is_directory_like(output, settings.output.as_deref()))
    {
        let output = files::normalize(output);
        let before = files.len();
        files.retain(|file| !files::normalize(file).starts_with(&output));
        if files.is_empty() && before > 0 {
            bail!(
                "every matched stylesheet is inside the output directory {}",
                display(&output)
            );
        }
    }
    Ok(files.into_iter().map(Input::File).collect())
}

/// `--output` names a directory if it already is one, or ends with `/`.
fn is_directory_like(output: &Path, as_given: Option<&Path>) -> bool {
    output.is_dir()
        || as_given.is_some_and(|given| given.as_os_str().to_string_lossy().ends_with(['/', '\\']))
}

fn absolute(path: &Path) -> PathBuf {
    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

/// Works out where each result goes, refusing plans that would lose data.
///
/// In directory mode, paths are mirrored from the inputs' deepest common
/// parent (after resolving `..`), so a destination can never leave the
/// output directory.
fn plan_outputs(
    inputs: &[Input],
    output: Option<&Path>,
    as_given: Option<&Path>,
) -> Result<Vec<Option<PathBuf>>> {
    let Some(output) = output else {
        return Ok(vec![None; inputs.len()]);
    };
    let output = files::normalize(output);
    let input_paths: Vec<PathBuf> = inputs
        .iter()
        .filter_map(|input| match input {
            Input::File(path) => Some(files::normalize(path)),
            Input::Stdin => None,
        })
        .collect();

    let directory = inputs.len() > 1 || is_directory_like(&output, as_given);
    let destinations: Vec<PathBuf> = if directory {
        let common = common_parent(&input_paths);
        inputs
            .iter()
            .map(|input| match input {
                Input::File(path) => {
                    let path = files::normalize(path);
                    let relative = common
                        .as_deref()
                        .and_then(|common| path.strip_prefix(common).ok())
                        .map_or_else(
                            || PathBuf::from(path.file_name().unwrap_or_default()),
                            Path::to_path_buf,
                        );
                    output.join(relative)
                }
                Input::Stdin => output.join("stdin.css"),
            })
            .collect()
    } else {
        vec![output]
    };

    let mut seen = std::collections::BTreeSet::new();
    for destination in &destinations {
        if !seen.insert(destination) {
            bail!(
                "two stylesheets would both be written to {}",
                display(destination)
            );
        }
        if input_paths
            .iter()
            .any(|input| same_file(input, destination))
        {
            bail!(
                "refusing to overwrite the input {}; choose a different --output",
                display(destination)
            );
        }
    }
    Ok(destinations.into_iter().map(Some).collect())
}

/// The deepest directory containing every path, if they share one.
fn common_parent(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut common = paths.first()?.parent()?.to_path_buf();
    for path in &paths[1..] {
        while !path.starts_with(&common) {
            if !common.pop() {
                return None;
            }
        }
    }
    Some(common)
}

/// Lexically equal, or the same file on disk (symlinks, case-insensitive
/// file systems).
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}

/// Expands the content patterns, refusing to continue without any files:
/// with nothing scanned, every class looks unused and all CSS would go.
fn content_files(content: &Patterns, gitignore: bool) -> Result<Vec<PathBuf>> {
    if content.patterns.is_empty() {
        bail!("no content given: pass --content, or set `content` in prunecss.json");
    }
    let expanded = files::expand(&content.patterns, &content.base, gitignore)?;
    for pattern in &expanded.unmatched {
        eprintln!("warning: content pattern `{pattern}` matched no files");
    }
    if expanded.files.is_empty() {
        bail!(
            "no content files matched, so every class would look unused; \
             refusing to prune. Check the --content patterns"
        );
    }
    Ok(expanded.files)
}

/// Reads content files in parallel. Invalid UTF-8 is replaced rather than
/// fatal: a stray binary file in a glob shouldn't stop the run.
fn read_all(paths: &[PathBuf]) -> Result<Vec<String>> {
    paths
        .par_iter()
        .map(|path| {
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .with_context(|| format!("reading {}", display(path)))
        })
        .collect()
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", display(path)))
}

fn read_input(input: &Input) -> Result<String> {
    match input {
        Input::File(path) => read(path),
        Input::Stdin => {
            let mut css = String::new();
            std::io::stdin()
                .read_to_string(&mut css)
                .context("reading standard input")?;
            Ok(css)
        }
    }
}

/// Collects candidate names across files in parallel, borrowing from them.
fn used_names(sources: &[String]) -> FxHashSet<Cow<'_, str>> {
    sources
        .par_iter()
        .fold(FxHashSet::default, |mut names, source| {
            names.extend(extract_classes(source));
            names
        })
        .reduce(FxHashSet::default, |mut larger, mut smaller| {
            if larger.len() < smaller.len() {
                std::mem::swap(&mut larger, &mut smaller);
            }
            larger.extend(smaller);
            larger
        })
}

/// Writes one result to its planned destination, or standard output.
fn write_output(destination: Option<&Path>, css: &str) -> Result<()> {
    let Some(destination) = destination else {
        print!("{css}");
        return Ok(());
    };
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", display(parent)))?;
    }
    std::fs::write(destination, css).with_context(|| format!("writing {}", display(destination)))
}

fn print_summary(report: &Report, check: bool) {
    let verb = if check { "would remove" } else { "removed" };
    for file in &report.files {
        let saved = file.bytes_before.saturating_sub(file.bytes_after);
        eprintln!(
            "{}: {verb} {} selectors, {} @keyframes, {} @font-face, {} variables; {} -> {} bytes ({saved} saved)",
            file.path,
            file.removed_selectors,
            file.removed_keyframes,
            file.removed_font_faces,
            file.removed_variables,
            file.bytes_before,
            file.bytes_after,
        );
    }
    eprintln!(
        "prunecss: {} stylesheet(s), {} content file(s), {} candidate names, {} ms",
        report.files.len(),
        report.content_files,
        report.used_names,
        report.duration_ms
    );
}

/// Paths are shown relative to the current directory where possible, with
/// `/` on every platform, so messages are short and snapshots match.
fn display(path: &Path) -> String {
    let cwd = std::env::current_dir().ok();
    let shown = cwd
        .as_deref()
        .and_then(|cwd| path.strip_prefix(cwd).ok())
        .filter(|relative| !relative.as_os_str().is_empty())
        .unwrap_or(path);
    shown.to_string_lossy().replace('\\', "/")
}
