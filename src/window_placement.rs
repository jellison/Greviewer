//! Translation between gpui's live window/display types and the serializable
//! [`WindowState`] we persist, plus the startup logic that turns saved state
//! into window-open options. Kept separate from `settings` so that module
//! stays free of any gpui dependency.

use gpui::{point, px, size, App, Bounds, DisplayId, Pixels, Size, Window, WindowBounds};

use crate::settings::{Settings, WindowMode, WindowState};

#[cfg(test)]
use crate::settings::GraphViewMode;

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

/// The saved size, clamped up to [`MIN_WINDOW_SIZE`] so a degenerate saved value
/// cannot produce an unusable window.
fn clamped_size(state: &WindowState) -> Size<Pixels> {
    size(
        px(state.width.max(f32::from(MIN_WINDOW_SIZE.width))),
        px(state.height.max(f32::from(MIN_WINDOW_SIZE.height))),
    )
}

/// Wrap `bounds` in the [`WindowBounds`] variant matching `state`'s window mode.
fn bounds_in_mode(state: &WindowState, bounds: Bounds<Pixels>) -> WindowBounds {
    match state.mode {
        WindowMode::Windowed => WindowBounds::Windowed(bounds),
        WindowMode::Maximized => WindowBounds::Maximized(bounds),
        WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    }
}

/// Reconstruct gpui window bounds from persisted state, clamping the size up to
/// [`MIN_WINDOW_SIZE`] so a degenerate saved value cannot open an unusable
/// window. Used when the saved monitor is still connected, so the saved position
/// is honored as-is.
pub fn window_bounds_from(state: &WindowState) -> WindowBounds {
    bounds_in_mode(
        state,
        Bounds {
            origin: point(px(state.x), px(state.y)),
            size: clamped_size(state),
        },
    )
}

