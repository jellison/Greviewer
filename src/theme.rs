//! Centralized UI color palette for greviewer.
//!
//! Every greviewer-drawn color reads from this module. Values are seeded
//! from the user's *OpenCode Material Dark* Zed theme
//! (`~/.config/zed/themes/opencode-material.json`); the values are authored
//! here and are not copied from any GPL-3.0 source (ADR-0001).
//!
//! Dark-only today. The `palette()` indirection means a future light
//! variant is a change inside this module, not at the call sites.

use std::sync::OnceLock;

use gpui::{rgb, rgba, App, Hsla};

/// The complete set of semantic UI colors. All fields are `Hsla`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    // Surfaces and structure.
    pub background: Hsla,
    pub surface: Hsla,
    pub element_bg: Hsla,
    pub element_hover: Hsla,
    pub ghost_element_hover: Hsla,
    pub border: Hsla,
    // Text.
    pub text: Hsla,
    pub text_muted: Hsla,
    pub icon_muted: Hsla,
    pub text_disabled: Hsla,
    // Accent and interaction.
    pub accent: Hsla,
    pub accent_bg: Hsla,
    pub accent_bg_hover: Hsla,
    pub row_selected: Hsla,
    pub drop_target: Hsla,
    pub match_highlight_bg: Hsla,
    // Diff status.
    pub diff_added_fg: Hsla,
    pub diff_added_bg: Hsla,
    pub diff_added_emphasis: Hsla,
    pub diff_removed_fg: Hsla,
    pub diff_removed_bg: Hsla,
    pub diff_removed_emphasis: Hsla,
    pub diff_empty_hatch: Hsla,
    pub code_text: Hsla,
    // Diff selection.
    pub diff_selection_bg: Hsla,
    pub diff_selection_bg_unfocused: Hsla,
    /// The text caret, everywhere it blinks: the diff view paints it directly,
    /// and `apply_to_gpui_component` hands it to the library's text inputs.
    pub caret: Hsla,
    pub active_line_bg: Hsla,
    // Review comment anchors (saved comments; pending drafts use `accent`).
    pub comment_anchor: Hsla,
    pub comment_anchor_fill: Hsla,
    // Change kinds.
    pub change_added: Hsla,
    pub change_modified: Hsla,
    pub change_deleted: Hsla,
    pub change_renamed: Hsla,
    // Danger.
    pub danger_fg: Hsla,
    pub danger_bg: Hsla,
    pub danger_border: Hsla,
    // Commit graph.
    pub commit_hash_fg: Hsla,
    pub ref_head_fg: Hsla,
    pub ref_head_bg: Hsla,
    pub ref_head_border: Hsla,
    pub ref_branch_fg: Hsla,
    pub ref_branch_bg: Hsla,
    pub ref_branch_border: Hsla,
    pub ref_tag_fg: Hsla,
    pub ref_tag_bg: Hsla,
    pub ref_tag_border: Hsla,
    pub ref_pr_fg: Hsla,
    pub ref_pr_bg: Hsla,
    pub ref_pr_border: Hsla,
    /// The "+N" ref-overflow badge: the pill triple in a neutral tone so the
    /// badge reads clearly without posing as another ref family.
    pub ref_overflow_fg: Hsla,
    pub ref_overflow_bg: Hsla,
    pub ref_overflow_border: Hsla,
    pub graph_lanes: [Hsla; 6],
}

