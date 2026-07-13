//! The Comments tab of the right-docked review sidebar: one flat,
//! conventionally scrolling list of the open changeset's saved comments,
//! most recent first (`created_at` descending, ties broken later-inserted
//! first — see `recency_ordered`). The list is independent of which file is
//! open: no grouping, no expand/collapse. Each row's meta line names its
//! anchor as "{file basename}:{line ref}" with a right-aligned timestamp.
//! While a draft is staged, its composer renders as the first row (the
//! most-recent-in-progress item).
//!
//! Navigation is click-driven in both directions and each click moves only
//! the opposite surface: clicking a row selects the comment, opens its
//! file, and scrolls the diff to its anchor — never this list; clicking
//! anchored text in the diff selects the comment and scrolls this list to
//! bring its row near the top (see `apply_pending_comments_list_scroll`).
//! Scrolling the diff never moves the list, and scrolling the list never
//! moves the diff.

use super::*;

use crate::reviews::ReviewComment;
use crate::theme::palette;

/// Gap between the meta line and the body inside a row.
const ROW_INNER_GAP: f32 = 5.;
/// Opacity of an unselected row: present but recessed behind the selection.
const UNSELECTED_ROW_OPACITY: f32 = 0.85;
/// Meta-line text size (location ref, timestamp).
const META_TEXT_SIZE: f32 = 11.;
/// Comment-body text size.
const BODY_TEXT_SIZE: f32 = 12.;
/// Body line height (1.5 × body size).
const BODY_LINE_HEIGHT: f32 = BODY_TEXT_SIZE * 1.5;
/// Where a just-selected comment's row lands: this far below the list's top
/// edge (see `apply_pending_comments_list_scroll`).
const LIST_TOP_INSET: f32 = 8.;

/// "11:49" for comments written today, "Jun 12" for older ones.
pub(crate) fn comment_timestamp_label(
    created_at: i64,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    use chrono::TimeZone;
    let Some(when) = chrono::Local.timestamp_opt(created_at, 0).single() else {
        return String::new();
    };
    if when.date_naive() == now.date_naive() {
        when.format("%H:%M").to_string()
    } else {
        when.format("%b %-d").to_string()
    }
}

/// The flat list's order: most recent first (`created_at` descending), ties
/// broken by insertion order in the review's `comments` vec, later-inserted
/// first (comments are pushed oldest-first, so a later push is the more
/// recent write). Anchor resolvability plays no part — recency alone
/// governs.
pub(crate) fn recency_ordered(mut comments: Vec<ReviewComment>) -> Vec<ReviewComment> {
    comments.reverse();
    comments.sort_by_key(|comment| std::cmp::Reverse(comment.created_at));
    comments
}

impl App {
    /// Render the Comments tab body (see module docs): one scrollable flat
    /// list of every saved comment, most recent first, with the staged
    /// draft's composer as the first row while one is open.
    pub(crate) fn render_comments_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let comments = recency_ordered(self.open_changeset_comments());

        if comments.is_empty() && self.comment_draft.is_none() {
            return self.render_comments_empty_state().into_any_element();
        }

        self.apply_pending_comments_list_scroll(&comments, cx);

        let mut list = div().flex().flex_col().w_full();
        if self.comment_draft.is_some() {
            list = list.child(self.render_comment_composer(cx));
        }
        for (index, comment) in comments.iter().enumerate() {
            let selected = self.selected_comment_id.as_deref() == Some(comment.id.as_str());
            list = list.child(self.render_comment_row(comment, index, selected, cx));
        }

