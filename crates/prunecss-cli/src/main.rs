//! `prunecss` command-line tool.

// The workspace forbids printing so the library never does; printing is this
// binary's whole job. Standard output goes through `run::print_stdout`, which
// handles a closed pipe; messages go to standard error.
#![expect(clippy::print_stderr, reason = "the CLI prints its messages")]

mod config;
mod files;
mod run;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand};
use prunecss_core::{Pattern, PruneOptions};

use crate::config::Config;
use crate::run::{Outcome, Patterns, PruneSettings};

/// Exit code when `--check` finds CSS that pruning would remove.
const EXIT_WOULD_REMOVE: u8 = 1;
/// Exit code for usage, config, input/output and internal errors. clap uses
/// the same code for invalid arguments.
const EXIT_ERROR: u8 = 2;

/// Remove unused CSS, fast.
///
/// Settings come from command-line flags, then the nearest prunecss.json
/// (in the current directory or a parent), then defaults. Each on/off
/// setting has a --no-* flag to turn off what the file turns on.
#[derive(Parser)]
#[command(version, about, long_about)]
#[command(after_help = "Examples:
  prunecss prune dist/app.css --content 'src/**/*.{html,tsx}' -o dist/app.min.css
  prunecss prune --check            # CI: exit 1 if anything is unused
  prunecss scan 'src/**/*.vue' --css dist/app.css

Exit codes: 0 success, 1 --check found unused CSS, 2 error.")]
struct Cli {
    /// Use this config file instead of searching for prunecss.json.
    #[arg(long, global = true, value_name = "FILE", conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// Ignore any prunecss.json.
    #[arg(long, global = true)]
    no_config: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Remove rules from stylesheets that no content file can match.
    Prune(PruneArgs),
    /// Print the candidate class and id names found in content files.
    ///
    /// The scanner over-collects on purpose: a candidate that is not a real
    /// class keeps no CSS, while a missed one would delete CSS. Pass --css
    /// to show only names a stylesheet defines.
    Scan(ScanArgs),
    /// Print the JSON Schema for prunecss.json.
    Schema,
    /// Print a shell completion script, e.g. `prunecss completions bash`.
    Completions {
        /// The shell to generate completions for.
        shell: clap_complete::Shell,
    },
}

#[derive(Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent command-line flag"
)]
struct PruneArgs {
    /// Stylesheets: files, directories or globs. `-` reads standard input.
    css: Vec<String>,
    /// Content files whose class names count as used: files, directories or
    /// globs such as 'src/**/*.{html,tsx}'.
    #[arg(long, short, num_args = 1.., value_name = "PATTERN")]
    content: Vec<String>,
    /// Write here instead of standard output: a file for one stylesheet, a
    /// directory for several.
    #[arg(long, short, value_name = "PATH")]
    output: Option<PathBuf>,
    /// Print the result without whitespace.
    #[arg(long, overrides_with = "no_minify")]
    minify: bool,
    /// Keep whitespace, even if prunecss.json sets `minify`.
    #[arg(long, overrides_with = "minify")]
    no_minify: bool,
    /// Print a summary of what was removed to standard error.
    #[arg(long)]
    stats: bool,
    /// Write nothing; exit with 1 if any CSS would be removed.
    #[arg(long)]
    check: bool,
    /// Print the summary as JSON to standard output.
    #[arg(long)]
    json: bool,
    /// Include files listed in .gitignore when expanding globs.
    #[arg(long)]
    no_gitignore: bool,

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
    #[arg(long, overrides_with = "no_keyframes", help_heading = AT_RULES)]
    keyframes: bool,
    /// Keep unused @keyframes, even if prunecss.json sets `keyframes`.
    #[arg(long, overrides_with = "keyframes", help_heading = AT_RULES)]
    no_keyframes: bool,
    /// Also remove @font-face rules whose family nothing uses.
    #[arg(long, overrides_with = "no_font_face", help_heading = AT_RULES)]
    font_face: bool,
    /// Keep unused @font-face rules, even if prunecss.json sets `fontFace`.
    #[arg(long, overrides_with = "font_face", help_heading = AT_RULES)]
    no_font_face: bool,
    /// Also remove custom properties (--name) that nothing reads.
    #[arg(long, overrides_with = "no_variables", help_heading = AT_RULES)]
    variables: bool,
    /// Keep unused custom properties, even if prunecss.json sets `variables`.
    #[arg(long, overrides_with = "variables", help_heading = AT_RULES)]
    no_variables: bool,
}

const AT_RULES: &str = "Unused at-rules (off by default)";

/// A setting with an on and an off flag: the flag given wins (clap keeps
/// only the last of the pair), otherwise the config's value.
fn flag(on: bool, off: bool, from_config: bool) -> bool {
    on || (!off && from_config)
}

#[derive(Args)]
struct ScanArgs {
    /// Content files, directories or globs. Defaults to `content` in
    /// prunecss.json.
    content: Vec<String>,
    /// Only print names that this stylesheet's selectors mention.
    #[arg(long, value_name = "FILE")]
    css: Option<PathBuf>,
    /// Include files listed in .gitignore when expanding globs.
    #[arg(long)]
    no_gitignore: bool,
}

