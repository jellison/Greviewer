//! Changeset status footer: a files-panel toggle on the left and, once AI is
//! enabled, the review-guide controls (sparkle to open the guide, dock
//! toggle for its panel) on the right. Only ever rendered from
//! `render_changeset_screen`, so its presence alone scopes it to the
//! changeset screen. See docs/specs for the guide feature (Task 8 renders
//! the guide panel's own contents; this task only wires the toggles).

use gpui::{div, px, AnyElement, Context, IntoElement, ParentElement, Styled};

use super::App;
use crate::icons::LucideIcon;
use crate::theme::palette;

/// Matches `diff_view`'s `CHANGE_BLOCK_FOOTER_HEIGHT`, the established
/// height for a slim status bar in this app.
const STATUS_FOOTER_HEIGHT: f32 = 28.;

impl App {
    /// Full-width footer below the changeset body.
    pub(crate) fn render_status_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let files_open = self.settings.changeset_panels.files_open;
        let guide_open = self.settings.changeset_panels.guide_open;

        let files_toggle = self.render_file_tree_icon_button(
            LucideIcon::PanelLeft,
            "footer-files-toggle",
            "Toggle file list",
            files_open,
            |app, _window, cx| app.toggle_changeset_files_panel(cx),
            cx,
        );

        let ai_controls: Option<AnyElement> =
            if self.settings.ai_enabled && self.open_changeset_supports_guide() {
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(self.render_file_tree_icon_button(
                            LucideIcon::Sparkles,
                            "footer-guide-sparkle",
                            "Review Guide",
                            false,
                            |app, _window, cx| app.open_guide_panel(cx),
                            cx,
                        ))
                        .child(self.render_file_tree_icon_button(
                            LucideIcon::PanelRight,
                            "footer-dock-toggle",
                            "Toggle guide panel",
                            guide_open,
                            |app, _window, cx| app.toggle_guide_panel(cx),
                            cx,
                        ))
                        .into_any_element(),
                )
            } else {
                None
            };

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(STATUS_FOOTER_HEIGHT))
            .px_2()
            .border_t_1()
            .border_color(palette().border)
            .bg(palette().surface)
            .child(files_toggle)
            .children(ai_controls)
    }

    pub(crate) fn toggle_changeset_files_panel(&mut self, cx: &mut Context<Self>) {
        self.settings.changeset_panels.files_open = !self.settings.changeset_panels.files_open;
        self.persist_settings();
        cx.notify();
    }

    pub(crate) fn toggle_guide_panel(&mut self, cx: &mut Context<Self>) {
        self.settings.changeset_panels.guide_open = !self.settings.changeset_panels.guide_open;
        self.persist_settings();
        cx.notify();
    }

    /// Opens the guide panel; never closes it (the sparkle is a one-way
    /// "show me the guide" action, distinct from the dock toggle).
    pub(crate) fn open_guide_panel(&mut self, cx: &mut Context<Self>) {
        if !self.settings.changeset_panels.guide_open {
            self.settings.changeset_panels.guide_open = true;
            self.persist_settings();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};

    /// Open a one-commit repo's changeset in a fresh window. `_dir` keeps the
    /// backing temp repo alive for the test's duration.
    fn open_simple_changeset(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        let (dir, sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(sha, cx);
                app.open_changeset(window, cx);
            })
            .expect("open simple changeset");
        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        (dir, window, visual)
    }

    #[gpui::test]
    async fn files_toggle_collapses_and_restores_the_file_list(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_simple_changeset(cx);
        assert!(
            visual.debug_bounds("changed-file-row-0").is_some(),
            "file list starts open"
        );
        let toggle = visual
            .debug_bounds("footer-files-toggle")
            .expect("footer toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());
        cx.run_until_parked();

        // `debug_bounds` (see gpui's `Frame::clear`) never removes a selector
        // once painted, so it cannot prove the file list's absence here —
        // assert against the setting the render is gated on instead (see the
        // analogous fix in `branch_sidebar`'s Reviews-section filter test).
        window
            .read_with(cx, |app, _| {
                assert!(
                    !app.settings.changeset_panels.files_open,
                    "clicking the toggle hides the file list"
                );
            })
            .unwrap();

        let toggle = visual
            .debug_bounds("footer-files-toggle")
            .expect("still present");
        visual.simulate_click(toggle.center(), Modifiers::none());
        cx.run_until_parked();
        assert!(
            visual.debug_bounds("changed-file-row-0").is_some(),
            "file list restored"
        );
    }

    #[gpui::test]
    async fn ai_controls_render_only_when_ai_is_enabled(cx: &mut TestAppContext) {
        let (dir, sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        // Opt out BEFORE the first render: once a selector has painted,
        // `debug_bounds` can never prove its absence (see the comment in the
        // files-toggle test), so the disabled state must come first.
        window
            .update(cx, |app, window, cx| {
                app.settings.ai_enabled = false;
                app.open_repository_at(path, window, cx);
                app.select_single_commit(sha, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset with AI opted out");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("footer-guide-sparkle").is_none());
        assert!(visual.debug_bounds("footer-dock-toggle").is_none());
        window
            .update(cx, |app, _window, cx| {
                app.settings.ai_enabled = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert!(visual.debug_bounds("footer-guide-sparkle").is_some());
        assert!(visual.debug_bounds("footer-dock-toggle").is_some());
    }

    #[gpui::test]
    async fn pending_changeset_never_offers_guide_controls(cx: &mut TestAppContext) {
        use crate::app::Selection;
        use std::fs;

        let (dir, _sha) = init_repo_with_one_commit();
        fs::write(dir.path().join("dirty.txt"), "dirt\n").expect("write dirty file");
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.settings.ai_enabled = true;
                app.open_repository_at(path, window, cx);
                app.selection = Selection::Pending;
                app.open_changeset(window, cx);
            })
            .expect("open pending changeset");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("footer-files-toggle").is_some(),
            "files toggle still renders for the pending changeset"
        );
        assert!(
            visual.debug_bounds("footer-guide-sparkle").is_none(),
            "the pending changeset must never offer a guide"
        );
        assert!(
            visual.debug_bounds("footer-dock-toggle").is_none(),
            "the pending changeset must never offer a guide"
        );
    }

    #[gpui::test]
    async fn panel_visibility_persists_to_settings(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_simple_changeset(cx);
        let toggle = visual.debug_bounds("footer-files-toggle").expect("toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());
        cx.run_until_parked();
        window
            .read_with(cx, |app, _| {
                assert!(!app.settings.changeset_panels.files_open);
            })
            .unwrap();
    }
}
