//! `prunecss` command-line tool.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use prunecss_core::pruner::{self, PruneOptions};
use prunecss_core::scanner::extract_classes;

/// Remove unused CSS, fast.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print every class name used in the given content files.
    Scan {
        /// HTML, JSX or other markup files to scan.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Remove rules from a stylesheet that no content file can match.
    Prune {
        /// The stylesheet to prune.
        css: PathBuf,
        /// Content files whose class names count as used.
        #[arg(long, short, required = true, num_args = 1..)]
        content: Vec<PathBuf>,
        /// Write the result here instead of to standard output.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Print the result without whitespace.
        #[arg(long)]
        minify: bool,
        /// Print a summary of what was removed to standard error.
        #[arg(long)]
        stats: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // `{:#}` prints the whole cause chain on one line, without the
            // backtrace that returning the error from `main` would show.
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Scan { files } => scan(&files),
        Command::Prune {
            css,
            content,
            output,
            minify,
            stats,
        } => prune(&css, &content, output.as_deref(), minify, stats),
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn read_all(paths: &[PathBuf]) -> Result<Vec<String>> {
    paths.iter().map(|path| read(path)).collect()
}

/// Collects the class names used across `sources`, borrowing from them.
fn used_classes(sources: &[String]) -> BTreeSet<&str> {
    sources
        .iter()
        .flat_map(|source| extract_classes(source))
        .collect()
}

fn scan(files: &[PathBuf]) -> Result<()> {
    let sources = read_all(files)?;
    for class in used_classes(&sources) {
        println!("{class}");
    }
    Ok(())
}

fn prune(
    css_path: &Path,
    content: &[PathBuf],
    output: Option<&Path>,
    minify: bool,
    stats: bool,
) -> Result<()> {
    let css = read(css_path)?;
    let sources = read_all(content)?;
    let used = used_classes(&sources);

    let pruned = pruner::prune(&css, &used, PruneOptions { minify })
        .with_context(|| format!("pruning {}", css_path.display()))?;

    match output {
        Some(path) => std::fs::write(path, &pruned.css)
            .with_context(|| format!("writing {}", path.display()))?,
        None => print!("{}", pruned.css),
    }

    if stats {
        eprintln!(
            "prunecss: removed {} selectors; {} -> {} bytes",
            pruned.removed_selectors,
            css.len(),
            pruned.css.len()
        );
    }
    Ok(())
}
