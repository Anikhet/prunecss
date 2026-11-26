//! The `prunecss.json` config file.
//!
//! Option names follow PurgeCSS (`content`, `css`, `safelist.standard`, …)
//! so moving a project over is mostly copying values. Unknown keys are
//! rejected: a typo like `safelsit` must not silently delete CSS.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prunecss_core::{Pattern, Safelist};
use schemars::JsonSchema;
use serde::Deserialize;

/// The file name looked for in the current directory and its parents.
pub(crate) const FILE_NAME: &str = "prunecss.json";

/// Settings for a prunecss run. Paths are relative to the config file.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
#[schemars(title = "prunecss configuration")]
#[expect(clippy::struct_excessive_bools, reason = "independent on/off settings")]
pub(crate) struct Config {
    /// Location of this schema, for editor autocompletion.
    #[serde(rename = "$schema")]
    pub(crate) schema: Option<String>,
    /// Stylesheets to prune: files, directories or globs such as
    /// `dist/**/*.css`.
    pub(crate) css: Vec<String>,
    /// Files whose class names count as used: files, directories or globs
    /// such as `src/**/*.{html,jsx,tsx,vue,svelte,astro}`.
    pub(crate) content: Vec<String>,
    /// Where to write the result: a file when pruning one stylesheet, a
    /// directory when pruning several. Omit to print a single stylesheet to
    /// standard output.
    pub(crate) output: Option<String>,
    /// Names to keep whatever the content says. Each entry is an exact name
    /// or a `/regex/`. A plain list means `standard`.
    pub(crate) safelist: SafelistConfig,
    /// Also remove `@keyframes` that nothing uses.
    pub(crate) keyframes: bool,
    /// Also remove `@font-face` rules whose family nothing uses.
    pub(crate) font_face: bool,
    /// Also remove custom properties (`--name`) that nothing reads.
    pub(crate) variables: bool,
    /// Print the result without whitespace.
    pub(crate) minify: bool,
    /// Skip files listed in `.gitignore` and `.ignore` when expanding globs.
    /// Files named directly are always used. Defaults to `true`.
    pub(crate) gitignore: Option<bool>,
}

/// The `safelist` setting: a plain list, or patterns by kind.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum SafelistConfig {
    /// Shorthand for `{ "standard": [...] }`.
    Standard(Vec<String>),
    /// Patterns by kind, as in PurgeCSS.
    ByKind(SafelistByKind),
}

impl Default for SafelistConfig {
    fn default() -> Self {
        Self::Standard(Vec::new())
    }
}

/// Safelist patterns by kind.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SafelistByKind {
    /// A matching class or id counts as used.
    pub(crate) standard: Vec<String>,
    /// Keep matching selectors and every rule nested inside them.
    pub(crate) deep: Vec<String>,
    /// Keep any selector containing a matching class or id.
    pub(crate) greedy: Vec<String>,
    /// Never remove matching `@keyframes`.
    pub(crate) keyframes: Vec<String>,
    /// Never remove matching custom properties (written without `--`).
    pub(crate) variables: Vec<String>,
}

impl SafelistConfig {
    /// Parses every pattern, naming the bad entry if one is invalid.
    pub(crate) fn to_safelist(&self) -> Result<Safelist> {
        let mut safelist = Safelist::default();
        match self {
            Self::Standard(standard) => safelist.standard = parse_all("safelist", standard)?,
            Self::ByKind(kinds) => {
                safelist.standard = parse_all("safelist.standard", &kinds.standard)?;
                safelist.deep = parse_all("safelist.deep", &kinds.deep)?;
                safelist.greedy = parse_all("safelist.greedy", &kinds.greedy)?;
                safelist.keyframes = parse_all("safelist.keyframes", &kinds.keyframes)?;
                safelist.variables = parse_all("safelist.variables", &kinds.variables)?;
            }
        }
        Ok(safelist)
    }
}

fn parse_all(key: &str, patterns: &[String]) -> Result<Vec<Pattern>> {
    patterns
        .iter()
        .enumerate()
        .map(|(index, text)| Pattern::parse(text).with_context(|| format!("{key}[{index}]")))
        .collect()
}