        div()
            .id("comments-tab-body")
            .debug_selector(|| "comments-tab-body".to_string())
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_h_0()
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                if app.comments_list_hovered != *hovered {
                    app.comments_list_hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("comments-list-scroll")
                    .debug_selector(|| "comments-list-scroll".to_string())
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.comments_list_scroll)
                    .child(list),
            )
            .when(self.comments_list_hovered, |region| {
                region.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .debug_selector(|| "comments-list-scrollbar".to_string())
                        .child(
                            Scrollbar::vertical(&self.comments_list_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
            })
            .into_any_element()
    }

    /// The panel shown when the open changeset has no saved comments and no
    /// staged draft: a muted one-line message, centered in the tab.
    fn render_comments_empty_state(&self) -> impl IntoElement {
        div()
            .id("comments-empty-state")
            .debug_selector(|| "comments-empty-state".to_string())
            .flex_1()
            .w_full()
            .h_full()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(palette().text_muted)
            .child("No comments yet.")
    }

    /// Consumes `pending_comments_list_scroll` (set only by
    /// `select_comment_from_diff`, the anchored-diff-text click path):
    /// scrolls the list so the pending comment's row sits just below the
    /// list's top edge, clamped to the list's scroll range. When the row
    /// hasn't painted yet (its origin is unknown), the id stays pending and
    /// a re-render is requested so the scroll lands once the row paints. A
    /// pending id no longer in the list is dropped.
    fn apply_pending_comments_list_scroll(
        &self,
        comments: &[ReviewComment],
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.pending_comments_list_scroll.borrow().clone() else {
            return;
        };
        if !comments.iter().any(|comment| comment.id == target) {
            self.pending_comments_list_scroll.borrow_mut().take();
            return;
        }
        let Some(&row_y) = self.comment_row_origins.borrow().get(&target) else {
            // Not painted yet (e.g. the tab just opened). Retry next frame.
            cx.notify();
            return;
        };

        // `row_y` is window-absolute, captured at the last paint; removing
        // the container's own window position (tracked by the scroll handle)
        // and the offset the row was painted at leaves the row's stable
        // position within the unscrolled list content.
        let offset_y = f32::from(self.comments_list_scroll.offset().y);
        let container_top = f32::from(self.comments_list_scroll.bounds().origin.y);
        let content_row_y = row_y - container_top - offset_y;
        let max_offset = f32::from(self.comments_list_scroll.max_offset().height);
        let new_offset_y = (-(content_row_y - LIST_TOP_INSET)).clamp(-max_offset, 0.);
        self.comments_list_scroll
            .set_offset(point(px(0.), px(new_offset_y)));
        self.pending_comments_list_scroll.borrow_mut().take();
    }

    /// One saved-comment row in the flat list. The selected row is
    /// unmistakable within the list itself: accent background, a 2px accent
    /// left border, full opacity (unselected rows are recessed to
    /// `UNSELECTED_ROW_OPACITY`), and an accent-colored location label in
    /// its meta line.
    fn render_comment_row(
        &self,
        comment: &ReviewComment,
        index: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = comment.id.clone();
        let selector = format!("comment-row-{id}");

        let mut row = div()
            .id(("comment-row", index))
            .debug_selector(move || selector.clone())
            .relative()
            .flex()
            .flex_col()
            .gap(px(ROW_INNER_GAP))
            .px(px(12.))
            .py(px(8.))
            .cursor_pointer()
            .child(comment_row_meta(comment, selected))
            .child(comment_row_text(comment));

        if selected {
            row = row
                .bg(palette().accent_bg)
                .border_l_2()
                .border_color(palette().accent);
        } else {
            row = row
                .opacity(UNSELECTED_ROW_OPACITY)
                .hover(|el| el.bg(palette().ghost_element_hover));
        }

        // Captures this row's painted y-origin, keyed by comment id, for the
        // select-a-comment list scroll (see
        // `apply_pending_comments_list_scroll`).
        let origins = self.comment_row_origins.clone();
        let origin_id = id.clone();
        row = row.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, _, _| {
                    origins
                        .borrow_mut()
                        .insert(origin_id.clone(), f32::from(bounds.origin.y));
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );

        row.on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
            app.select_comment(&id, cx);
        }))
        .into_any_element()
    }

    /// The staged draft's composer, rendered as the first row of the flat
    /// list: styled like a selected row (accent background, accent left
    /// rail) but hosting the live `comment_input` instead of a saved body,
    /// plus a Cancel/Comment button row.
    ///
    /// Escape discards the draft. `InputEvent` (gpui-component 0.5) has no
    /// escape/cancel variant — pressing Escape in an `InputState` without
    /// `clean_on_escape` set (ours isn't) runs the crate's own `Escape`
    /// action handler, which does nothing and calls `cx.propagate()`. That
    /// lets the same `Escape` action (`gpui_component::input::Escape`) be
    /// caught here via `on_action` as it bubbles from the focused composer
    /// input up to this container.
    fn render_comment_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let draft = self
            .comment_draft
            .as_ref()
            .expect("composer row only exists while a draft is staged");
        let location = comment_anchors::comment_location_label(&draft.path, &draft.anchor);

        let meta = div()
            .flex()
            .items_center()
            .gap(px(ROW_INNER_GAP))
            .text_size(px(META_TEXT_SIZE))
            .child(
                div()
                    .font_family(MONO_FONT_FAMILY)
                    .text_color(palette().text_muted)
                    .child(location),
            )
            .child(div().flex_1())
            .child(div().text_color(palette().text_muted).child("New comment"));

        let field = div()
            .min_h(px(52.))
            .rounded(px(4.))
            .bg(palette().background)
            .border_1()
            .border_color(palette().accent)
            .px(px(8.))
            .py(px(6.))
            .child(
                Input::new(&self.comment_input)
                    .appearance(false)
                    .text_size(px(BODY_TEXT_SIZE)),
            );

        let buttons = div()
            .flex()
            .justify_end()
            .gap(px(6.))
            .child(self.render_composer_button(
                "comment-composer-cancel",
                "Cancel",
                false,
                |app, window, cx| app.cancel_comment_draft(window, cx),
                cx,
            ))
            .child(self.render_composer_button(
                "comment-composer-save",
                "Comment",
                true,
                |app, window, cx| app.save_comment_draft(window, cx),
                cx,
            ));

        div()
            .id("comment-composer")
            .debug_selector(|| "comment-composer".to_string())
            .relative()
            .flex()
            .flex_col()
            .gap(px(ROW_INNER_GAP))
            .px(px(12.))
            .py(px(8.))
            .bg(palette().accent_bg)
            .border_l_2()
            .border_color(palette().accent)
            .on_action(
                cx.listener(|app, _: &gpui_component::input::Escape, window, cx| {
                    app.cancel_comment_draft(window, cx);
                }),
            )
            .child(meta)
            .child(field)
            .child(buttons)
            .into_any_element()
    }

    /// A hand-rolled 22px-tall composer button (Cancel/Comment), matching the
    /// codebase's bordered/ghost button convention (see
    /// `guide_panel::render_guide_text_button`) rather than a themed widget.
    /// `primary` picks the accent-filled "Comment" style over the ghost
    /// "Cancel" style.
    fn render_composer_button(
        &self,
        selector: &'static str,
        label: &'static str,
        primary: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut button = div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .flex()
            .items_center()
            .justify_center()
            .h(px(22.))
            .px_2()
            .rounded(px(4.))
            .text_size(px(META_TEXT_SIZE))
            .cursor_pointer();

        button = if primary {
            button
                .bg(palette().accent)
                .text_color(palette().background)
                .font_weight(FontWeight::MEDIUM)
                .hover(|style| style.bg(palette().accent_bg_hover))
        } else {
            button
                .text_color(palette().text_muted)
                .hover(|style| style.bg(palette().ghost_element_hover))
        };

        button
            .on_click(cx.listener(move |app, _event: &ClickEvent, window, cx| {
                on_click(app, window, cx);
            }))
            .child(label)
    }
}

