//! Turn the paths and globs a user gives into a sorted list of files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The files a list of patterns matched, and the patterns that matched none.
#[derive(Debug, Default)]
pub(crate) struct Expanded {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) unmatched: Vec<String>,
}

/// Expands `patterns` relative to `base`.
///
/// - An existing file is always used, even if `.gitignore` lists it: naming
///   a file is a clear request (build output in `dist/` is often ignored).
/// - A directory means every file under it.
/// - Anything else is a glob such as `src/**/*.{html,tsx}`. Walking respects
///   `.gitignore` and `.ignore` unless `respect_ignore_files` is false, and
///   works even outside a git repository.
///
/// The result is sorted and free of duplicates, so runs are reproducible.
pub(crate) fn expand(
    patterns: &[String],
    base: &Path,
    respect_ignore_files: bool,
) -> Result<Expanded> {
    let mut expanded = Expanded::default();
    for original in patterns {
        let pattern = original.strip_prefix("./").unwrap_or(original);
        let path = base.join(pattern);
        let before = expanded.files.len();
        if path.is_file() {
            expanded.files.push(path);
        } else if path.is_dir() {
            walk(&path, None, base, respect_ignore_files, &mut expanded.files);
        } else if has_glob_syntax(pattern) {
            let glob = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("invalid glob `{pattern}`"))?
                .compile_matcher();
            let root = base.join(literal_prefix(pattern));
            // An absolute pattern is matched against absolute paths.
            let match_base = if Path::new(pattern).is_absolute() {
                Path::new("")
            } else {
                base
            };
            walk(
                &root,
                Some(&glob),
                match_base,
                respect_ignore_files,
                &mut expanded.files,
            );
        }
        if expanded.files.len() == before {
            expanded.unmatched.push(original.clone());
        }
    }
    expanded.files.sort();
    expanded.files.dedup();
    Ok(expanded)
}

fn has_glob_syntax(pattern: &str) -> bool {
    pattern.contains(['*', '?', '[', '{'])
}

/// The directories before the first glob character: `src/**/*.tsx` is
/// walked from `src`, not from the project root.
fn literal_prefix(pattern: &str) -> PathBuf {
    // Joining the segments with `/` keeps a leading root: `/srv/x/*.html`
    // is walked from `/srv/x`, not from `srv/x` under the base.
    let literal: Vec<&str> = pattern
        .split('/')
        .take_while(|segment| !has_glob_syntax(segment))
        .collect();
    PathBuf::from(literal.join("/"))
}

