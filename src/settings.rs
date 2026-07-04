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

/// Which commit-graph row layout the graph renders. Persisted so the choice
/// survives restarts. `Compact` (single-row) is the default and matches the
/// layout shipped before the switcher existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphViewMode {
    #[default]
    Compact,
    Table,
}

/// The complete set of persisted user settings. This is the single value
/// written to and read from disk; add fields here to persist more state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Most-recently-opened repositories, newest first.
    pub recent_repositories: Vec<RecentRepository>,
    /// Persisted geometry of the main window, or `None` when nothing has been
    /// saved yet. Restored on launch; see the `window_placement` module.
    pub window_state: Option<WindowState>,
    /// Persisted widths of the resizable sidebars. Each entry is `None` until a
    /// width has been measured and saved for that sidebar.
    pub sidebar_widths: SidebarWidths,
    /// Whether AI assistance (ADR-0005) is enabled. Off by default; enabling
    /// requires a configured Claude CLI on the machine.
    pub ai_enabled: bool,
    /// Which commit-graph layout to render. Defaults to `Compact`.
    pub graph_view_mode: GraphViewMode,
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

/// How the main window was displayed when it was last closed. The geometry
/// stored alongside this in [`WindowState`] is the windowed "restore" size for
/// the maximized and fullscreen variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    Windowed,
    Maximized,
    Fullscreen,
}

/// Persisted position, size, mode, and monitor of the main window. Coordinates
/// and dimensions are in logical pixels. `display` is the stable per-monitor
/// UUID (string form) the window was on, so restoration can target the same
/// physical screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub display: String,
    pub mode: WindowMode,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Persisted widths (in logical pixels) of the resizable sidebars. A field is
/// `None` when no width has been captured for that sidebar yet, in which case
/// the app falls back to that sidebar's default width.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarWidths {
    /// Left width of the branch sidebar in the graph view.
    pub branch_sidebar: Option<f32>,
    /// Left width of the changed-files list in the changeset view.
    pub changeset_files: Option<f32>,
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
            window_state: None,
            sidebar_widths: SidebarWidths::default(),
            ai_enabled: false,
            graph_view_mode: GraphViewMode::default(),
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

    #[test]
    fn window_state_survives_json_round_trip_for_all_modes() {
        let dir = tempfile::tempdir().expect("create tempdir");
        for mode in [
            WindowMode::Windowed,
            WindowMode::Maximized,
            WindowMode::Fullscreen,
        ] {
            let path = dir.path().join(format!("settings-{mode:?}.json"));
            let settings = Settings {
                recent_repositories: vec![],
                window_state: Some(WindowState {
                    display: "1e2d3c4b-0000-0000-0000-000000000000".to_string(),
                    mode,
                    x: 120.5,
                    y: -40.0,
                    width: 1440.0,
                    height: 900.0,
                }),
                sidebar_widths: SidebarWidths::default(),
                ai_enabled: false,
                graph_view_mode: GraphViewMode::default(),
            };

            save(&path, &settings).expect("save settings");

            assert_eq!(load(&path), settings);
        }
    }

    #[test]
    fn settings_without_window_state_load_it_as_none() {
        // A file written before this field existed must still load cleanly,
        // with window_state defaulting to None.
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"recent_repositories": []}"#).expect("write file");

        assert_eq!(load(&path).window_state, None);
    }

    #[test]
    fn ai_enabled_defaults_off_and_round_trips() {
        // Old settings files (no ai_enabled key) must load with it off.
        let legacy: Settings = serde_json::from_str("{}").expect("legacy parses");
        assert!(!legacy.ai_enabled);

        let enabled = Settings {
            ai_enabled: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&enabled).expect("serialize");
        let reloaded: Settings = serde_json::from_str(&json).expect("reload");
        assert!(reloaded.ai_enabled);
    }

    #[test]
    fn graph_view_mode_defaults_compact_and_round_trips() {
        // Legacy files with no key load as Compact.
        let legacy: Settings = serde_json::from_str("{}").expect("legacy parses");
        assert_eq!(legacy.graph_view_mode, GraphViewMode::Compact);

        let table = Settings {
            graph_view_mode: GraphViewMode::Table,
            ..Settings::default()
        };
        let json = serde_json::to_string(&table).expect("serialize");
        let reloaded: Settings = serde_json::from_str(&json).expect("reload");
        assert_eq!(reloaded.graph_view_mode, GraphViewMode::Table);
    }

    #[test]
    fn sidebar_widths_survive_json_round_trip() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("settings.json");
        let settings = Settings {
            recent_repositories: vec![],
            window_state: None,
            sidebar_widths: SidebarWidths {
                branch_sidebar: Some(275.5),
                changeset_files: Some(360.0),
            },
            ai_enabled: false,
            graph_view_mode: GraphViewMode::default(),
        };

        save(&path, &settings).expect("save settings");

        assert_eq!(load(&path), settings);
    }

    #[test]
    fn settings_without_sidebar_widths_load_as_all_none() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"recent_repositories": []}"#).expect("write file");

        assert_eq!(load(&path).sidebar_widths, SidebarWidths::default());
    }
}
