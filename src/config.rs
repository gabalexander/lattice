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
//! ```
//!
//! A key lattice doesn't know is an error, not something to skip: a
//! setting spelled wrong would otherwise do nothing, without a word. And
//! so is a value that makes no sense, said with the setting's name.
//!
//! Adapted from crystal's `src/config.rs` (MIT).

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

    /// Writes the settings into the config file, whole or not at all, as
    /// the settings page saves them. Comments the file had are lost: it's
    /// written as the settings say, every one of them.
    pub fn save(&self) -> Result<()> {
        self.check()?;
        let path = paths::config_file();
        let dir = path.parent().context("the config file has a directory")?;
        std::fs::create_dir_all(dir).with_context(|| format!("couldn't make {}", dir.display()))?;
        let text = format!(
            "# lattice's settings: docs/configuration.md says what each one does.\n{}",
            toml::to_string(self)?
        );
        let partial = path.with_extension(format!("toml.{}", std::process::id()));
        std::fs::write(&partial, text)
            .with_context(|| format!("couldn't write {}", partial.display()))?;
        std::fs::rename(&partial, &path)
            .with_context(|| format!("couldn't write {}", path.display()))
    }

    /// The settings `text` holds, checked.
    pub fn from_text(text: &str) -> Result<Config> {
        let config: Config = toml::from_str(text)?;
        config.check()?;
        Ok(config)
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
            }
        );
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