/// A comment row's meta line: the "{file basename}:{line ref}" location and
/// a right-aligned timestamp. The selected row's location renders in the
/// accent color; everything else stays muted.
fn comment_row_meta(comment: &ReviewComment, selected: bool) -> impl IntoElement {
    let location = comment_anchors::comment_location_label(&comment.path, &comment.anchor);
    let timestamp = comment_timestamp_label(comment.created_at, chrono::Local::now());
    let location_color = if selected {
        palette().accent
    } else {
        palette().text_muted
    };
    div()
        .flex()
        .items_center()
        .gap(px(ROW_INNER_GAP))
        .text_size(px(META_TEXT_SIZE))
        .font_family(MONO_FONT_FAMILY)
        .child(div().text_color(location_color).child(location))
        .child(div().flex_1())
        .child(div().text_color(palette().text_muted).child(timestamp))
}

/// A comment row's body text.
fn comment_row_text(comment: &ReviewComment) -> impl IntoElement {
    div()
        .text_size(px(BODY_TEXT_SIZE))
        .line_height(px(BODY_LINE_HEIGHT))
        .text_color(palette().text)
        .child(comment.body.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use gpui::{TestAppContext, VisualTestContext};

    #[test]
    fn timestamps_show_clock_time_same_day_and_date_otherwise() {
        use chrono::{Local, TimeZone};
        let now = Local.with_ymd_and_hms(2026, 7, 8, 12, 0, 0).unwrap();
        let today = Local
            .with_ymd_and_hms(2026, 7, 8, 11, 49, 0)
            .unwrap()
            .timestamp();
        let last_month = Local
            .with_ymd_and_hms(2026, 6, 12, 9, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(comment_timestamp_label(today, now), "11:49");
        assert_eq!(comment_timestamp_label(last_month, now), "Jun 12");
    }

    #[test]
    fn recency_orders_newest_first_and_breaks_ties_later_inserted_first() {
        let mut old = test_comment("a.rs", 1);
        old.created_at = 10;
        let mut newer = test_comment("b.rs", 2);
        newer.created_at = 20;
        // Two comments sharing a timestamp: the later-pushed one wins.
        let mut tie_first = test_comment("c.rs", 3);
        tie_first.created_at = 20;
        let mut tie_second = test_comment("d.rs", 4);
        tie_second.created_at = 20;

        let ordered = recency_ordered(vec![
            old.clone(),
            newer.clone(),
            tie_first.clone(),
            tie_second.clone(),
        ]);
        let ids: Vec<&str> = ordered.iter().map(|comment| comment.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                tie_second.id.as_str(),
                tie_first.id.as_str(),
                newer.id.as_str(),
                old.id.as_str()
            ],
            "created_at descending, equal timestamps later-inserted first"
        );
    }

    #[gpui::test]
    async fn flat_list_orders_by_recency_across_files_with_no_group_headers(
        cx: &mut TestAppContext,
    ) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let (oldest_id, middle_id, newest_id) = window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let id = app.current_review().expect("review").id.clone();
                // The newest comment lives on ANOTHER file — recency, not
                // the open file, governs the order.
                let mut oldest = test_comment(&path, 1);
                oldest.created_at = 10;
                let mut middle = test_comment(&path, 1);
                middle.created_at = 20;
                let mut newest = test_comment("some/other_file.py", 1);
                newest.created_at = 30;
                let ids = (oldest.id.clone(), middle.id.clone(), newest.id.clone());
                app.reviews
                    .mutate(&id, |review| {
                        review.comments.extend([newest, oldest, middle]);
                    })
                    .expect("mutate review");
                app.open_file_preview(path.clone(), cx);
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
                ids
            })
            .unwrap();
        cx.run_until_parked();

        let newest_bounds = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{newest_id}")))
            .expect("newest comment row renders");
        let middle_bounds = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{middle_id}")))
            .expect("middle comment row renders");
        let oldest_bounds = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{oldest_id}")))
            .expect("oldest comment row renders");

        assert!(
            newest_bounds.origin.y < middle_bounds.origin.y
                && middle_bounds.origin.y < oldest_bounds.origin.y,
            "rows order newest-first regardless of file"
        );

        // The flat list has no group headers.
        assert!(
            visual
                .debug_bounds(test_debug_selector(format!("comments-group-{path}")))
                .is_none(),
            "no group header renders for the open file"
        );
        assert!(
            visual
                .debug_bounds("comments-group-some/other_file.py")
                .is_none(),
            "no group header renders for the other file"
        );
    }

    #[gpui::test]
    async fn unresolvable_anchor_rows_render_and_order_by_recency(cx: &mut TestAppContext) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let (resolved_id, unresolved_id) = window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let id = app.current_review().expect("review").id.clone();
                // `hello.txt`'s new side only has line 1, so an anchor on
                // line 500 can never resolve against it. It is also the
                // NEWER comment: recency puts it first — unresolvable
                // anchors get no special ordering.
                let mut resolved = test_comment(&path, 1);
                resolved.created_at = 10;
                let mut unresolved = test_comment(&path, 500);
                unresolved.created_at = 20;
                let ids = (resolved.id.clone(), unresolved.id.clone());
                app.reviews
                    .mutate(&id, |review| {
                        review.comments.extend([unresolved, resolved]);
                    })
                    .expect("mutate review");
                app.open_file_preview(path.clone(), cx);
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
                ids
            })
            .unwrap();
        cx.run_until_parked();

        let unresolved_bounds = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{unresolved_id}")))
            .expect("unresolvable-anchor comment still renders a row");
        let resolved_bounds = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{resolved_id}")))
            .expect("resolvable comment row renders");
        assert!(
            unresolved_bounds.origin.y < resolved_bounds.origin.y,
            "the newer comment leads even though its anchor no longer resolves"
        );
    }

    #[gpui::test]
    async fn clicking_a_row_selects_opens_its_file_and_scrolls_the_diff(cx: &mut TestAppContext) {
        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        // Seed a comment deep in `long.txt` without opening any file, so the
        // row click has to open the file and land the diff on the anchor. It
        // spans lines 80–82, so the gutter marker must land on the START row
        // only (a marker wrongly painted on a later span row would be the
        // last one recorded and fail the containment check below).
        let comment_id = window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let mut comment = test_comment(&path, 80);
                comment.anchor.end_line = 82;
                let comment_id = comment.id.clone();
                app.reviews
                    .mutate(&review_id, |review| review.comments.push(comment))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
                comment_id
            })
            .expect("open long-diff changeset with one deep comment");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(gpui::size(px(900.), px(360.)));

        let row = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{comment_id}")))
            .expect("comment row renders");
        visual.simulate_click(row.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.selected_comment_id.as_deref(),
                    Some(comment_id.as_str()),
                    "row click selects the comment"
                );
                let pane = app.workspace.active_pane();
                assert_eq!(
                    app.workspace
                        .active_item(pane)
                        .map(|item| item.path().to_string()),
                    Some(path.clone()),
                    "row click opens the comment's file"
                );
            })
            .unwrap();

        let offset = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read new diff scroll offset");
        // Line 80 is 1-based; its row is 79, with 3 rows of context above.
        let expected = crate::app::diff_view::scroll_offset_for_block_top(79, 3);
        assert_eq!(
            offset.y, expected,
            "row click scrolls the diff to the comment's anchor"
        );

        // The selected comment's right-edge marker sits on the anchor's
        // START row (79), not on any later row of the 80–82 span, pinned at
        // the new-side pane's right edge.
        let marker = visual
            .debug_bounds("diff-anchor-marker")
            .expect("selecting from the sidebar paints the right-edge marker");
        let gutter = visual
            .debug_bounds("file-diff-line-new-79")
            .expect("row 79 gutter cell on the new side");
        let pane = visual
            .debug_bounds("file-diff-side-new")
            .expect("new-side pane bounds");
        assert!(
            marker.origin.y >= gutter.origin.y
                && marker.origin.y + marker.size.height <= gutter.origin.y + gutter.size.height,
            "the marker sits on the anchor's start row \
             (marker = {marker:?}, gutter = {gutter:?})"
        );
        assert!(
            marker.origin.x + marker.size.width <= pane.origin.x + pane.size.width
                && marker.origin.x >= pane.origin.x + pane.size.width - px(30.),
            "the marker is pinned at the pane's right edge \
             (marker = {marker:?}, pane = {pane:?})"
        );
    }

    #[gpui::test]
    async fn clicking_anchored_diff_text_scrolls_the_list_to_its_row(cx: &mut TestAppContext) {
        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        // The target comment anchors the whole first diff line but sits in
        // the MIDDLE of the recency order (created_at 50, between a newer
        // and an older batch), so its row starts below the fold with enough
        // rows underneath it that scrolling it to the top never hits the
        // list's end clamp.
        let target_id = window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.open_file_preview(path.clone(), cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let target = crate::reviews::ReviewComment {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: path.clone(),
                    anchor: crate::reviews::CommentAnchor {
                        side: crate::reviews::CommentSide::New,
                        start_line: 1,
                        start_col: 0,
                        end_line: 1,
                        end_col: 12,
                        quoted_text: String::new(),
                    },
                    body: "the clicked one".into(),
                    created_at: 50,
                };
                let target_id = target.id.clone();
                // Seven newer comments (rendered above the target) and seven
                // older ones (rendered below it).
                let others: Vec<_> = (11..=141)
                    .step_by(10)
                    .map(|line| {
                        let mut comment = test_comment(&path, line);
                        comment.created_at = if line <= 71 { 100 } else { 0 };
                        comment
                    })
                    .collect();
                app.reviews
                    .mutate(&review_id, |review| {
                        review.comments.push(target);
                        review.comments.extend(others);
                    })
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
                target_id
            })
            .expect("open long-diff changeset with many comments");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(gpui::size(px(900.), px(360.)));

        let list_bounds = visual
            .debug_bounds("comments-list-scroll")
            .expect("comments list container bounds");
        let row_before = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{target_id}")))
            .expect("target row renders (painted, below the fold)");
        assert!(
            row_before.origin.y > list_bounds.origin.y + list_bounds.size.height,
            "the target row starts below the list's fold"
        );

        // Click the anchored text in the diff: row 0's code cell (the anchor
        // spans the whole 12-character first line).
        let code = visual
            .debug_bounds("file-diff-code-new-0")
            .expect("first code row");
        visual.simulate_click(code.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.selected_comment_id.as_deref(),
                    Some(target_id.as_str()),
                    "clicking anchored diff text selects its comment"
                );
                assert!(
                    app.pending_comments_list_scroll.borrow().is_none(),
                    "the pending list scroll was consumed"
                );
            })
            .unwrap();

        let row_after = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{target_id}")))
            .expect("target row renders after the click");
        assert!(
            (row_after.origin.y - list_bounds.origin.y).abs() < px(LIST_TOP_INSET + 40.),
            "the list scrolled the selected row near its top \
             (row.y = {:?}, list top = {:?})",
            row_after.origin.y,
            list_bounds.origin.y
        );
    }

    #[gpui::test]
    async fn scrolling_the_diff_never_moves_the_comments_list(cx: &mut TestAppContext) {
        use gpui::{point, size, ScrollDelta, ScrollWheelEvent};

        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.open_file_preview(path.clone(), cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let comments: Vec<_> = (1..=150)
                    .step_by(10)
                    .map(|line| test_comment(&path, line))
                    .collect();
                app.reviews
                    .mutate(&review_id, |review| review.comments.extend(comments))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
            })
            .expect("open long-diff changeset with spread-out comments");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(360.)));

        let diff_bounds = visual
            .debug_bounds("file-diff-side-new")
            .expect("diff side debug bounds");
        let list_offset_before = window
            .read_with(cx, |app, _| app.comments_list_scroll.offset().y)
            .unwrap();

        visual.simulate_event(ScrollWheelEvent {
            position: diff_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-1200.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let diff_offset = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read diff scroll offset after wheel");
        assert!(
            diff_offset.y < px(0.),
            "the wheel scroll moved the diff down"
        );
        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.comments_list_scroll.offset().y,
                    list_offset_before,
                    "scrolling the diff must not move the comments list"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    async fn scrolling_the_list_never_moves_the_diff(cx: &mut TestAppContext) {
        use gpui::{point, size, ScrollDelta, ScrollWheelEvent};

        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.open_file_preview(path.clone(), cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let comments: Vec<_> = (1..=150)
                    .step_by(10)
                    .map(|line| test_comment(&path, line))
                    .collect();
                app.reviews
                    .mutate(&review_id, |review| review.comments.extend(comments))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
            })
            .expect("open long-diff changeset with spread-out comments");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(360.)));

        let list_bounds = visual
            .debug_bounds("comments-list-scroll")
            .expect("comments list container bounds");
        let diff_offset_before = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read diff scroll offset before the list scrolls");

        visual.simulate_event(ScrollWheelEvent {
            position: list_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
            ..Default::default()
        });
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert!(
                    app.comments_list_scroll.offset().y < px(0.),
                    "the wheel scroll moved the comments list"
                );
            })
            .unwrap();
        let diff_offset_after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read diff scroll offset after the list scrolls");
        assert_eq!(
            diff_offset_after, diff_offset_before,
            "scrolling the comments list must not move the diff"
        );
    }

    #[gpui::test]
    async fn clicking_a_sidebar_row_does_not_move_the_comments_list(cx: &mut TestAppContext) {
        use gpui::{point, size, ScrollDelta, ScrollWheelEvent};

        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        let comment_ids = window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.open_file_preview(path.clone(), cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let comments: Vec<_> = (1..=150)
                    .step_by(10)
                    .map(|line| test_comment(&path, line))
                    .collect();
                let ids: Vec<String> = comments.iter().map(|comment| comment.id.clone()).collect();
                app.reviews
                    .mutate(&review_id, |review| review.comments.extend(comments))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
                ids
            })
            .expect("open long-diff changeset with spread-out comments");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(360.)));

        // Put the list somewhere other than its resting position, so "the
        // click didn't move it" is distinguishable from "it never moved".
        let list_bounds = visual
            .debug_bounds("comments-list-scroll")
            .expect("comments list container bounds");
        visual.simulate_event(ScrollWheelEvent {
            position: list_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
            ..Default::default()
        });
        cx.run_until_parked();
        let scrolled_offset = window
            .read_with(cx, |app, _| app.comments_list_scroll.offset().y)
            .unwrap();
        assert!(
            scrolled_offset < px(0.),
            "the wheel scroll moved the list off its resting position"
        );

        // Click a row that is fully inside the list's viewport after the
        // scroll.
        let (visible_id, row_bounds) = comment_ids
            .iter()
            .find_map(|id| {
                let bounds =
                    visual.debug_bounds(test_debug_selector(format!("comment-row-{id}")))?;
                let fully_visible = bounds.origin.y > list_bounds.origin.y + px(20.)
                    && bounds.origin.y + bounds.size.height
                        < list_bounds.origin.y + list_bounds.size.height - px(20.);
                fully_visible.then(|| (id.clone(), bounds))
            })
            .expect("some comment row is fully visible after scrolling the list");
        visual.simulate_click(row_bounds.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.selected_comment_id.as_deref(),
                    Some(visible_id.as_str()),
                    "the row click selected its comment"
                );
                assert!(
                    app.pending_comments_list_scroll.borrow().is_none(),
                    "a sidebar row click never queues a list scroll"
                );
                assert_eq!(
                    app.comments_list_scroll.offset().y,
                    scrolled_offset,
                    "a sidebar row click must not move the comments list"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    async fn no_comments_shows_the_empty_state(cx: &mut TestAppContext) {
        let (_dir, _path, window, mut visual) = open_changeset_with_guide_panel(cx);
        window
            .update(cx, |app, _window, cx| {
                app.sidebar_tab = SidebarTab::Comments;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();

        visual
            .debug_bounds("comments-empty-state")
            .expect("empty state renders with no comments and no draft");
    }

    #[gpui::test]
    async fn the_draft_composer_renders_at_the_top_of_the_list(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let saved_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.ensure_open_changeset_review(Some(window), cx);
                    let review_id = app.current_review().expect("review").id.clone();
                    // A recent saved comment: even the newest saved row
                    // renders below the composer.
                    let mut saved = test_comment(&path, 1);
                    saved.created_at = i64::MAX;
                    let saved_id = saved.id.clone();
                    app.reviews
                        .mutate(&review_id, |review| review.comments.push(saved))
                        .expect("mutate review");
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(
                        pane,
                        &path,
                        diff_selection::DiffSelection {
                            side: repo::DiffSide::New,
                            anchor: diff_selection::DiffPoint { row: 0, column: 0 },
                            head: diff_selection::DiffPoint { row: 0, column: 3 },
                            goal_x: None,
                        },
                        cx,
                    );
                    app.stage_comment_draft(window, cx);
                    saved_id
                })
            })
            .unwrap();
        cx.run_until_parked();

        let composer = visual
            .debug_bounds("comment-composer")
            .expect("composer renders in the list");
        let saved_row = visual
            .debug_bounds(test_debug_selector(format!("comment-row-{saved_id}")))
            .expect("saved comment row renders");
        assert!(
            composer.origin.y < saved_row.origin.y,
            "the composer is the first row of the flat list"
        );
    }

    #[gpui::test]
    async fn the_composer_saves_on_comment_and_discards_on_cancel(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(
                        pane,
                        &path,
                        diff_selection::DiffSelection {
                            side: repo::DiffSide::New,
                            anchor: diff_selection::DiffPoint { row: 0, column: 0 },
                            head: diff_selection::DiffPoint { row: 0, column: 3 },
                            goal_x: None,
                        },
                        cx,
                    );
                    app.stage_comment_draft(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("comment-composer")
            .expect("composer renders in the list");

        // Cancel discards. `debug_bounds` never removes a selector once
        // painted (see `Frame::clear` and the analogous fix in
        // `status_footer`'s files-toggle test), so absence is asserted
        // against the draft state the composer's presence is gated on,
        // rather than against `debug_bounds("comment-composer")`.
        let cancel = visual.debug_bounds("comment-composer-cancel").unwrap();
        visual.simulate_click(cancel.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        app_entity.read_with(cx, |app, _| {
            assert!(
                app.comment_draft.is_none(),
                "cancel click cleared the draft state"
            );
            assert_eq!(app.comment_count(), 0, "cancel never saved a comment");
        });

        // Stage again, type, save.
        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(
                        pane,
                        &path,
                        diff_selection::DiffSelection {
                            side: repo::DiffSide::New,
                            anchor: diff_selection::DiffPoint { row: 0, column: 0 },
                            head: diff_selection::DiffPoint { row: 0, column: 3 },
                            goal_x: None,
                        },
                        cx,
                    );
                    app.stage_comment_draft(window, cx);
                    app.comment_input
                        .update(cx, |state, cx| state.set_value("Ship it", window, cx));
                });
            })
            .unwrap();
        cx.run_until_parked();
        let save = visual.debug_bounds("comment-composer-save").unwrap();
        visual.simulate_click(save.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        app_entity.read_with(cx, |app, _| {
            assert!(app.comment_draft.is_none(), "saving clears the draft");
            assert_eq!(app.comment_count(), 1);
            assert_eq!(app.open_changeset_comments()[0].body, "Ship it");
        });
    }

    #[gpui::test]
    async fn escape_discards_the_draft(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(
                        pane,
                        &path,
                        diff_selection::DiffSelection {
                            side: repo::DiffSide::New,
                            anchor: diff_selection::DiffPoint { row: 0, column: 0 },
                            head: diff_selection::DiffPoint { row: 0, column: 3 },
                            goal_x: None,
                        },
                        cx,
                    );
                    app.stage_comment_draft(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("comment-composer")
            .expect("composer renders before escape");

        visual.simulate_keystrokes("escape");
        cx.run_until_parked();

        // `debug_bounds` never removes a selector once painted (see
        // `Frame::clear`), so absence is asserted against the draft state
        // instead of `debug_bounds("comment-composer")`.
        app_entity.read_with(cx, |app, _| {
            assert!(app.comment_draft.is_none(), "escape discards the draft")
        });
    }
}