/// Reads and parses a config file.
pub(crate) fn load(path: &Path) -> Result<Config> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    // serde_json errors include the line and column.
    serde_json::from_str(&text).with_context(|| format!("invalid config {}", path.display()))
}

/// Finds the nearest `prunecss.json` in `start` or one of its parents.
pub(crate) fn discover(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(FILE_NAME))
        .find(|candidate| candidate.is_file())
}

/// The JSON Schema for [`Config`], pretty-printed.
pub(crate) fn schema() -> String {
    let schema = schemars::schema_for!(Config);
    // Serialising a schema value cannot fail; fall back to `{}` regardless.
    let mut text = serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_owned());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Config> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, json).unwrap();
        load(&path)
    }

    #[test]
    fn load_should_accept_an_empty_object() {
        let config = parse("{}").unwrap();
        assert!(config.css.is_empty() && config.content.is_empty());
    }

    #[test]
    fn load_should_read_every_option() {
        let config = parse(
            r#"{
                "$schema": "./prunecss.schema.json",
                "css": ["dist/*.css"],
                "content": ["src/**/*.tsx"],
                "output": "dist/pruned",
                "safelist": { "standard": ["a"], "deep": ["b"], "greedy": ["/^c/"],
                              "keyframes": ["spin"], "variables": ["brand"] },
                "keyframes": true, "fontFace": true, "variables": true,
                "minify": true, "gitignore": false
            }"#,
        )
        .unwrap();
        let flags = (
            config.keyframes,
            config.font_face,
            config.variables,
            config.minify,
            config.gitignore,
        );
        assert_eq!(flags, (true, true, true, true, Some(false)));
    }

    #[test]
    fn load_should_reject_unknown_keys() {
        let err = parse(r#"{ "safelsit": ["a"] }"#).unwrap_err();
        assert!(
            format!("{err:#}").contains("unknown field `safelsit`"),
            "{err:#}"
        );
    }

    #[test]
    fn load_should_name_the_file_and_position_of_syntax_errors() {
        let err = parse("{\n  \"css\": [,]\n}").unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains(FILE_NAME) && message.contains("line 2"),
            "{message}"
        );
    }

    #[test]
    fn safelist_list_should_mean_standard() {
        let config = parse(r#"{ "safelist": ["a", "/^b-/"] }"#).unwrap();
        let safelist = config.safelist.to_safelist().unwrap();
        assert_eq!((safelist.standard.len(), safelist.greedy.len()), (2, 0));
    }

    #[test]
    fn safelist_by_kind_should_fill_each_kind() {
        let config = parse(r#"{ "safelist": { "deep": ["a"], "greedy": ["b", "c"] } }"#).unwrap();
        let safelist = config.safelist.to_safelist().unwrap();
        assert_eq!((safelist.deep.len(), safelist.greedy.len()), (1, 2));
    }

    #[test]
    fn safelist_should_name_the_invalid_entry() {
        let config = parse(r#"{ "safelist": { "greedy": ["ok", "/(/"] } }"#).unwrap();
        let err = config.safelist.to_safelist().unwrap_err();
        assert!(format!("{err:#}").contains("safelist.greedy[1]"), "{err:#}");
    }

    #[test]
    fn discover_should_find_the_nearest_config_in_a_parent() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("packages/app/src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.path().join("packages/app").join(FILE_NAME), "{}").unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "{}").unwrap();
        assert_eq!(
            discover(&nested),
            Some(dir.path().join("packages/app").join(FILE_NAME))
        );
    }

    #[test]
    fn discover_should_return_none_without_a_config() {
        let dir = tempfile::tempdir().unwrap();
        // The temp dir's parents (e.g. /tmp) must not contain a config either.
        assert!(discover(dir.path()).is_none_or(|found| !found.starts_with(dir.path())));
    }

    // The committed schema must match the code. Regenerate with:
    // UPDATE_SCHEMA=1 cargo test -p prunecss schema_file
    #[test]
    fn schema_file_should_match_the_config_type() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../prunecss.schema.json");
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, schema()).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            committed,
            schema(),
            "run with UPDATE_SCHEMA=1 to regenerate"
        );
    }
}