/// Reconstruct window bounds for the case where the monitor the state was saved
/// on is not among the currently connected displays. Losing the monitor must not
/// lose the user's window size: a window's size has nothing to do with which
/// screen shows it, so the saved size and window mode are preserved. Only the
/// position is adjusted — clamped onto the primary display — so the window can
/// never be restored partly or wholly off-screen on the new arrangement.
fn window_bounds_without_saved_display(state: &WindowState, cx: &App) -> WindowBounds {
    let size = clamped_size(state);
    let origin = match cx.primary_display() {
        Some(display) => {
            let display_bounds = display.bounds();
            let max_x = (f32::from(display_bounds.size.width) - f32::from(size.width)).max(0.0);
            let max_y = (f32::from(display_bounds.size.height) - f32::from(size.height)).max(0.0);
            point(px(state.x.clamp(0.0, max_x)), px(state.y.clamp(0.0, max_y)))
        }
        None => point(px(state.x), px(state.y)),
    };
    bounds_in_mode(state, Bounds { origin, size })
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

/// Decide the bounds and target display for opening the main window.
///
/// When saved state exists and its monitor is still connected, the window is
/// restored exactly — size, position, mode, and monitor. When the saved monitor
/// is *not* connected, the size and mode are still preserved and only the
/// position is re-homed onto the primary display; this matters because a
/// monitor's identity is not always stable across sessions (some systems,
/// notably headless or virtual displays, report a fresh display UUID after a
/// sleep or reconnect), and losing that match must not silently reset the
/// window to a default size. With no saved state at all, a default-size window
/// is centered on the primary display.
pub fn restore_window_options(settings: &Settings, cx: &App) -> (WindowBounds, Option<DisplayId>) {
    if let Some(state) = &settings.window_state {
        if let Some(display_id) = resolve_display_id(&state.display, cx) {
            return (window_bounds_from(state), Some(display_id));
        }
        return (window_bounds_without_saved_display(state, cx), None);
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
    use crate::settings::{Settings, SidebarWidths, WindowMode, WindowState};
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
                sidebar_widths: SidebarWidths::default(),
                ai_enabled: false,
                graph_view_mode: GraphViewMode::default(),
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
    fn restore_keeps_saved_size_and_position_when_display_is_gone(cx: &mut TestAppContext) {
        // The saved monitor is not connected (or its identity changed across
        // sessions). The window must NOT reset to a default size: the saved
        // size and an on-screen position are preserved, only the monitor is
        // dropped. This is the fix for geometry being lost across rebuilds on
        // systems whose display UUID is not stable.
        cx.update(|cx| {
            let settings = Settings {
                recent_repositories: vec![],
                window_state: Some(sample_state(WindowMode::Windowed, "uuid-not-connected")),
                sidebar_widths: SidebarWidths::default(),
                ai_enabled: false,
                graph_view_mode: GraphViewMode::default(),
            };

            let (bounds, display_id) = restore_window_options(&settings, cx);

            assert!(display_id.is_none());
            let WindowBounds::Windowed(bounds) = bounds else {
                panic!("expected windowed fallback");
            };
            // Saved size is preserved, not reset to the default.
            assert_eq!(f32::from(bounds.size.width), 1024.0);
            assert_eq!(f32::from(bounds.size.height), 768.0);
            // The saved position (100, 200) fits on the test display, so it is
            // kept exactly.
            assert_eq!(f32::from(bounds.origin.x), 100.0);
            assert_eq!(f32::from(bounds.origin.y), 200.0);
        });
    }

    #[gpui::test]
    fn restore_clamps_offscreen_position_onto_the_primary_display(cx: &mut TestAppContext) {
        // A position saved on a now-missing monitor can land off-screen on the
        // remaining arrangement; it is clamped so the window is always visible,
        // while the saved size is still preserved.
        cx.update(|cx| {
            let mut state = sample_state(WindowMode::Windowed, "uuid-not-connected");
            state.x = 5000.0;
            state.y = 5000.0;
            let settings = Settings {
                recent_repositories: vec![],
                window_state: Some(state),
                sidebar_widths: SidebarWidths::default(),
                ai_enabled: false,
                graph_view_mode: GraphViewMode::default(),
            };

            let (bounds, display_id) = restore_window_options(&settings, cx);

            assert!(display_id.is_none());
            let WindowBounds::Windowed(bounds) = bounds else {
                panic!("expected windowed fallback");
            };
            let display = cx.primary_display().expect("primary display");
            let display_bounds = display.bounds();
            // Fully on-screen: right/bottom edges do not exceed the display.
            assert!(f32::from(bounds.origin.x) + 1024.0 <= f32::from(display_bounds.size.width));
            assert!(f32::from(bounds.origin.y) + 768.0 <= f32::from(display_bounds.size.height));
            assert!(f32::from(bounds.origin.x) >= 0.0);
            assert!(f32::from(bounds.origin.y) >= 0.0);
            // Size is still the saved size.
            assert_eq!(f32::from(bounds.size.width), 1024.0);
            assert_eq!(f32::from(bounds.size.height), 768.0);
        });
    }

    #[gpui::test]
    fn restore_preserves_window_mode_when_display_is_gone(cx: &mut TestAppContext) {
        // A maximized/fullscreen window whose monitor is gone stays in that mode
        // (its saved size is the restore size for when the user leaves the mode).
        cx.update(|cx| {
            for mode in [WindowMode::Maximized, WindowMode::Fullscreen] {
                let settings = Settings {
                    recent_repositories: vec![],
                    window_state: Some(sample_state(mode, "uuid-not-connected")),
                    sidebar_widths: SidebarWidths::default(),
                    ai_enabled: false,
                    graph_view_mode: GraphViewMode::default(),
                };

                let (bounds, _display_id) = restore_window_options(&settings, cx);

                match mode {
                    WindowMode::Maximized => {
                        assert!(matches!(bounds, WindowBounds::Maximized(_)))
                    }
                    WindowMode::Fullscreen => {
                        assert!(matches!(bounds, WindowBounds::Fullscreen(_)))
                    }
                    WindowMode::Windowed => unreachable!(),
                }
            }
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
