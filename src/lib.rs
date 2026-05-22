//! Greviewer library entry point.

use gpui::{
    px, size, App, AppContext, Application, Bounds, KeyBinding, Menu, MenuItem, SharedString,
    TitlebarOptions, WindowBounds, WindowOptions,
};

pub mod app;
pub mod repo;

pub fn run() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        cx.bind_keys([KeyBinding::new("cmd-o", app::OpenRepository, None)]);

        cx.set_menus(vec![Menu {
            name: SharedString::from("Greviewer"),
            items: vec![MenuItem::action(
                "Open Repository\u{2026}",
                app::OpenRepository,
            )],
        }]);

        cx.on_action(|_: &app::OpenRepository, cx: &mut App| {
            let Some(window) = cx
                .active_window()
                .and_then(|handle| handle.downcast::<app::App>())
            else {
                return;
            };
            window
                .update(cx, |app, window, cx| {
                    app.prompt_and_open_repository(window, cx);
                })
                .ok();
        });

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Greviewer")),
                    ..Default::default()
                }),
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
