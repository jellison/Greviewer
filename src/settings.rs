//! Persisted user settings.
//!
//! All durable user state lives in a single JSON file (`settings.json`) under
//! the platform configuration directory. [`Settings`] is the one serialized
//! unit; new fields can be added freely because the struct loads missing fields
//! as defaults, so an older file stays readable after the schema grows.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Upper bound on how many recent repositories are retained on disk and in
/// memory. Older entries past this count are dropped.
pub const MAX_RECENT_REPOSITORIES: usize = 10;

/// The complete set of persisted user settings. This is the single value
/// written to and read from disk; add fields here to persist more state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Most-recently-opened repositories, newest first.
    pub recent_repositories: Vec<RecentRepository>,
}

/// A repository the user has opened before, plus whether its folder could still
/// be opened the last time we checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentRepository {
    pub path: PathBuf,
    pub available: bool,
}

impl RecentRepository {
    pub fn available(path: PathBuf) -> Self {
        Self {
            path,
            available: true,
        }
    }

    pub fn unavailable(path: PathBuf) -> Self {
        Self {
            path,
            available: false,
        }
    }
}

/// Read settings from `path`. Returns [`Settings::default`] when the file is
/// missing or cannot be parsed, so a corrupt or absent file never blocks
/// startup.
pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// Write `settings` to `path` as pretty-printed JSON, creating the parent
/// directory if needed.
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let content =
        serde_json::to_string_pretty(settings).map_err(|err| io::Error::other(err.to_string()))?;
    fs::write(path, content)
}

/// The default on-disk location for `settings.json`. `None` only when the
/// platform exposes no home/config directory.
#[cfg(test)]
pub fn default_store_path() -> Option<PathBuf> {
    None
}

#[cfg(all(not(test), target_os = "macos"))]
pub fn default_store_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Greviewer")
            .join("settings.json")
    })
}

#[cfg(all(not(test), not(target_os = "macos")))]
pub fn default_store_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config_home| config_home.join("greviewer").join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips_settings() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("state").join("settings.json");
        let settings = Settings {
            recent_repositories: vec![
                RecentRepository::available(dir.path().join("repo-one")),
                // Characters that needed hand-rolled escaping in the old TSV
                // format must survive a JSON round trip untouched.
                RecentRepository::unavailable(dir.path().join("repo\t\n\r-two")),
            ],
        };

        save(&path, &settings).expect("save settings");

        assert_eq!(load(&path), settings);
    }

    #[test]
    fn load_returns_default_when_file_is_missing() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("does-not-exist.json");

        assert_eq!(load(&path), Settings::default());
    }

    #[test]
    fn load_returns_default_when_file_is_malformed() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("settings.json");
        fs::write(&path, "not json at all").expect("write malformed file");

        assert_eq!(load(&path), Settings::default());
    }

    #[test]
    fn unknown_and_missing_fields_load_as_defaults() {
        // Forward/backward compatibility: a file written by a different schema
        // version (extra field, no recent list) must still load cleanly.
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"future_setting": 42}"#).expect("write file");

        assert_eq!(load(&path), Settings::default());
    }
}
