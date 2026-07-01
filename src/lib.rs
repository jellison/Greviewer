//! Greviewer library entry point.

use gpui::{App, AppContext, Application, WindowOptions};
use gpui_component::{Root, TitleBar};

use crate::assets::Assets;
use crate::window_placement::{restore_window_options, MIN_WINDOW_SIZE};

pub mod app;
pub mod assets;
pub mod diff_highlight;
pub mod graph;
pub mod icons;
pub mod repo;
pub mod settings;
pub mod theme;
pub mod window_placement;
pub mod workspace;

pub fn run() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        crate::theme::apply_to_gpui_component(cx);

        app::bind_app_keys(cx);

        let (menus, _menu_snapshot) = app::build_app_menus();
        cx.set_menus(menus);

        let settings_store_path = settings::default_store_path();
        let settings = settings_store_path
            .as_deref()
            .map(settings::load)
            .unwrap_or_default();

        let (window_bounds, display_id) = restore_window_options(&settings, cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(window_bounds),
                display_id,
                window_min_size: Some(MIN_WINDOW_SIZE),
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            },
            // The window's root entity must be a `gpui_component::Root`: components
            // like `Input` reach for it via `Root::read`/`Root::update` during paint
            // (see gpui_component::input), and panic if the window's first layer is
            // not a `Root`. Wrap the `App` view in one so those components work.
            move |window, cx| {
                let app = cx.new(|cx| {
                    app::App::new_with_loaded_settings(window, cx, settings, settings_store_path)
                });
                cx.new(|cx| Root::new(app, window, cx))
            },
        )
        .expect("opening the main window");

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.activate(true);
    });
}
