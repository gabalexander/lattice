//! The settings in `~/.config/lattice/config.toml` (see [`crate::paths`]).
//! The file is optional, and so is every setting in it; a setting left out
//! has its default:
//!
//! ```toml
//! model = "sonnet"          # the model that plans and writes a wiki
//! concurrency = 4           # how many of its writers write at once
//! budget_usd = 30.0         # the most one build may spend
//! ask_model = "sonnet"      # the model that answers the chat
//! ask_budget_usd = 0.5      # the most one question may spend
//! exclude = ["vendor/**"]   # files left out of every wiki
//!
//! [index]                   # the symbol index links come from
//! precise = true            # run the SCIP indexers installed here
//! indexer_timeout_secs = 900
//! indexer_memory_mb = 8192
//! max_file_kb = 1024
//! paths_only = ["vendor/", "third_party/", "node_modules/", "testdata/"]
//! ```
//!
//! A key lattice doesn't know is an error, not something to skip: a
//! setting spelled wrong would otherwise do nothing, without a word. And
//! so is a value that makes no sense, said with the setting's name.
//!
//! Adapted from crystal's `src/config.rs` (MIT).

use crate::index::IndexSettings;
use crate::paths;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

/// How many of a build's writers may write at once.
pub const CONCURRENCY: RangeInclusive<usize> = 1..=16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The model that plans and writes a wiki, as `claude --model` takes
    /// it: `sonnet`, `opus`, or a model's full name.
    pub model: String,
    /// How many of a build's writers write at once.
    pub concurrency: usize,
    /// The most one build may spend, in US dollars, by Claude's own count:
    /// one that reaches it stops, keeping what it wrote, for a resume to
    /// carry on from.
    pub budget_usd: f64,
    /// The model that answers the chat.
    pub ask_model: String,
    /// The most one question may spend, in US dollars.
    pub ask_budget_usd: f64,
    /// Files left out of every wiki, as globs the way `.gitignore` writes
    /// them: `vendor/`, `*.pb.go`, `docs/**/*.svg`.
    pub exclude: Vec<String>,
    /// How the symbol index the wiki's links come from is built: `[index]`
    /// in the file. Its `exclude` is never read from there: it's the one
    /// above, which [`Config::index_settings`] gives it.
    pub index: IndexSettings,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            model: "sonnet".to_string(),
            concurrency: 4,
            budget_usd: 30.0,
            ask_model: "sonnet".to_string(),
            ask_budget_usd: 0.5,
            exclude: Vec::new(),
            index: IndexSettings::default(),
        }
    }
}

impl Config {
    /// The settings in the config file, or the defaults when there's no
    /// file. A file that can't be read, or doesn't make sense, is an error
    /// that names it.
    pub fn load() -> Result<Config> {
        let path = paths::config_file();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config::default());
            }
            Err(err) => {
                return Err(err).with_context(|| format!("couldn't read {}", path.display()));
            }
        };
        Config::from_text(&text).with_context(|| format!("in {}", path.display()))
    }

    /// The settings `text` holds, checked.
    pub fn from_text(text: &str) -> Result<Config> {
        let config: Config = toml::from_str(text)?;
        config.check()?;
        Ok(config)
    }

    /// How the symbol index is built: `[index]`, leaving out the files
    /// every wiki leaves out.
    pub fn index_settings(&self) -> IndexSettings {
        IndexSettings {
            exclude: self.exclude.clone(),
            ..self.index.clone()
        }
    }

    /// Refuses settings that make no sense, naming the one that doesn't.
    pub fn check(&self) -> Result<()> {
        check_model(&self.model).context("model")?;
        check_model(&self.ask_model).context("ask_model")?;
        if !CONCURRENCY.contains(&self.concurrency) {
            bail!(
                "concurrency is {}: it's from {} to {}",
                self.concurrency,
                CONCURRENCY.start(),
                CONCURRENCY.end()
            );
        }
        for (name, budget) in [
            ("budget_usd", self.budget_usd),
            ("ask_budget_usd", self.ask_budget_usd),
        ] {
            if !budget.is_finite() || budget <= 0.0 {
                bail!("{name} is {budget}: it's more than 0");
            }
        }
        if self.exclude.iter().any(|glob| glob.trim().is_empty()) {
            bail!("exclude has an empty glob");
        }
        let index = &self.index;
        for (name, value) in [
            ("indexer_timeout_secs", index.indexer_timeout_secs),
            ("indexer_memory_mb", index.indexer_memory_mb),
            ("max_file_kb", index.max_file_kb),
        ] {
            if value == 0 {
                bail!("[index] {name} is 0: it's more than 0");
            }
        }
        if index.paths_only.iter().any(|glob| glob.trim().is_empty()) {
            bail!("[index] paths_only has an empty glob");
        }
        Ok(())
    }
}

