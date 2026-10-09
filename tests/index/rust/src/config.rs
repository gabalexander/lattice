use serde::Deserialize;

/// Where the settings are read from.
pub const CONFIG_FILE: &str = "bell.toml";

/// Set to anything, keeps bell quiet.
pub const QUIET: &str = "BELL_QUIET";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub sessions: SessionSettings,
    #[serde(rename = "profile")]
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionSettings {
    pub stop_idle_after: u64,
}

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub name: String,
}

impl Config {
    pub fn new() -> Config {
        todo!()
    }
}
