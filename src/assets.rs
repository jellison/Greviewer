//! Embedded application assets served to gpui (icons, etc.).

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use include_dir::{include_dir, Dir};

/// All files under `assets/`, embedded into the binary at compile time.
static ASSETS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

/// Serves embedded application assets (e.g. `icons/*.svg`) to gpui.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS_DIR
            .get_file(path)
            .map(|file| Cow::Borrowed(file.contents())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let Some(dir) = ASSETS_DIR.get_dir(path) else {
            return Ok(Vec::new());
        };

        Ok(dir
            .entries()
            .iter()
            .map(|entry| SharedString::from(entry.path().to_string_lossy().into_owned()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::Assets;
    use gpui::AssetSource;

    #[test]
    fn loads_embedded_icon_bytes() {
        let bytes = Assets
            .load("icons/square-plus.svg")
            .expect("load result")
            .expect("square-plus.svg should be embedded");
        assert!(!bytes.is_empty(), "embedded icon should have bytes");
    }

    #[test]
    fn missing_asset_returns_none() {
        let result = Assets
            .load("icons/does-not-exist.svg")
            .expect("load result");
        assert!(result.is_none(), "absent asset should resolve to None");
    }
}
