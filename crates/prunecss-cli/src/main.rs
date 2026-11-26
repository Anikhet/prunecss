//! `prunecss` command-line tool.

// The workspace forbids printing so the library never does; printing is this
// binary's whole job.
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the CLI prints its results"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use prunecss_core::{Pattern, PruneOptions, Safelist, extract_classes};

/// Remove unused CSS, fast.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print every candidate class or id name found in the content files.
    ///
    /// The scanner over-collects on purpose: a candidate that is not a real
    /// class keeps no CSS, while a missed one would delete CSS.
    Scan {
        /// HTML, JSX or other markup files to scan.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Remove rules from a stylesheet that no content file can match.
    Prune(PruneArgs),
}

#[derive(Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent command-line flag"
)]
struct PruneArgs {
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

    /// Treat a class or id as used. `/regex/` or an exact name; repeatable.
    #[arg(long, value_name = "PATTERN", value_parser = Pattern::parse, help_heading = "Safelist")]
    safelist: Vec<Pattern>,
    /// Keep any selector containing a matching class or id.
    #[arg(long, value_name = "PATTERN", value_parser = Pattern::parse, help_heading = "Safelist")]
    safelist_greedy: Vec<Pattern>,
    /// Keep matching selectors and every rule nested inside them.
    #[arg(long, value_name = "PATTERN", value_parser = Pattern::parse, help_heading = "Safelist")]
    safelist_deep: Vec<Pattern>,

    /// Also remove @keyframes that nothing uses.
    #[arg(long, help_heading = "Unused at-rules (off by default)")]
    keyframes: bool,
    /// Also remove @font-face rules whose family nothing uses.
    #[arg(long, help_heading = "Unused at-rules (off by default)")]
    font_face: bool,
    /// Also remove custom properties (--name) that nothing reads.
    #[arg(long, help_heading = "Unused at-rules (off by default)")]
    variables: bool,
}

impl PruneArgs {
    fn options(&self) -> PruneOptions {
        let mut safelist = Safelist::default();
        safelist.standard.clone_from(&self.safelist);
        safelist.greedy.clone_from(&self.safelist_greedy);
        safelist.deep.clone_from(&self.safelist_deep);
        PruneOptions::default()
            .minify(self.minify)
            .safelist(safelist)
            .keyframes(self.keyframes)
            .font_face(self.font_face)
            .variables(self.variables)
    }
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
        Command::Prune(args) => prune(&args),
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

fn prune(args: &PruneArgs) -> Result<()> {
    let css = read(&args.css)?;
    let sources = read_all(&args.content)?;
    let used = used_classes(&sources);

    let pruned = prunecss_core::prune(&css, &used, &args.options())
        .with_context(|| format!("pruning {}", args.css.display()))?;

    match &args.output {
        Some(path) => std::fs::write(path, &pruned.css)
            .with_context(|| format!("writing {}", path.display()))?,
        None => print!("{}", pruned.css),
    }

    if args.stats {
        eprintln!(
            "prunecss: removed {} selectors, {} @keyframes, {} @font-face, {} variables; {} -> {} bytes",
            pruned.removed_selectors,
            pruned.removed_keyframes,
            pruned.removed_font_faces,
            pruned.removed_variables,
            css.len(),
            pruned.css.len()
        );
    }
    Ok(())
}
