//! Greviewer library entry point.

use gpui::{px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use gpui_component::TitleBar;

use crate::assets::Assets;

pub mod app;
pub mod assets;
pub mod graph;
pub mod icons;
pub mod repo;

pub fn run() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        gpui_component::init(cx);

        app::bind_app_keys(cx);

        let (menus, _menu_snapshot) = app::build_app_menus();
        cx.set_menus(menus);

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| app::App::new(window, cx)),
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
