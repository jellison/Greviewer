//! Typed references to vendored Lucide icons.

use gpui::SharedString;
use gpui_component::IconNamed;

/// A vendored Lucide icon. Each variant maps to an SVG embedded under
/// `assets/icons/`. To add an icon: drop the SVG into `assets/icons/`, add a
/// variant here, map it in `path`, and add it to `ALL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LucideIcon {
    /// `square-dot.svg`
    SquareDot,
    /// `square-minus.svg`
    SquareMinus,
    /// `square-plus.svg`
    SquarePlus,
}

impl LucideIcon {
    /// Every variant, used by tests to assert each has a vendored asset.
    /// Keep in sync with the enum variants when adding or removing an icon.
    pub const ALL: &[LucideIcon] = &[
        LucideIcon::SquareDot,
        LucideIcon::SquareMinus,
        LucideIcon::SquarePlus,
    ];
}

impl IconNamed for LucideIcon {
    fn path(self) -> SharedString {
        match self {
            LucideIcon::SquareDot => "icons/square-dot.svg",
            LucideIcon::SquareMinus => "icons/square-minus.svg",
            LucideIcon::SquarePlus => "icons/square-plus.svg",
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::LucideIcon;
    use crate::assets::Assets;
    use gpui::{
        div, AssetSource, Context, IntoElement, ParentElement as _, Render, TestAppContext, Window,
    };
    use gpui_component::{Icon, IconNamed};

    #[test]
    fn every_variant_resolves_to_a_vendored_asset() {
        for icon in LucideIcon::ALL {
            let path = icon.path();
            let loaded = Assets.load(path.as_ref()).expect("load result");
            assert!(loaded.is_some(), "no vendored asset for {path}");
        }
    }

    struct IconHarness;

    impl Render for IconHarness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().child(Icon::new(LucideIcon::SquarePlus))
        }
    }

    #[gpui::test]
    async fn icon_widget_renders_without_panicking(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let window = cx.add_window(|_window, _cx| IconHarness);
        cx.run_until_parked();
        window
            .update(cx, |_view, _window, _cx| {})
            .expect("icon harness window should render");
    }
}
