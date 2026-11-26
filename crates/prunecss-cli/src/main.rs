//! `prunecss` command-line tool.
//!
//! For now it supports one command: `prunecss scan <file>...`, which prints
//! every class name used in the given files.

use std::collections::BTreeSet;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // `{:#}` prints the whole cause chain on one line, without the
            // backtrace that returning the error from `main` would show.
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("scan") => {
            let paths: Vec<String> = args.collect();
            if paths.is_empty() {
                bail!("usage: prunecss scan <file>...");
            }
            scan(&paths)
        }
        _ => bail!("usage: prunecss scan <file>..."),
    }
}

fn scan(paths: &[String]) -> Result<()> {
    // Keep every file's text alive so the borrowed class names stay valid.
    let sources = paths
        .iter()
        .map(|path| std::fs::read_to_string(path).with_context(|| format!("reading {path}")))
        .collect::<Result<Vec<_>>>()?;

    let classes: BTreeSet<&str> = sources
        .iter()
        .flat_map(|source| prunecss_core::scanner::extract_classes(source))
        .collect();

    for class in classes {
        println!("{class}");
    }
    Ok(())
}