/// Resolves `.` and `..` without touching the filesystem, so
/// `site/../styles/a.css` and `styles/a.css` compare equal.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// Adds every file under `root` (matching `glob`, relative to `base`).
fn walk(
    root: &Path,
    glob: Option<&globset::GlobMatcher>,
    base: &Path,
    respect_ignore_files: bool,
    files: &mut Vec<PathBuf>,
) {
    let walker = ignore::WalkBuilder::new(root)
        // Like Tailwind's scanner: hidden files can hold templates, and a
        // user's global gitignore shouldn't change what a build produces.
        .hidden(false)
        .git_global(false)
        .require_git(false)
        .git_ignore(respect_ignore_files)
        .git_exclude(respect_ignore_files)
        .ignore(respect_ignore_files)
        .parents(respect_ignore_files)
        .build();
    // Unreadable entries are skipped; a pattern that then matches nothing is
    // reported as unmatched by the caller.
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        let relative = path.strip_prefix(base).unwrap_or(&path);
        if glob.is_none_or(|glob| glob.is_match(relative)) {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "src/index.html",
            "src/components/Button.tsx",
            "src/components/Card.vue",
            "src/styles.css",
            "dist/app.css",
            "notes.md",
        ] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
        }
        std::fs::write(dir.path().join(".gitignore"), "dist/\n").unwrap();
        dir
    }

    fn relative(expanded: &Expanded, base: &Path) -> Vec<String> {
        expanded
            .files
            .iter()
            .map(|file| {
                file.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn expand_in(dir: &tempfile::TempDir, patterns: &[&str], respect: bool) -> Expanded {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        expand(&patterns, dir.path(), respect).unwrap()
    }

    #[test]
    fn expand_should_match_recursive_globs_with_braces() {
        let dir = project();
        let expanded = expand_in(&dir, &["src/**/*.{html,tsx,vue}"], true);
        assert_eq!(
            relative(&expanded, dir.path()),
            [
                "src/components/Button.tsx",
                "src/components/Card.vue",
                "src/index.html"
            ]
        );
    }

    #[test]
    fn expand_should_not_let_a_single_star_cross_directories() {
        let dir = project();
        let expanded = expand_in(&dir, &["src/*.html"], true);
        assert_eq!(relative(&expanded, dir.path()), ["src/index.html"]);
    }

    #[test]
    fn expand_should_respect_gitignore_in_globs() {
        let dir = project();
        let expanded = expand_in(&dir, &["**/*.css"], true);
        assert_eq!(relative(&expanded, dir.path()), ["src/styles.css"]);
    }

    #[test]
    fn expand_should_include_ignored_files_when_asked() {
        let dir = project();
        let expanded = expand_in(&dir, &["**/*.css"], false);
        assert_eq!(
            relative(&expanded, dir.path()),
            ["dist/app.css", "src/styles.css"]
        );
    }

    #[test]
    fn expand_should_always_use_files_named_explicitly() {
        let dir = project();
        let expanded = expand_in(&dir, &["dist/app.css"], true);
        assert_eq!(relative(&expanded, dir.path()), ["dist/app.css"]);
    }

    #[test]
    fn expand_should_treat_a_directory_as_everything_inside_it() {
        let dir = project();
        let expanded = expand_in(&dir, &["src/components"], true);
        assert_eq!(
            relative(&expanded, dir.path()),
            ["src/components/Button.tsx", "src/components/Card.vue"]
        );
    }

    #[test]
    fn expand_should_remove_duplicates() {
        let dir = project();
        let expanded = expand_in(&dir, &["src/index.html", "src/*.html"], true);
        assert_eq!(relative(&expanded, dir.path()), ["src/index.html"]);
    }

    #[test]
    fn expand_should_report_patterns_that_match_nothing() {
        let dir = project();
        let expanded = expand_in(&dir, &["src/**/*.jsx", "missing.html", "src/*.html"], true);
        assert_eq!(expanded.unmatched, ["src/**/*.jsx", "missing.html"]);
    }

    #[test]
    fn expand_should_accept_a_leading_dot_slash() {
        let dir = project();
        let expanded = expand_in(&dir, &["./src/*.html"], true);
        assert_eq!(relative(&expanded, dir.path()), ["src/index.html"]);
    }

    #[test]
    fn expand_should_reject_invalid_globs() {
        let dir = project();
        let err = expand(&["src/**/*.{html".to_owned()], dir.path(), true).unwrap_err();
        assert!(format!("{err:#}").contains("invalid glob"), "{err:#}");
    }

    #[test]
    fn expand_should_accept_absolute_globs() {
        let dir = project();
        let pattern = format!(
            "{}/src/*.html",
            dir.path().to_string_lossy().replace('\\', "/")
        );
        let expanded = expand(&[pattern], Path::new("/somewhere/else"), true).unwrap();
        assert_eq!(expanded.files, [dir.path().join("src/index.html")]);
    }

    #[test]
    fn normalize_should_resolve_dots_lexically() {
        assert_eq!(
            normalize(Path::new("/a/site/../styles/./x.css")),
            Path::new("/a/styles/x.css")
        );
    }

    #[test]
    fn expand_should_accept_absolute_paths() {
        let dir = project();
        let absolute = dir.path().join("src/index.html");
        let expanded = expand(
            &[absolute.to_string_lossy().into_owned()],
            Path::new("/"),
            true,
        )
        .unwrap();
        assert_eq!(expanded.files, [absolute]);
    }
}
