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
    /// `chevron-down.svg`
    ChevronDown,
    /// `chevron-right.svg`
    ChevronRight,
    /// `chevron-up.svg`
    ChevronUp,
    /// `chevrons-down-up.svg`
    ChevronsDownUp,
    /// `chevrons-up-down.svg`
    ChevronsUpDown,
    /// `cloud.svg`
    Cloud,
    /// `columns-2.svg`
    Columns2,
    /// `eye.svg`
    Eye,
    /// `eye-off.svg`
    EyeOff,
    /// `git-branch.svg`
    GitBranch,
    /// `git-branch-plus.svg`
    GitBranchPlus,
    /// `git-compare-arrows.svg`
    GitCompareArrows,
    /// `list-tree.svg`
    ListTree,
    /// `monitor.svg`
    Monitor,
    /// `rows-2.svg`
    Rows2,
    /// `search.svg`
    Search,
    /// `square-dot.svg`
    SquareDot,
    /// `square-minus.svg`
    SquareMinus,
    /// `square-plus.svg`
    SquarePlus,
    /// `tag.svg`
    Tag,
    /// `x.svg`
    X,
}

impl LucideIcon {
    /// Every variant, used by tests to assert each has a vendored asset.
    /// Keep in sync with the enum variants when adding or removing an icon.
    pub const ALL: &[LucideIcon] = &[
        LucideIcon::Check,
        LucideIcon::ChevronDown,
        LucideIcon::ChevronRight,
        LucideIcon::ChevronUp,
        LucideIcon::ChevronsDownUp,
        LucideIcon::ChevronsUpDown,
        LucideIcon::Cloud,
        LucideIcon::Columns2,
        LucideIcon::Eye,
        LucideIcon::EyeOff,
        LucideIcon::GitBranch,
        LucideIcon::GitBranchPlus,
        LucideIcon::GitCompareArrows,
        LucideIcon::ListTree,
        LucideIcon::Monitor,
        LucideIcon::Rows2,
        LucideIcon::Search,
        LucideIcon::SquareDot,
        LucideIcon::SquareMinus,
        LucideIcon::SquarePlus,
        LucideIcon::Tag,
        LucideIcon::X,
    ];
}

impl IconNamed for LucideIcon {
    fn path(self) -> SharedString {
        match self {
            LucideIcon::Check => "icons/check.svg",
            LucideIcon::ChevronDown => "icons/chevron-down.svg",
            LucideIcon::ChevronRight => "icons/chevron-right.svg",
            LucideIcon::ChevronUp => "icons/chevron-up.svg",
            LucideIcon::ChevronsDownUp => "icons/chevrons-down-up.svg",
            LucideIcon::ChevronsUpDown => "icons/chevrons-up-down.svg",
            LucideIcon::Cloud => "icons/cloud.svg",
            LucideIcon::Columns2 => "icons/columns-2.svg",
            LucideIcon::Eye => "icons/eye.svg",
            LucideIcon::EyeOff => "icons/eye-off.svg",
            LucideIcon::GitBranch => "icons/git-branch.svg",
            LucideIcon::GitBranchPlus => "icons/git-branch-plus.svg",
            LucideIcon::GitCompareArrows => "icons/git-compare-arrows.svg",
            LucideIcon::ListTree => "icons/list-tree.svg",
            LucideIcon::Monitor => "icons/monitor.svg",
            LucideIcon::Rows2 => "icons/rows-2.svg",
            LucideIcon::Search => "icons/search.svg",
            LucideIcon::SquareDot => "icons/square-dot.svg",
            LucideIcon::SquareMinus => "icons/square-minus.svg",
            LucideIcon::SquarePlus => "icons/square-plus.svg",
            LucideIcon::Tag => "icons/tag.svg",
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
