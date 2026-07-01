//! Translation between gpui's live window/display types and the serializable
//! [`WindowState`] we persist, plus the startup logic that turns saved state
//! into window-open options. Kept separate from `settings` so that module
//! stays free of any gpui dependency.

use gpui::{point, px, size, App, Bounds, DisplayId, Pixels, Size, Window, WindowBounds};

use crate::settings::{Settings, WindowMode, WindowState};

/// Size of a freshly-placed window when there is nothing to restore.
pub const DEFAULT_WINDOW_SIZE: Size<Pixels> = Size {
    width: px(1280.),
    height: px(800.),
};

/// Smallest size the window may be opened or restored at, so a corrupt or
/// degenerate saved geometry can never produce an unusable window.
pub const MIN_WINDOW_SIZE: Size<Pixels> = Size {
    width: px(480.),
    height: px(320.),
};

/// Build the serializable state from live window bounds and the monitor UUID.
pub fn window_state_from(bounds: WindowBounds, display: String) -> WindowState {
    let (mode, b) = match bounds {
        WindowBounds::Windowed(b) => (WindowMode::Windowed, b),
        WindowBounds::Maximized(b) => (WindowMode::Maximized, b),
        WindowBounds::Fullscreen(b) => (WindowMode::Fullscreen, b),
    };
    WindowState {
        display,
        mode,
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        width: f32::from(b.size.width),
        height: f32::from(b.size.height),
    }
}

/// Reconstruct gpui window bounds from persisted state, clamping the size up to
/// [`MIN_WINDOW_SIZE`] so a degenerate saved value cannot open an unusable
/// window.
pub fn window_bounds_from(state: &WindowState) -> WindowBounds {
    let bounds = Bounds {
        origin: point(px(state.x), px(state.y)),
        size: size(
            px(state.width.max(f32::from(MIN_WINDOW_SIZE.width))),
            px(state.height.max(f32::from(MIN_WINDOW_SIZE.height))),
        ),
    };
    match state.mode {
        WindowMode::Windowed => WindowBounds::Windowed(bounds),
        WindowMode::Maximized => WindowBounds::Maximized(bounds),
        WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    }
}

/// Find the live display whose stable UUID matches `saved`, returning its
/// (session-transient) `DisplayId`. `None` when no connected monitor matches —
/// e.g. the monitor was disconnected since the state was saved.
pub fn resolve_display_id(saved: &str, cx: &App) -> Option<DisplayId> {
    let available: Vec<(String, DisplayId)> = cx
        .displays()
        .into_iter()
        .filter_map(|display| {
            let uuid = display.uuid().ok()?.to_string();
            Some((uuid, display.id()))
        })
        .collect();
    matching_display_index(
        saved,
        &available.iter().map(|(u, _)| u.clone()).collect::<Vec<_>>(),
    )
    .map(|index| available[index].1)
}

/// Pure core of [`resolve_display_id`]: index of the first available UUID equal
/// to `saved`, or `None`. Split out so it is unit-testable without live
/// displays.
fn matching_display_index(saved: &str, available: &[String]) -> Option<usize> {
    available.iter().position(|uuid| uuid == saved)
}

/// Decide the bounds and target display for opening the main window. Restores
/// the saved state when it exists and its monitor is still connected; otherwise
/// falls back to a centered default on the primary display.
pub fn restore_window_options(settings: &Settings, cx: &App) -> (WindowBounds, Option<DisplayId>) {
    if let Some(state) = &settings.window_state {
        if let Some(display_id) = resolve_display_id(&state.display, cx) {
            return (window_bounds_from(state), Some(display_id));
        }
    }
    let centered = Bounds::centered(None, DEFAULT_WINDOW_SIZE, cx);
    (WindowBounds::Windowed(centered), None)
}

/// Read the live window geometry and monitor UUID into a [`WindowState`], or
/// `None` when the window has no resolvable display.
pub fn capture_window_state(window: &Window, cx: &App) -> Option<WindowState> {
    let display = window.display(cx)?.uuid().ok()?.to_string();
    Some(window_state_from(window.window_bounds(), display))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Settings, WindowMode, WindowState};
    use gpui::TestAppContext;

    fn sample_state(mode: WindowMode, display: &str) -> WindowState {
        WindowState {
            display: display.to_string(),
            mode,
            x: 100.0,
            y: 200.0,
            width: 1024.0,
            height: 768.0,
        }
    }

    #[test]
    fn window_bounds_round_trip_preserves_mode_and_geometry() {
        for mode in [
            WindowMode::Windowed,
            WindowMode::Maximized,
            WindowMode::Fullscreen,
        ] {
            let state = sample_state(mode, "uuid-a");
            let bounds = window_bounds_from(&state);
            let back = window_state_from(bounds, "uuid-a".to_string());
            assert_eq!(back, state);
        }
    }

    #[test]
    fn window_bounds_from_clamps_tiny_sizes_up_to_the_minimum() {
        let mut state = sample_state(WindowMode::Windowed, "uuid-a");
        state.width = 10.0;
        state.height = 10.0;
        let WindowBounds::Windowed(bounds) = window_bounds_from(&state) else {
            panic!("expected windowed");
        };
        assert_eq!(
            f32::from(bounds.size.width),
            f32::from(MIN_WINDOW_SIZE.width)
        );
        assert_eq!(
            f32::from(bounds.size.height),
            f32::from(MIN_WINDOW_SIZE.height)
        );
    }

    #[test]
    fn matching_display_index_finds_or_misses() {
        let available = vec!["uuid-a".to_string(), "uuid-b".to_string()];
        assert_eq!(matching_display_index("uuid-b", &available), Some(1));
        assert_eq!(matching_display_index("uuid-missing", &available), None);
    }

    #[gpui::test]
    fn restore_uses_saved_state_when_the_display_matches(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let saved_uuid = cx.displays()[0].uuid().expect("display uuid").to_string();
            let settings = Settings {
                recent_repositories: vec![],
                window_state: Some(sample_state(WindowMode::Windowed, &saved_uuid)),
            };

            let (bounds, display_id) = restore_window_options(&settings, cx);

            assert!(display_id.is_some());
            let WindowBounds::Windowed(bounds) = bounds else {
                panic!("expected windowed restore");
            };
            assert_eq!(f32::from(bounds.size.width), 1024.0);
            assert_eq!(f32::from(bounds.size.height), 768.0);
        });
    }

    #[gpui::test]
    fn restore_falls_back_to_centered_default_when_display_is_gone(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings = Settings {
                recent_repositories: vec![],
                window_state: Some(sample_state(WindowMode::Windowed, "uuid-not-connected")),
            };

            let (bounds, display_id) = restore_window_options(&settings, cx);

            assert!(display_id.is_none());
            let WindowBounds::Windowed(bounds) = bounds else {
                panic!("expected windowed fallback");
            };
            assert_eq!(
                f32::from(bounds.size.width),
                f32::from(DEFAULT_WINDOW_SIZE.width)
            );
            assert_eq!(
                f32::from(bounds.size.height),
                f32::from(DEFAULT_WINDOW_SIZE.height)
            );
        });
    }

    #[gpui::test]
    fn restore_defaults_when_no_state_saved(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let (bounds, display_id) = restore_window_options(&Settings::default(), cx);
            assert!(display_id.is_none());
            assert!(matches!(bounds, WindowBounds::Windowed(_)));
        });
    }
}