/// Parsing recurses. The core caps nesting depth, and every thread that
/// parses gets a stack far larger than that cap needs.
const STACK_SIZE: usize = 64 << 20;

fn main() -> ExitCode {
    // `build_global` fails only if a global pool already exists, and nothing
    // creates one before this first line of `main`. Were it ever to fail,
    // rayon would keep the existing pool: the run still works, just without
    // the larger stacks, so there is nothing better to do with the error.
    let _ = rayon::ThreadPoolBuilder::new()
        .stack_size(STACK_SIZE)
        .build_global();
    let cli = Cli::parse();
    // rayon runs a single task on the calling thread, so run the whole
    // command on a thread with the same large stack.
    let outcome = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || run(cli))
        .map_err(anyhow::Error::from)
        .and_then(|handle| {
            handle.join().map_err(|payload| {
                anyhow::anyhow!("internal error: {}", panic_message(payload.as_ref()))
            })
        })
        .and_then(|result| result);
    match outcome {
        Ok(Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::WouldRemove) => ExitCode::from(EXIT_WOULD_REMOVE),
        Err(err) => {
            // `{:#}` prints the whole cause chain on one line, without the
            // backtrace that returning the error from `main` would show.
            eprintln!("error: {err:#}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

/// The text of a panic, for the error message.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("the worker thread panicked")
}

fn run(cli: Cli) -> Result<Outcome> {
    match cli.command {
        Command::Prune(args) => {
            let loaded = load_config(cli.config.as_deref(), cli.no_config)?;
            run::prune(&prune_settings(args, &loaded)?)
        }
        Command::Scan(args) => {
            let loaded = load_config(cli.config.as_deref(), cli.no_config)?;
            let content = choose(args.content, &loaded.config.content, &loaded);
            let gitignore = !args.no_gitignore && loaded.config.gitignore.unwrap_or(true);
            run::scan(&content, args.css.as_deref(), gitignore)?;
            Ok(Outcome::Clean)
        }
        Command::Schema => {
            run::print_stdout(&config::schema())?;
            Ok(Outcome::Clean)
        }
        Command::Completions { shell } => {
            // Generated into memory: `generate` panics if writing fails, as
            // it would on a closed pipe.
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "prunecss", &mut script);
            run::print_stdout(&String::from_utf8_lossy(&script))?;
            Ok(Outcome::Clean)
        }
    }
}

/// A config file and the directory its paths are relative to.
struct Loaded {
    config: Config,
    dir: PathBuf,
    cwd: PathBuf,
}

fn load_config(explicit: Option<&Path>, disabled: bool) -> Result<Loaded> {
    let cwd = std::env::current_dir().context("finding the current directory")?;
    let path = match explicit {
        Some(path) => Some(path.to_path_buf()),
        None if disabled => None,
        None => config::discover(&cwd),
    };
    let Some(path) = path else {
        return Ok(Loaded {
            config: Config::default(),
            dir: cwd.clone(),
            cwd,
        });
    };
    let config = config::load(&path)?;
    let dir = cwd
        .join(&path)
        .parent()
        .map_or_else(|| cwd.clone(), Path::to_path_buf);
    Ok(Loaded { config, dir, cwd })
}

/// Command-line patterns win and are relative to the current directory;
/// otherwise the config's, relative to the config file.
fn choose(cli: Vec<String>, from_config: &[String], loaded: &Loaded) -> Patterns {
    if cli.is_empty() {
        Patterns {
            patterns: from_config.to_vec(),
            base: loaded.dir.clone(),
        }
    } else {
        Patterns {
            patterns: cli,
            base: loaded.cwd.clone(),
        }
    }
}

fn prune_settings(args: PruneArgs, loaded: &Loaded) -> Result<PruneSettings> {
    let config = &loaded.config;
    let mut safelist = config
        .safelist
        .to_safelist()
        .context("invalid `safelist` in prunecss.json")?;
    safelist.standard.extend(args.safelist);
    safelist.greedy.extend(args.safelist_greedy);
    safelist.deep.extend(args.safelist_deep);

    let options = PruneOptions::default()
        .minify(flag(args.minify, args.no_minify, config.minify))
        .safelist(safelist)
        .keyframes(flag(args.keyframes, args.no_keyframes, config.keyframes))
        .font_face(flag(args.font_face, args.no_font_face, config.font_face))
        .variables(flag(args.variables, args.no_variables, config.variables));

    // `-` means standard output, wherever it comes from.
    let output = args.output.or_else(|| {
        config.output.as_ref().map(|output| {
            if output == "-" {
                PathBuf::from("-")
            } else {
                loaded.dir.join(output)
            }
        })
    });

    Ok(PruneSettings {
        css: choose(args.css, &config.css, loaded),
        content: choose(args.content, &config.content, loaded),
        output,
        options,
        gitignore: !args.no_gitignore && config.gitignore.unwrap_or(true),
        check: args.check,
        stats: args.stats,
        json: args.json,
    })
}