/// Refuses what isn't a model's name as `claude --model` takes it: an
/// alias like `sonnet`, or a full name like `claude-sonnet-5-5[1m]`. It's
/// given to `claude` as an argument of its own, so it must never read as
/// an option.
pub fn check_model(model: &str) -> Result<()> {
    let mut chars = model.chars();
    let first = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let rest = chars.all(|c| c.is_ascii_alphanumeric() || "-._[]".contains(c));
    if !first || !rest || model.len() > 100 {
        bail!("{model:?} isn't a model's name, like sonnet, opus or claude-sonnet-5-5");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_has_its_default() {
        assert_eq!(Config::from_text("").unwrap(), Config::default());
        let config = Config::from_text(
            "model = \"opus\"\nconcurrency = 2\nbudget_usd = 12.5\nask_model = \"claude-haiku-5-5\"\n\
             ask_budget_usd = 0.25\nexclude = [\"vendor/**\", \"*.min.js\"]\n",
        )
        .unwrap();
        assert_eq!(
            config,
            Config {
                model: "opus".into(),
                concurrency: 2,
                budget_usd: 12.5,
                ask_model: "claude-haiku-5-5".into(),
                ask_budget_usd: 0.25,
                exclude: vec!["vendor/**".into(), "*.min.js".into()],
                index: IndexSettings::default(),
            }
        );
    }

    #[test]
    fn the_index_is_built_by_a_table_of_its_own_with_the_wiki_s_exclude() {
        let index = Config::default().index_settings();
        assert!(index.precise);
        assert_eq!((index.indexer_timeout_secs, index.max_file_kb), (900, 1024));
        assert_eq!(index.paths_only[0], "vendor/");
        let config = Config::from_text(
            "exclude = [\"gen/\"]\n[index]\nprecise = false\nindexer_memory_mb = 2048\n\
             paths_only = [\"third_party/\"]\n",
        )
        .unwrap();
        let index = config.index_settings();
        assert!(!index.precise);
        assert_eq!((index.indexer_memory_mb, index.max_file_kb), (2048, 1024));
        assert_eq!(index.paths_only, ["third_party/"]);
        assert_eq!(index.exclude, ["gen/"]);
        let refused = |text: &str| format!("{:#}", Config::from_text(text).unwrap_err());
        assert!(refused("[index]\nprecis = false").contains("unknown field `precis`"));
        assert!(refused("[index]\nexclude = [\"x\"]").contains("unknown field `exclude`"));
        assert!(refused("[index]\nmax_file_kb = 0").contains("[index] max_file_kb is 0"));
        assert!(refused("[index]\npaths_only = [\"\"]").contains("empty glob"));
    }

    #[test]
    fn a_setting_spelled_wrong_or_out_of_range_is_named() {
        let refused = |text: &str| format!("{:#}", Config::from_text(text).unwrap_err());
        assert!(refused("modle = \"opus\"").contains("unknown field `modle`"));
        assert!(refused("concurrency = 0").contains("concurrency is 0"));
        assert!(refused("concurrency = 17").contains("from 1 to 16"));
        assert!(refused("budget_usd = 0").contains("budget_usd is 0"));
        assert!(refused("ask_budget_usd = -1").contains("ask_budget_usd is -1"));
        assert!(refused("budget_usd = nan").contains("budget_usd is NaN"));
        assert!(refused("exclude = [\" \"]").contains("empty glob"));
        assert!(refused("model = \"\"").starts_with("model: "));
        assert!(refused("ask_model = \"--help\"").starts_with("ask_model: "));
    }

    #[test]
    fn a_model_s_name_never_reads_as_an_option() {
        for model in ["sonnet", "opus", "claude-sonnet-5-5", "claude-opus-5-5[1m]"] {
            assert!(check_model(model).is_ok(), "{model}");
        }
        for model in ["", "-p", "--dangerously-skip-permissions", "son net", "a;b"] {
            assert!(check_model(model).is_err(), "{model}");
        }
    }
}