impl Palette {
    /// The *OpenCode Material Dark* palette.
    fn opencode_material_dark() -> Self {
        Self {
            background: Hsla::from(rgb(0x263238)),
            surface: Hsla::from(rgb(0x1e272c)),
            element_bg: Hsla::from(rgb(0x37474f)),
            element_hover: Hsla::from(rgba(0x37474fcc)),
            ghost_element_hover: Hsla::from(rgba(0x37474f80)),
            border: Hsla::from(rgb(0x37474f)),
            text: Hsla::from(rgb(0xeeffff)),
            text_muted: Hsla::from(rgb(0x546e7a)),
            icon_muted: Hsla::from(rgb(0x718ca1)),
            text_disabled: Hsla::from(rgba(0x546e7a80)),
            accent: Hsla::from(rgb(0x82aaff)),
            accent_bg: Hsla::from(rgba(0x82aaff26)),
            accent_bg_hover: Hsla::from(rgba(0x82aaff40)),
            row_selected: Hsla::from(rgba(0x82aaff26)),
            drop_target: Hsla::from(rgba(0x82aaff26)),
            match_highlight_bg: Hsla::from(rgba(0xffcb6b40)),
            diff_added_fg: Hsla::from(rgb(0xc3e88d)),
            diff_added_bg: Hsla::from(rgba(0xc3e88d26)),
            diff_added_emphasis: Hsla::from(rgba(0xc3e88d40)),
            diff_removed_fg: Hsla::from(rgb(0xf07178)),
            diff_removed_bg: Hsla::from(rgba(0xf0717826)),
            diff_removed_emphasis: Hsla::from(rgba(0xf0717840)),
            diff_empty_hatch: Hsla::from(rgba(0x546e7acc)),
            code_text: Hsla::from(rgb(0xeeffff)),
            diff_selection_bg: Hsla::from(rgba(0x82aaff4d)),
            diff_selection_bg_unfocused: Hsla::from(rgba(0x82aaff26)),
            caret: Hsla::from(rgb(0x82aaff)),
            active_line_bg: Hsla::from(rgba(0xeeffff0f)),
            comment_anchor: Hsla::from(rgb(0xffcb6b)),
            comment_anchor_fill: Hsla::from(rgba(0xffcb6b1a)),
            change_added: Hsla::from(rgb(0xc3e88d)),
            change_modified: Hsla::from(rgb(0x82aaff)),
            change_deleted: Hsla::from(rgb(0xf07178)),
            change_renamed: Hsla::from(rgb(0x89ddff)),
            danger_fg: Hsla::from(rgb(0xf07178)),
            danger_bg: Hsla::from(rgba(0xf0717826)),
            danger_border: Hsla::from(rgba(0xf0717866)),
            commit_hash_fg: Hsla::from(rgb(0xc3e88d)),
            ref_head_fg: Hsla::from(rgb(0x82aaff)),
            ref_head_bg: Hsla::from(rgba(0x82aaff26)),
            ref_head_border: Hsla::from(rgba(0x82aaff66)),
            ref_branch_fg: Hsla::from(rgb(0xc3e88d)),
            ref_branch_bg: Hsla::from(rgba(0xc3e88d26)),
            ref_branch_border: Hsla::from(rgba(0xc3e88d66)),
            ref_tag_fg: Hsla::from(rgb(0xffcb6b)),
            ref_tag_bg: Hsla::from(rgba(0xffcb6b26)),
            ref_tag_border: Hsla::from(rgba(0xffcb6b66)),
            ref_pr_fg: Hsla::from(rgb(0xc792ea)),
            ref_pr_bg: Hsla::from(rgba(0xc792ea26)),
            ref_pr_border: Hsla::from(rgba(0xc792ea66)),
            ref_overflow_fg: Hsla::from(rgb(0x718ca1)),
            ref_overflow_bg: Hsla::from(rgba(0x718ca126)),
            ref_overflow_border: Hsla::from(rgba(0x718ca166)),
            graph_lanes: [
                Hsla::from(rgb(0x82aaff)),
                Hsla::from(rgb(0xc3e88d)),
                Hsla::from(rgb(0xffcb6b)),
                Hsla::from(rgb(0xc792ea)),
                Hsla::from(rgb(0x89ddff)),
                Hsla::from(rgb(0xf78c6c)),
            ],
        }
    }
}

/// Returns the active palette. Built once; dark-only.
pub fn palette() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(Palette::opencode_material_dark)
}

