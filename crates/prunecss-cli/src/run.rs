//! What each command does, once arguments and config are resolved.

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
    let inputs = css_inputs(settings)?;
    let output = settings
        .output
        .as_deref()
        .filter(|path| *path != Path::new("-"));
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

    let content_files = content_files(&settings.content, settings.gitignore)?;
    let sources = read_all(&content_files)?;
    let used = used_names(&sources);

    let results: Vec<(Input, String, PruneOutput)> = inputs
        .into_par_iter()
        .map(|input| {
            let css = read_input(&input)?;
            let pruned = prunecss_core::prune(&css, &used, &settings.options)
                .with_context(|| format!("pruning {}", input.label()))?;
            Ok((input, css, pruned))
        })
        .collect::<Result<_>>()?;

    let single = results.len() == 1;
    let mut reports = Vec::with_capacity(results.len());
    for (input, css, pruned) in &results {
        let destination = if settings.check {
            None
        } else {
            write_output(input, &pruned.css, output, single, &settings.css.base)?
        };
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
    let mut names: Vec<&str> = used_names(&sources).into_iter().collect();
    if let Some(css) = css {
        let defined =
            defined_names(&read(css)?).with_context(|| format!("reading {}", display(css)))?;
        names.retain(|name| defined.contains(*name));
    }
    // Hash-set order is random; print sorted so output is reproducible.
    names.sort_unstable();
    for name in names {
        println!("{name}");
    }
    Ok(())
}

fn css_inputs(settings: &PruneSettings) -> Result<Vec<Input>> {
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
    Ok(expanded.files.into_iter().map(Input::File).collect())
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
fn used_names(sources: &[String]) -> FxHashSet<&str> {
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

/// Writes one result and returns where it went (`None` for stdout).
fn write_output(
    input: &Input,
    css: &str,
    output: Option<&Path>,
    single: bool,
    css_base: &Path,
) -> Result<Option<PathBuf>> {
    let Some(output) = output else {
        print!("{css}");
        return Ok(None);
    };
    let is_dir =
        !single || output.is_dir() || output.as_os_str().to_string_lossy().ends_with(['/', '\\']);
    let destination = if is_dir {
        let relative = match input {
            Input::File(path) => path
                .strip_prefix(css_base)
                .ok()
                .map(Path::to_path_buf)
                .or_else(|| path.file_name().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("stdin.css")),
            Input::Stdin => PathBuf::from("stdin.css"),
        };
        output.join(relative)
    } else {
        output.to_path_buf()
    };
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", display(parent)))?;
    }
    std::fs::write(&destination, css)
        .with_context(|| format!("writing {}", display(&destination)))?;
    Ok(Some(destination))
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

/// Paths are shown with `/` on every platform, so output and snapshots match.
fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
