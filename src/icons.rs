//! Typed references to vendored Lucide icons.

use gpui::SharedString;
use gpui_component::IconNamed;

/// A vendored Lucide icon. Each variant maps to an SVG embedded under
/// `assets/icons/`. To add an icon: drop the SVG into `assets/icons/`, add a
/// variant here, map it in `path`, and add it to `ALL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LucideIcon {
    /// `check.svg`
    Check,
    /// `chevrons-down-up.svg`
    ChevronsDownUp,
    /// `chevrons-up-down.svg`
    ChevronsUpDown,
    /// `columns-2.svg`
    Columns2,
    /// `list-tree.svg`
    ListTree,
    /// `rows-2.svg`
    Rows2,
    /// `square-dot.svg`
    SquareDot,
    /// `square-minus.svg`
    SquareMinus,
    /// `square-plus.svg`
    SquarePlus,
    /// `x.svg`
    X,
}

impl LucideIcon {
    /// Every variant, used by tests to assert each has a vendored asset.
    /// Keep in sync with the enum variants when adding or removing an icon.
    pub const ALL: &[LucideIcon] = &[
        LucideIcon::Check,
        LucideIcon::ChevronsDownUp,
        LucideIcon::ChevronsUpDown,
        LucideIcon::Columns2,
        LucideIcon::ListTree,
        LucideIcon::Rows2,
        LucideIcon::SquareDot,
        LucideIcon::SquareMinus,
        LucideIcon::SquarePlus,
        LucideIcon::X,
    ];
}

impl IconNamed for LucideIcon {
    fn path(self) -> SharedString {
        match self {
            LucideIcon::Check => "icons/check.svg",
            LucideIcon::ChevronsDownUp => "icons/chevrons-down-up.svg",
            LucideIcon::ChevronsUpDown => "icons/chevrons-up-down.svg",
            LucideIcon::Columns2 => "icons/columns-2.svg",
            LucideIcon::ListTree => "icons/list-tree.svg",
            LucideIcon::Rows2 => "icons/rows-2.svg",
            LucideIcon::SquareDot => "icons/square-dot.svg",
            LucideIcon::SquareMinus => "icons/square-minus.svg",
            LucideIcon::SquarePlus => "icons/square-plus.svg",
            LucideIcon::X => "icons/x.svg",
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