/// Copies the palette into gpui-component's global `Theme` so the widgets
/// greviewer does not paint itself — the title bar, the `Root` window
/// background, scrollbars, and inputs — match the palette. One-way: the
/// palette leads, the library follows. Call once after
/// `gpui_component::init`.
pub fn apply_to_gpui_component(cx: &mut App) {
    let p = palette();
    let theme = gpui_component::Theme::global_mut(cx);

    theme.background = p.background;
    theme.foreground = p.text;
    theme.border = p.border;
    theme.title_bar = p.surface;
    theme.title_bar_border = p.border;
    theme.sidebar = p.surface;
    theme.sidebar_border = p.border;
    theme.sidebar_foreground = p.text;
    theme.tab_bar = p.surface;
    theme.tab = p.surface;
    theme.tab_active = p.background;
    theme.tab_foreground = p.text_muted;
    theme.tab_active_foreground = p.text;
    theme.popover = p.surface;
    theme.popover_foreground = p.text;
    theme.muted_foreground = p.text_muted;
    theme.accent = p.accent;
    theme.ring = p.accent;
    theme.selection = p.row_selected;
    theme.drop_target = p.drop_target;
    theme.input = p.border;
    // Left unmapped, the library's inputs blink a near-black caret that is
    // invisible against the dark surface they sit on. Hand them the same caret
    // the diff view paints.
    theme.caret = p.caret;
    theme.scrollbar = Hsla::from(rgba(0x00000000));
    theme.scrollbar_thumb = Hsla::from(rgba(0x546e7a40));
    theme.scrollbar_thumb_hover = Hsla::from(rgba(0x546e7a66));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_seed_values_match_opencode_material_dark() {
        let p = palette();
        assert_eq!(p.background, Hsla::from(rgb(0x263238)));
        assert_eq!(p.surface, Hsla::from(rgb(0x1e272c)));
        assert_eq!(p.border, Hsla::from(rgb(0x37474f)));
        assert_eq!(p.text, Hsla::from(rgb(0xeeffff)));
        assert_eq!(p.text_muted, Hsla::from(rgb(0x546e7a)));
        assert_eq!(p.icon_muted, Hsla::from(rgb(0x718ca1)));
        assert_eq!(p.ghost_element_hover, Hsla::from(rgba(0x37474f80)));
        assert_eq!(p.accent, Hsla::from(rgb(0x82aaff)));
        assert_eq!(p.diff_added_bg, Hsla::from(rgba(0xc3e88d26)));
        assert_eq!(p.diff_removed_bg, Hsla::from(rgba(0xf0717826)));
        assert_eq!(p.change_renamed, Hsla::from(rgb(0x89ddff)));
        assert_eq!(p.graph_lanes[3], Hsla::from(rgb(0xc792ea)));
        // The ref-overflow badge follows the ref-pill triple pattern in the
        // neutral icon_muted tone, bright enough to read beside the pills.
        assert_eq!(p.ref_overflow_fg, Hsla::from(rgb(0x718ca1)));
        assert_eq!(p.ref_overflow_bg, Hsla::from(rgba(0x718ca126)));
        assert_eq!(p.ref_overflow_border, Hsla::from(rgba(0x718ca166)));
        assert_eq!(p.ref_pr_fg, Hsla::from(rgb(0xc792ea)));
        assert_eq!(p.ref_pr_bg, Hsla::from(rgba(0xc792ea26)));
        assert_eq!(p.ref_pr_border, Hsla::from(rgba(0xc792ea66)));
        assert_eq!(p.comment_anchor, Hsla::from(rgb(0xffcb6b)));
        assert_eq!(p.comment_anchor_fill, Hsla::from(rgba(0xffcb6b1a)));
    }

    #[test]
    fn palette_returns_the_same_instance() {
        assert!(std::ptr::eq(palette(), palette()));
    }

    #[gpui::test]
    fn apply_overrides_gpui_component_theme(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            apply_to_gpui_component(cx);

            let theme = <gpui::App as gpui_component::ActiveTheme>::theme(cx);
            let p = palette();
            assert_eq!(theme.background, p.background);
            assert_eq!(theme.title_bar, p.surface);
            assert_eq!(theme.title_bar_border, p.border);
            assert_eq!(theme.sidebar, p.surface);
            assert_eq!(theme.tab_bar, p.surface);
            assert_eq!(theme.tab_active, p.background);
            assert_eq!(theme.accent, p.accent);
            assert_eq!(theme.scrollbar_thumb, Hsla::from(rgba(0x546e7a40)));
        });
    }

    #[gpui::test]
    fn library_inputs_blink_the_palette_caret(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let before = <gpui::App as gpui_component::ActiveTheme>::theme(cx).caret;
            apply_to_gpui_component(cx);

            let theme = <gpui::App as gpui_component::ActiveTheme>::theme(cx);
            let p = palette();
            assert_eq!(
                theme.caret, p.caret,
                "the library's inputs must paint the palette's caret"
            );
            assert_ne!(
                before, p.caret,
                "the library default is not already the palette caret, so the \
                 mapping is what makes the caret visible on the dark surface"
            );
        });
    }
}
