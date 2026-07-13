//! The right-docked review sidebar: a tab strip over the AI guide (Review)
//! and the anchored-comments list (Comments). The strip and root chrome are
//! the only things owned here; each tab's body lives in its own module
//! (`guide_panel`, and eventually `comments_panel`).

use super::*;

use crate::theme::palette;

/// Height of the tab strip above the active tab's body.
pub(crate) const SIDEBAR_TAB_STRIP_HEIGHT: f32 = 30.;

/// Thickness of the active tab's accent underline.
const SIDEBAR_TAB_UNDERLINE_HEIGHT: f32 = 2.;

/// Tab-strip text size, independent of the file tree's own scale.
const SIDEBAR_TAB_TEXT_SIZE: f32 = 12.;

/// Comments-tab count badge text size.
const SIDEBAR_TAB_BADGE_TEXT_SIZE: f32 = 10.;

impl App {
    /// Render the review sidebar: root chrome, tab strip, and the active
    /// tab's body. With AI disabled the Review tab doesn't exist, so
    /// rendering falls back to Comments regardless of the last-selected
    /// `sidebar_tab` — this is a render-time fallback rather than a mutation
    /// of `sidebar_tab` itself, since the render chain above this point
    /// (`render_changeset_screen`, `render_repo_open`) only holds `&self`;
    /// `sidebar_tab` is updated for real by the tab's own click handler and
    /// by `stage_comment_draft`, both of which already have `&mut self`.
    pub(crate) fn render_review_sidebar(
        &self,
        changeset: &repo::ChangeSet,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let effective_tab = if self.settings.ai_enabled {
            self.sidebar_tab
        } else {
            SidebarTab::Comments
        };
        let body: AnyElement = match effective_tab {
            SidebarTab::Review => self.render_guide_tab(changeset, cx),
            SidebarTab::Comments => self.render_comments_tab(cx),
        };

        div()
            .id("review-sidebar")
            .debug_selector(|| "review-sidebar".to_string())
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .h_full()
            .min_h_0()
            .border_l_1()
            .border_color(palette().border)
            .bg(palette().surface)
            .child(self.render_sidebar_tab_strip(effective_tab, cx))
            .child(div().flex().flex_1().min_h_0().child(body))
    }

    /// The 30px strip above the active tab's body: Review (only with AI on)
    /// and Comments (always), the latter carrying a count badge once there
    /// are saved comments.
    fn render_sidebar_tab_strip(
        &self,
        effective_tab: SidebarTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut strip = div()
            .flex()
            .flex_none()
            .h(px(SIDEBAR_TAB_STRIP_HEIGHT))
            .border_b_1()
            .border_color(palette().border);
        if self.settings.ai_enabled {
            strip = strip.child(self.render_sidebar_tab(
                SidebarTab::Review,
                effective_tab,
                "Review",
                "sidebar-tab-review",
                None,
                cx,
            ));
        }
        let count = self.comment_count();
        strip.child(self.render_sidebar_tab(
            SidebarTab::Comments,
            effective_tab,
            "Comments",
            "sidebar-tab-comments",
            (count > 0).then_some(count),
            cx,
        ))
    }

    /// One tab: label plus an optional count badge, active state rendered as
    /// full-weight text with a bottom accent underline, inactive as muted
    /// text. Clicking switches `sidebar_tab`.
    fn render_sidebar_tab(
        &self,
        tab: SidebarTab,
        effective_tab: SidebarTab,
        label: &'static str,
        selector: &'static str,
        badge: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = effective_tab == tab;
        div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .relative()
            .flex()
            .items_center()
            .gap_1p5()
            .px_3()
            .cursor_pointer()
            .text_size(px(SIDEBAR_TAB_TEXT_SIZE))
            .when(active, |el| {
                el.text_color(palette().text)
                    .font_weight(FontWeight::MEDIUM)
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(SIDEBAR_TAB_UNDERLINE_HEIGHT))
                            .bg(palette().accent),
                    )
            })
            .when(!active, |el| el.text_color(palette().text_muted))
            .child(label)
            .when_some(badge, |el, count| {
                el.child(
                    div()
                        .debug_selector(move || format!("{selector}-badge"))
                        .font_family(MONO_FONT_FAMILY)
                        .text_size(px(SIDEBAR_TAB_BADGE_TEXT_SIZE))
                        .px(px(5.))
                        .rounded(px(3.))
                        .bg(palette().element_bg)
                        .text_color(palette().icon_muted)
                        .child(count.to_string()),
                )
            })
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                app.sidebar_tab = tab;
                cx.notify();
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use gpui::{Modifiers, TestAppContext};

    #[gpui::test]
    async fn tab_strip_switches_between_review_and_comments(cx: &mut TestAppContext) {
        let (_dir, _path, window, mut visual) = open_changeset_with_guide_panel(cx);
        // Both tabs render; Review is active by default with AI on.
        let review_tab = visual
            .debug_bounds("sidebar-tab-review")
            .expect("review tab");
        visual
            .debug_bounds("sidebar-tab-comments")
            .expect("comments tab");
        window
            .read_with(cx, |app, _| assert_eq!(app.sidebar_tab, SidebarTab::Review))
            .unwrap();
        let comments_tab = visual.debug_bounds("sidebar-tab-comments").unwrap();
        visual.simulate_click(comments_tab.center(), Modifiers::none());
        cx.run_until_parked();
        window
            .read_with(cx, |app, _| {
                assert_eq!(app.sidebar_tab, SidebarTab::Comments)
            })
            .unwrap();
        visual
            .debug_bounds("comments-empty-state")
            .expect("comments tab renders its empty state with no comments");
        visual.simulate_click(review_tab.center(), Modifiers::none());
        cx.run_until_parked();
        window
            .read_with(cx, |app, _| assert_eq!(app.sidebar_tab, SidebarTab::Review))
            .unwrap();
    }

    #[gpui::test]
    async fn badge_counts_saved_comments_across_files(cx: &mut TestAppContext) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        assert!(
            visual.debug_bounds("sidebar-tab-comments-badge").is_none(),
            "no badge at zero comments"
        );
        window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let id = app.current_review().expect("review").id.clone();
                app.reviews
                    .mutate(&id, |review| {
                        review.comments.push(test_comment(&path, 1));
                        review.comments.push(test_comment("other/file.py", 1));
                    })
                    .expect("mutate review");
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("sidebar-tab-comments-badge")
            .expect("badge appears");
        window
            .read_with(cx, |app, _| assert_eq!(app.comment_count(), 2))
            .unwrap();
    }

    #[gpui::test]
    async fn with_ai_off_the_sidebar_shows_only_the_comments_tab(cx: &mut TestAppContext) {
        let (_dir, _path, window, mut visual) = open_changeset_with_guide_panel(cx);
        window
            .update(cx, |app, _window, cx| {
                app.settings.ai_enabled = false;
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert!(visual.debug_bounds("sidebar-tab-review").is_none());
        visual
            .debug_bounds("sidebar-tab-comments")
            .expect("comments tab still there");
    }
}
