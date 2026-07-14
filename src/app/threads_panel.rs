//! The Threads tab of the right-docked review sidebar: one flat,
//! conventionally scrolling list of the open changeset's saved threads,
//! most recently active first (`last_activity_at` descending, ties broken
//! later-inserted first — see `recency_ordered`). A reply bumps its thread
//! to the top, same as adding it did. The list is independent of which file
//! is open: no grouping, no expand/collapse. Each row's meta line names its
//! anchor as "{file basename}:{line ref}" with a right-aligned timestamp of
//! the thread's last activity, followed by every message in the thread,
//! oldest first, and a ghost Reply control. While a draft is staged, its
//! composer renders as the first row (the most-recent-in-progress item).
//!
//! Navigation is click-driven in both directions and each click moves only
//! the opposite surface: clicking a row selects the thread, opens its
//! file, and scrolls the diff to its anchor — never this list; clicking
//! anchored text in the diff selects the thread and scrolls this list to
//! bring its row near the top (see `apply_pending_threads_list_scroll`).
//! Scrolling the diff never moves the list, and scrolling the list never
//! moves the diff.

use super::*;

use crate::reviews::{MessageAuthor, ReviewThread, ReviewThreadKind, ThreadMessage};
use crate::theme::palette;
use gpui::{rems, SharedString};
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::ActiveTheme;

/// Gap between the meta line and the body inside a row.
const ROW_INNER_GAP: f32 = 5.;
/// Opacity of an unselected row: present but recessed behind the selection.
const UNSELECTED_ROW_OPACITY: f32 = 0.85;
/// Meta-line text size (location ref, timestamp).
const META_TEXT_SIZE: f32 = 11.;
/// Thread-body text size.
const BODY_TEXT_SIZE: f32 = 12.;
/// Body line height (1.5 × body size).
const BODY_LINE_HEIGHT: f32 = BODY_TEXT_SIZE * 1.5;
/// Where a just-selected thread's row lands: this far below the list's top
/// edge (see `apply_pending_threads_list_scroll`).
const LIST_TOP_INSET: f32 = 8.;
/// Height of the divider drawn between a thread's stacked messages.
const MESSAGE_DIVIDER_HEIGHT: f32 = 1.;

/// "11:49" for threads written today, "Jun 12" for older ones.
pub(crate) fn thread_timestamp_label(
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

/// The flat list's order: most recently active first (`last_activity_at`
/// descending — the latest of a thread's own `created_at` and its newest
/// message's `created_at`, so a reply bubbles its thread to the top), ties
/// broken by insertion order in the review's `threads` vec, later-inserted
/// first (threads are pushed oldest-first, so a later push is the more
/// recent write). Anchor resolvability plays no part — recency alone
/// governs.
pub(crate) fn recency_ordered(mut threads: Vec<ReviewThread>) -> Vec<ReviewThread> {
    threads.reverse();
    threads.sort_by_key(|thread| std::cmp::Reverse(thread.last_activity_at()));
    threads
}

impl App {
    /// Render the Threads tab body (see module docs): one scrollable flat
    /// list of every saved thread, most recently active first, with the
    /// staged draft's composer as the first row while one is open.
    pub(crate) fn render_threads_tab(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let threads = recency_ordered(self.open_changeset_threads());

        if threads.is_empty() && self.thread_draft.is_none() {
            return self.render_threads_empty_state().into_any_element();
        }

        self.apply_pending_threads_list_scroll(&threads, cx);

        let mut list = div().flex().flex_col().w_full();
        if self.thread_draft.is_some() {
            list = list.child(self.render_thread_composer(cx));
        }
        for (index, thread) in threads.iter().enumerate() {
            let selected = self.selected_thread_id.as_deref() == Some(thread.id.as_str());
            list = list.child(self.render_thread_row(thread, index, selected, window, cx));
        }

        div()
            .id("threads-tab-body")
            .debug_selector(|| "threads-tab-body".to_string())
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_h_0()
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                if app.threads_list_hovered != *hovered {
                    app.threads_list_hovered = *hovered;
                    cx.notify();
                }
            }))
            .when(!threads.is_empty(), |body| {
                body.child(self.render_threads_header(cx))
            })
            .child(
                div()
                    .id("threads-list-scroll")
                    .debug_selector(|| "threads-list-scroll".to_string())
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.threads_list_scroll)
                    .child(list),
            )
            .when(self.threads_list_hovered, |region| {
                region.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .debug_selector(|| "threads-list-scrollbar".to_string())
                        .child(
                            Scrollbar::vertical(&self.threads_list_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
            })
            .into_any_element()
    }

    /// The Threads tab's slim, pinned header: a collapse-all/expand-all
    /// control pair sitting above the scrollable list, mirroring the file
    /// tree's repo-root header. Only rendered by the caller while at least
    /// one saved thread exists — collapsing an empty (or draft-only) list
    /// has nothing to act on.
    fn render_threads_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("threads-tab-header")
            .debug_selector(|| "threads-tab-header".to_string())
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .w_full()
            .h(px(FILE_TREE_HEADER_HEIGHT))
            .gap(px(2.))
            .px(px(FILE_TREE_HEADER_INSET))
            .bg(palette().surface)
            .child(self.render_file_tree_icon_button(
                LucideIcon::ChevronsDownUp,
                "threads-collapse-all",
                "Collapse all threads",
                false,
                |app, _window, cx| app.set_all_threads_collapsed(true, cx),
                cx,
            ))
            .child(self.render_file_tree_icon_button(
                LucideIcon::ChevronsUpDown,
                "threads-expand-all",
                "Expand all threads",
                false,
                |app, _window, cx| app.set_all_threads_collapsed(false, cx),
                cx,
            ))
    }

    /// The panel shown when the open changeset has no saved threads and no
    /// staged draft: a muted one-line message, centered in the tab.
    fn render_threads_empty_state(&self) -> impl IntoElement {
        div()
            .id("threads-empty-state")
            .debug_selector(|| "threads-empty-state".to_string())
            .flex_1()
            .w_full()
            .h_full()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(palette().text_muted)
            .child("No threads yet.")
    }

    /// Consumes `pending_threads_list_scroll` (set only by
    /// `select_thread_from_diff`, the anchored-diff-text click path):
    /// scrolls the list so the pending thread's row sits just below the
    /// list's top edge, clamped to the list's scroll range. When the row
    /// hasn't painted yet (its origin is unknown), the id stays pending and
    /// a re-render is requested so the scroll lands once the row paints. A
    /// pending id no longer in the list is dropped.
    fn apply_pending_threads_list_scroll(&self, threads: &[ReviewThread], cx: &mut Context<Self>) {
        let Some(target) = self.pending_threads_list_scroll.borrow().clone() else {
            return;
        };
        if !threads.iter().any(|thread| thread.id == target) {
            self.pending_threads_list_scroll.borrow_mut().take();
            return;
        }
        let Some(&row_y) = self.thread_row_origins.borrow().get(&target) else {
            // Not painted yet (e.g. the tab just opened). Retry next frame.
            cx.notify();
            return;
        };

        // `row_y` is window-absolute, captured at the last paint; removing
        // the container's own window position (tracked by the scroll handle)
        // and the offset the row was painted at leaves the row's stable
        // position within the unscrolled list content.
        let offset_y = f32::from(self.threads_list_scroll.offset().y);
        let container_top = f32::from(self.threads_list_scroll.bounds().origin.y);
        let content_row_y = row_y - container_top - offset_y;
        let max_offset = f32::from(self.threads_list_scroll.max_offset().height);
        let new_offset_y = (-(content_row_y - LIST_TOP_INSET)).clamp(-max_offset, 0.);
        self.threads_list_scroll
            .set_offset(point(px(0.), px(new_offset_y)));
        self.pending_threads_list_scroll.borrow_mut().take();
    }

    /// One saved-thread row in the flat list. The selected row is
    /// unmistakable within the list itself: accent background, a 2px accent
    /// left border, full opacity (unselected rows are recessed to
    /// `UNSELECTED_ROW_OPACITY`), and an accent-colored location label in
    /// its meta line.
    fn render_thread_row(
        &self,
        thread: &ReviewThread,
        index: usize,
        selected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = thread.id.clone();
        let selector = format!("thread-row-{id}");
        let group_name = format!("thread-row-group-{id}");
        let collapsed = self.thread_row_collapsed(thread);

        let mut row = div()
            .id(("thread-row", index))
            .debug_selector(move || selector.clone())
            .group(group_name.clone())
            .relative()
            .flex()
            .flex_col()
            .gap(px(ROW_INNER_GAP))
            .px(px(12.))
            .py(px(8.))
            .cursor_pointer()
            .child(self.render_thread_row_header(thread, index, selected, collapsed, cx));

        if !collapsed {
            row = row.child(thread_row_messages(thread, window, cx));
            if thread.kind == ReviewThreadKind::Agent {
                row = row.child(self.render_agent_thread_footer(
                    thread,
                    index,
                    selected,
                    &group_name,
                    cx,
                ));
            } else {
                row = row.child(self.render_thread_reply_affordance(
                    thread,
                    index,
                    selected,
                    &group_name,
                    cx,
                ));
            }
        }

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

        // Captures this row's painted y-origin, keyed by thread id, for the
        // select-a-thread list scroll (see
        // `apply_pending_threads_list_scroll`).
        let origins = self.thread_row_origins.clone();
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
            app.select_thread(&id, cx);
        }))
        .into_any_element()
    }

    /// A thread row's header: a leading chevron that toggles collapse
    /// (`ChevronDown` expanded, `ChevronRight` collapsed), the
    /// "{file basename}:{line ref}" location, a message-count badge while
    /// collapsed (hidden while expanded, since the messages themselves are
    /// visible below), and a right-aligned timestamp of the thread's last
    /// activity. The selected row's location renders in the accent color;
    /// everything else stays muted.
    fn render_thread_row_header(
        &self,
        thread: &ReviewThread,
        index: usize,
        selected: bool,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let location = thread_anchors::thread_location_label(&thread.path, &thread.anchor);
        let timestamp = thread_timestamp_label(thread.last_activity_at(), chrono::Local::now());
        let location_color = if selected {
            palette().accent
        } else {
            palette().text_muted
        };

        let mut row = div()
            .flex()
            .items_center()
            .gap(px(ROW_INNER_GAP))
            .text_size(px(META_TEXT_SIZE))
            .font_family(MONO_FONT_FAMILY)
            .child(self.render_thread_collapse_toggle(thread, index, collapsed, cx))
            .child(div().text_color(location_color).child(location));

        if collapsed {
            let count = thread.messages.len();
            let label = if count == 1 {
                "1 message".to_string()
            } else {
                format!("{count} messages")
            };
            let count_selector = format!("thread-count-{}", thread.id);
            row = row.child(
                div()
                    .debug_selector(move || count_selector.clone())
                    .text_color(palette().text_muted)
                    .child(label),
            );
        }

        row.child(div().flex_1())
            .child(div().text_color(palette().text_muted).child(timestamp))
            .into_any_element()
    }

    /// The leading chevron on a thread row's header: toggles that thread's
    /// collapsed state without selecting or navigating (the click stops
    /// propagation before it reaches the row's own `on_click`).
    fn render_thread_collapse_toggle(
        &self,
        thread: &ReviewThread,
        index: usize,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = thread.id.clone();
        let selector = format!("thread-collapse-{id}");
        let icon = if collapsed {
            LucideIcon::ChevronRight
        } else {
            LucideIcon::ChevronDown
        };

        div()
            .id(("thread-collapse-toggle", index))
            .debug_selector(move || selector.clone())
            .flex()
            .items_center()
            .justify_center()
            .size(px(16.))
            .rounded(px(4.))
            .cursor_pointer()
            .hover(|style| style.bg(palette().ghost_element_hover))
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                app.toggle_thread_collapsed(&id, cx);
            }))
            .child(
                Icon::new(icon)
                    .text_color(palette().icon_muted)
                    .size(px(12.)),
            )
            .into_any_element()
    }

    /// The bottom of a thread row: either the inline reply composer (while
    /// `reply_draft_thread_id` names this thread) or a ghost "Reply" text
    /// button, shown for the selected row and revealed on hover for any
    /// other row (`group_hover` keyed to this row's own group, since rows
    /// have no per-row hover state of their own).
    fn render_thread_reply_affordance(
        &self,
        thread: &ReviewThread,
        index: usize,
        selected: bool,
        group_name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.reply_draft_thread_id.as_deref() == Some(thread.id.as_str()) {
            return self.render_thread_reply_composer(cx).into_any_element();
        }

        let id = thread.id.clone();
        let selector = format!("thread-reply-{id}");
        let mut button = div()
            .id(("thread-reply", index))
            .debug_selector(move || selector.clone())
            .flex()
            .items_center()
            .text_size(px(META_TEXT_SIZE))
            .text_color(palette().text_muted)
            .cursor_pointer()
            .hover(|style| style.text_color(palette().text))
            .child("Reply");

        if selected {
            button = button.opacity(1.);
        } else {
            button = button
                .opacity(0.)
                .group_hover(group_name.to_string(), |el| el.opacity(1.));
        }

        button
            .on_click(cx.listener(move |app, _event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                app.start_thread_reply(&id, window, cx);
            }))
            .into_any_element()
    }

    /// The footer of an Agent thread row (below its messages): the standing
    /// AI disclaimer once the thread carries a reply, then whichever of the
    /// running ticker, the error+Retry, or the reply affordance applies.
    /// With AI assistance off the reply affordance is withheld, so the thread
    /// renders as read-only history.
    fn render_agent_thread_footer(
        &self,
        thread: &ReviewThread,
        index: usize,
        selected: bool,
        group_name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut column = div().flex().flex_col().gap(px(ROW_INNER_GAP)).w_full();

        if thread
            .messages
            .iter()
            .any(|message| message.author == MessageAuthor::Agent)
        {
            let selector = format!("thread-agent-disclaimer-{}", thread.id);
            column = column.child(
                div()
                    .debug_selector(move || selector.clone())
                    .text_size(px(META_TEXT_SIZE))
                    .text_color(palette().text_muted)
                    .child("AI-generated — verify against the diff"),
            );
        }

        if let Some(activity) = self.agent_thread_run_activity(&thread.id, cx) {
            column =
                column.child(self.render_agent_running_ticker(&thread.id, index, activity, cx));
        } else if let Some(error) = self.agent_thread_errors.get(&thread.id).cloned() {
            column = column.child(self.render_agent_error(&thread.id, index, error, cx));
        } else if self.settings.ai_enabled {
            column = column.child(
                self.render_thread_reply_affordance(thread, index, selected, group_name, cx),
            );
        }

        column.into_any_element()
    }

    /// The running ticker for an in-flight agent turn: "Running {tool}…" (or
    /// "Working…" before the first tool call) with a Cancel control, reusing
    /// the guide panel's ticker convention.
    fn render_agent_running_ticker(
        &self,
        thread_id: &str,
        index: usize,
        activity: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ticker_text = match activity {
            Some(name) => format!("Running {name}…"),
            None => "Working…".to_string(),
        };
        let selector = format!("thread-agent-running-{thread_id}");
        let cancel_selector = format!("thread-agent-cancel-{thread_id}");
        let id = thread_id.to_string();

        div()
            .debug_selector(move || selector.clone())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(ROW_INNER_GAP))
            .text_size(px(META_TEXT_SIZE))
            .child(div().text_color(palette().text_muted).child(ticker_text))
            .child(
                div()
                    .id(("thread-agent-cancel", index))
                    .debug_selector(move || cancel_selector.clone())
                    .flex()
                    .items_center()
                    .text_color(palette().accent)
                    .cursor_pointer()
                    .hover(|style| style.text_color(palette().text))
                    .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        app.cancel_agent_thread(&id, cx);
                    }))
                    .child("Cancel"),
            )
            .into_any_element()
    }

    /// The error state for an agent turn that failed (or couldn't start): the
    /// message with a Retry control that re-runs the turn.
    fn render_agent_error(
        &self,
        thread_id: &str,
        index: usize,
        error: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selector = format!("thread-agent-error-{thread_id}");
        let retry_selector = format!("thread-agent-retry-{thread_id}");
        let id = thread_id.to_string();

        div()
            .debug_selector(move || selector.clone())
            .flex()
            .flex_col()
            .gap(px(ROW_INNER_GAP))
            .text_size(px(META_TEXT_SIZE))
            .child(div().text_color(palette().text_muted).child(error))
            .child(
                div()
                    .id(("thread-agent-retry", index))
                    .debug_selector(move || retry_selector.clone())
                    .flex()
                    .items_center()
                    .text_color(palette().accent)
                    .cursor_pointer()
                    .hover(|style| style.text_color(palette().text))
                    .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        app.retry_agent_thread(&id, cx);
                    }))
                    .child("Retry"),
            )
            .into_any_element()
    }

    /// The inline reply composer, rendered in place of the Reply button
    /// while `reply_draft_thread_id` names this thread. Mirrors
    /// `render_thread_composer`'s Input/Cancel/Save wiring.
    fn render_thread_reply_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let field = div()
            .min_h(px(40.))
            .rounded(px(4.))
            .bg(palette().background)
            .border_1()
            .border_color(palette().accent)
            .px(px(8.))
            .py(px(6.))
            .child(
                Input::new(&self.reply_input)
                    .appearance(false)
                    .text_size(px(BODY_TEXT_SIZE)),
            );

        let buttons = div()
            .flex()
            .justify_end()
            .gap(px(6.))
            .child(self.render_composer_button(
                "thread-reply-cancel",
                "Cancel",
                false,
                |app, window, cx| app.cancel_thread_reply(window, cx),
                cx,
            ))
            .child(self.render_composer_button(
                "thread-reply-save",
                "Reply",
                true,
                |app, window, cx| app.save_thread_reply(window, cx),
                cx,
            ));

        div()
            .id("thread-reply-composer")
            .debug_selector(|| "thread-reply-composer".to_string())
            .flex()
            .flex_col()
            .gap(px(ROW_INNER_GAP))
            .pt(px(4.))
            .on_click(|_event: &ClickEvent, _window, cx| cx.stop_propagation())
            .on_action(
                cx.listener(|app, _: &gpui_component::input::Escape, window, cx| {
                    app.cancel_thread_reply(window, cx);
                }),
            )
            .child(field)
            .child(buttons)
            .into_any_element()
    }

    /// The staged draft's composer, rendered as the first row of the flat
    /// list: styled like a selected row (accent background, accent left
    /// rail) but hosting the live `thread_input` instead of a saved body,
    /// plus a Cancel/Comment button row.
    ///
    /// Escape discards the draft. `InputEvent` (gpui-component 0.5) has no
    /// escape/cancel variant — pressing Escape in an `InputState` without
    /// `clean_on_escape` set (ours isn't) runs the crate's own `Escape`
    /// action handler, which does nothing and calls `cx.propagate()`. That
    /// lets the same `Escape` action (`gpui_component::input::Escape`) be
    /// caught here via `on_action` as it bubbles from the focused composer
    /// input up to this container.
    fn render_thread_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let draft = self
            .thread_draft
            .as_ref()
            .expect("composer row only exists while a draft is staged");
        let location = thread_anchors::thread_location_label(&draft.path, &draft.anchor);
        let is_agent = draft.kind == ReviewThreadKind::Agent;
        let meta_label = if is_agent { "Ask AI" } else { "New thread" };
        let save_label = if is_agent { "Ask" } else { "Comment" };

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
            .child(div().text_color(palette().text_muted).child(meta_label));

        let field = div()
            .min_h(px(52.))
            .rounded(px(4.))
            .bg(palette().background)
            .border_1()
            .border_color(palette().accent)
            .px(px(8.))
            .py(px(6.))
            .child(
                Input::new(&self.thread_input)
                    .appearance(false)
                    .text_size(px(BODY_TEXT_SIZE)),
            );

        let buttons = div()
            .flex()
            .justify_end()
            .gap(px(6.))
            .child(self.render_composer_button(
                "thread-composer-cancel",
                "Cancel",
                false,
                |app, window, cx| app.cancel_thread_draft(window, cx),
                cx,
            ))
            .child(self.render_composer_button(
                "thread-composer-save",
                save_label,
                true,
                |app, window, cx| app.submit_thread_draft(window, cx),
                cx,
            ));

        div()
            .id("thread-composer")
            .debug_selector(|| "thread-composer".to_string())
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
                    app.cancel_thread_draft(window, cx);
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

/// A thread row's stacked messages, oldest first, separated by a subtle
/// divider between consecutive messages.
fn thread_row_messages(
    thread: &ReviewThread,
    window: &mut Window,
    cx: &mut Context<App>,
) -> impl IntoElement {
    let mut container = div().flex().flex_col().gap(px(ROW_INNER_GAP)).w_full();
    for (index, message) in thread.messages.iter().enumerate() {
        if index > 0 {
            container = container.child(
                div()
                    .h(px(MESSAGE_DIVIDER_HEIGHT))
                    .w_full()
                    .bg(palette().border),
            );
        }
        container = container.child(thread_message_block(thread, index, message, window, cx));
    }
    container
}

/// One message block: an "AI" tag above the body when the message is
/// Agent-authored (a Reviewer message never gets an author tag — on the
/// first message it would just be visual noise, and there is only one
/// reviewer per thread today). An Agent-authored body renders as formatted
/// markdown (headings, lists, inline/fenced code with syntax highlighting);
/// a Reviewer body always stays plain text.
fn thread_message_block(
    thread: &ReviewThread,
    index: usize,
    message: &ThreadMessage,
    window: &mut Window,
    cx: &mut Context<App>,
) -> impl IntoElement {
    let selector = format!("thread-message-{}-{}", thread.id, index);
    let mut block = div()
        .id(("thread-message", index))
        .debug_selector(move || selector.clone())
        .flex()
        .flex_col()
        .gap(px(2.));
    if message.author == MessageAuthor::Agent {
        let tag_selector = format!("thread-message-{}-{}-agent-tag", thread.id, index);
        block = block.child(
            div()
                .debug_selector(move || tag_selector.clone())
                .text_size(px(META_TEXT_SIZE - 1.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette().accent)
                .child("AI"),
        );
    }

    let body: AnyElement = if message.author == MessageAuthor::Agent {
        render_agent_message_markdown(thread, index, message, window, cx)
    } else {
        div()
            .text_size(px(BODY_TEXT_SIZE))
            .line_height(px(BODY_LINE_HEIGHT))
            .text_color(palette().text)
            .child(message.body.clone())
            .into_any_element()
    };
    block.child(body)
}

/// An Agent-authored message body, rendered as markdown via
/// `gpui_component::text::TextView` — headings, lists, and inline/fenced
/// code with syntax highlighting from the active `highlight_theme`. Parsing
/// happens on a background task, so the first frame can render empty; the
/// element id is keyed by this thread's id and the message's index so two
/// messages never share (and corrupt) each other's parse state.
fn render_agent_message_markdown(
    thread: &ReviewThread,
    index: usize,
    message: &ThreadMessage,
    window: &mut Window,
    cx: &mut Context<App>,
) -> AnyElement {
    let selector = format!("thread-message-md-{}-{}", thread.id, index);
    let element_id = SharedString::from(format!("thread-message-md-{}-{}", thread.id, index));
    let theme = cx.theme();
    let style = TextViewStyle {
        // Tighter than the crate default (1 rem) so paragraphs sit close
        // together in the narrow sidebar, matching the plain-text body's
        // line spacing.
        paragraph_gap: rems(0.4),
        highlight_theme: theme.highlight_theme.clone(),
        is_dark: theme.mode.is_dark(),
        ..Default::default()
    };
    let markdown = TextView::markdown(element_id, message.body.clone(), window, cx)
        .style(style)
        .selectable(true)
        .text_size(px(BODY_TEXT_SIZE))
        .line_height(px(BODY_LINE_HEIGHT))
        .text_color(palette().text);

    div()
        .debug_selector(move || selector.clone())
        .w_full()
        .child(markdown)
        .into_any_element()
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
        assert_eq!(thread_timestamp_label(today, now), "11:49");
        assert_eq!(thread_timestamp_label(last_month, now), "Jun 12");
    }

    #[test]
    fn recency_orders_newest_first_and_breaks_ties_later_inserted_first() {
        let mut old = test_thread("a.rs", 1);
        old.created_at = 10;
        old.messages[0].created_at = 10;
        let mut newer = test_thread("b.rs", 2);
        newer.created_at = 20;
        newer.messages[0].created_at = 20;
        // Two threads sharing a timestamp: the later-pushed one wins.
        let mut tie_first = test_thread("c.rs", 3);
        tie_first.created_at = 20;
        tie_first.messages[0].created_at = 20;
        let mut tie_second = test_thread("d.rs", 4);
        tie_second.created_at = 20;
        tie_second.messages[0].created_at = 20;

        let ordered = recency_ordered(vec![
            old.clone(),
            newer.clone(),
            tie_first.clone(),
            tie_second.clone(),
        ]);
        let ids: Vec<&str> = ordered.iter().map(|thread| thread.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                tie_second.id.as_str(),
                tie_first.id.as_str(),
                newer.id.as_str(),
                old.id.as_str()
            ],
            "last activity descending, equal timestamps later-inserted first"
        );
    }

    #[test]
    fn recency_orders_by_last_activity_so_a_reply_outranks_a_newer_thread() {
        // An old thread that just received a reply must sort ahead of a
        // thread created later but never replied to — the sort key is
        // `last_activity_at`, not `created_at`.
        let mut replied = test_thread("a.rs", 1);
        replied.created_at = 10;
        replied.messages[0].created_at = 10;
        replied.messages.push(crate::reviews::ThreadMessage {
            id: "reply".into(),
            author: crate::reviews::MessageAuthor::Reviewer,
            body: "late reply".into(),
            created_at: 30,
        });
        let mut newer_unreplied = test_thread("b.rs", 2);
        newer_unreplied.created_at = 20;
        newer_unreplied.messages[0].created_at = 20;

        let ordered = recency_ordered(vec![replied.clone(), newer_unreplied.clone()]);
        let ids: Vec<&str> = ordered.iter().map(|thread| thread.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![replied.id.as_str(), newer_unreplied.id.as_str()],
            "the replied-to thread leads despite its older created_at"
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
                // The newest thread lives on ANOTHER file — recency, not
                // the open file, governs the order.
                let mut oldest = test_thread(&path, 1);
                oldest.created_at = 10;
                let mut middle = test_thread(&path, 1);
                middle.created_at = 20;
                let mut newest = test_thread("some/other_file.py", 1);
                newest.created_at = 30;
                let ids = (oldest.id.clone(), middle.id.clone(), newest.id.clone());
                app.reviews
                    .mutate(&id, |review| {
                        review.threads.extend([newest, oldest, middle]);
                    })
                    .expect("mutate review");
                app.open_file_preview(path.clone(), cx);
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                ids
            })
            .unwrap();
        cx.run_until_parked();

        let newest_bounds = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{newest_id}")))
            .expect("newest thread row renders");
        let middle_bounds = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{middle_id}")))
            .expect("middle thread row renders");
        let oldest_bounds = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{oldest_id}")))
            .expect("oldest thread row renders");

        assert!(
            newest_bounds.origin.y < middle_bounds.origin.y
                && middle_bounds.origin.y < oldest_bounds.origin.y,
            "rows order newest-first regardless of file"
        );

        // The flat list has no group headers.
        assert!(
            visual
                .debug_bounds(test_debug_selector(format!("threads-group-{path}")))
                .is_none(),
            "no group header renders for the open file"
        );
        assert!(
            visual
                .debug_bounds("threads-group-some/other_file.py")
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
                // NEWER thread: recency puts it first — unresolvable
                // anchors get no special ordering.
                let mut resolved = test_thread(&path, 1);
                resolved.created_at = 10;
                let mut unresolved = test_thread(&path, 500);
                unresolved.created_at = 20;
                let ids = (resolved.id.clone(), unresolved.id.clone());
                app.reviews
                    .mutate(&id, |review| {
                        review.threads.extend([unresolved, resolved]);
                    })
                    .expect("mutate review");
                app.open_file_preview(path.clone(), cx);
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                ids
            })
            .unwrap();
        cx.run_until_parked();

        let unresolved_bounds = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{unresolved_id}")))
            .expect("unresolvable-anchor thread still renders a row");
        let resolved_bounds = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{resolved_id}")))
            .expect("resolvable thread row renders");
        assert!(
            unresolved_bounds.origin.y < resolved_bounds.origin.y,
            "the newer thread leads even though its anchor no longer resolves"
        );
    }

    #[gpui::test]
    async fn clicking_a_row_selects_opens_its_file_and_scrolls_the_diff(cx: &mut TestAppContext) {
        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        // Seed a thread deep in `long.txt` without opening any file, so the
        // row click has to open the file and land the diff on the anchor. It
        // spans lines 80–82, so the gutter marker must land on the START row
        // only (a marker wrongly painted on a later span row would be the
        // last one recorded and fail the containment check below).
        let thread_id = window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let mut thread = test_thread(&path, 80);
                thread.anchor.end_line = 82;
                let thread_id = thread.id.clone();
                app.reviews
                    .mutate(&review_id, |review| review.threads.push(thread))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                thread_id
            })
            .expect("open long-diff changeset with one deep thread");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(gpui::size(px(900.), px(360.)));

        let row = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{thread_id}")))
            .expect("thread row renders");
        visual.simulate_click(row.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.selected_thread_id.as_deref(),
                    Some(thread_id.as_str()),
                    "row click selects the thread"
                );
                let pane = app.workspace.active_pane();
                assert_eq!(
                    app.workspace
                        .active_item(pane)
                        .map(|item| item.path().to_string()),
                    Some(path.clone()),
                    "row click opens the thread's file"
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
            "row click scrolls the diff to the thread's anchor"
        );

        // The selected thread's right-edge marker sits on the anchor's
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

        // The target thread anchors the whole first diff line but sits in
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
                let target = crate::reviews::ReviewThread {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: path.clone(),
                    anchor: crate::reviews::ThreadAnchor {
                        side: crate::reviews::ThreadSide::New,
                        start_line: 1,
                        start_col: 0,
                        end_line: 1,
                        end_col: 12,
                        quoted_text: String::new(),
                    },
                    messages: vec![crate::reviews::ThreadMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        author: crate::reviews::MessageAuthor::Reviewer,
                        body: "the clicked one".into(),
                        created_at: 50,
                    }],
                    created_at: 50,
                    ..Default::default()
                };
                let target_id = target.id.clone();
                // Seven newer threads (rendered above the target) and seven
                // older ones (rendered below it).
                let others: Vec<_> = (11..=141)
                    .step_by(10)
                    .map(|line| {
                        let mut thread = test_thread(&path, line);
                        thread.created_at = if line <= 71 { 100 } else { 0 };
                        thread
                    })
                    .collect();
                app.reviews
                    .mutate(&review_id, |review| {
                        review.threads.push(target);
                        review.threads.extend(others);
                    })
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                target_id
            })
            .expect("open long-diff changeset with many threads");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(gpui::size(px(900.), px(360.)));

        let list_bounds = visual
            .debug_bounds("threads-list-scroll")
            .expect("threads list container bounds");
        let row_before = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{target_id}")))
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
                    app.selected_thread_id.as_deref(),
                    Some(target_id.as_str()),
                    "clicking anchored diff text selects its thread"
                );
                assert!(
                    app.pending_threads_list_scroll.borrow().is_none(),
                    "the pending list scroll was consumed"
                );
            })
            .unwrap();

        let row_after = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{target_id}")))
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
    async fn scrolling_the_diff_never_moves_the_threads_list(cx: &mut TestAppContext) {
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
                let threads: Vec<_> = (1..=150)
                    .step_by(10)
                    .map(|line| test_thread(&path, line))
                    .collect();
                app.reviews
                    .mutate(&review_id, |review| review.threads.extend(threads))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
            })
            .expect("open long-diff changeset with spread-out threads");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(360.)));

        let diff_bounds = visual
            .debug_bounds("file-diff-side-new")
            .expect("diff side debug bounds");
        let list_offset_before = window
            .read_with(cx, |app, _| app.threads_list_scroll.offset().y)
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
                    app.threads_list_scroll.offset().y,
                    list_offset_before,
                    "scrolling the diff must not move the threads list"
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
                let threads: Vec<_> = (1..=150)
                    .step_by(10)
                    .map(|line| test_thread(&path, line))
                    .collect();
                app.reviews
                    .mutate(&review_id, |review| review.threads.extend(threads))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
            })
            .expect("open long-diff changeset with spread-out threads");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(360.)));

        let list_bounds = visual
            .debug_bounds("threads-list-scroll")
            .expect("threads list container bounds");
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
                    app.threads_list_scroll.offset().y < px(0.),
                    "the wheel scroll moved the threads list"
                );
            })
            .unwrap();
        let diff_offset_after = window
            .read_with(cx, |app, cx| app.file_diff_new_scroll_offset(cx))
            .expect("read diff scroll offset after the list scrolls");
        assert_eq!(
            diff_offset_after, diff_offset_before,
            "scrolling the threads list must not move the diff"
        );
    }

    #[gpui::test]
    async fn clicking_a_sidebar_row_does_not_move_the_threads_list(cx: &mut TestAppContext) {
        use gpui::{point, size, ScrollDelta, ScrollWheelEvent};

        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        let thread_ids = window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.open_file_preview(path.clone(), cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let threads: Vec<_> = (1..=150)
                    .step_by(10)
                    .map(|line| test_thread(&path, line))
                    .collect();
                let ids: Vec<String> = threads.iter().map(|thread| thread.id.clone()).collect();
                app.reviews
                    .mutate(&review_id, |review| review.threads.extend(threads))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                ids
            })
            .expect("open long-diff changeset with spread-out threads");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(360.)));

        // Put the list somewhere other than its resting position, so "the
        // click didn't move it" is distinguishable from "it never moved".
        let list_bounds = visual
            .debug_bounds("threads-list-scroll")
            .expect("threads list container bounds");
        visual.simulate_event(ScrollWheelEvent {
            position: list_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
            ..Default::default()
        });
        cx.run_until_parked();
        let scrolled_offset = window
            .read_with(cx, |app, _| app.threads_list_scroll.offset().y)
            .unwrap();
        assert!(
            scrolled_offset < px(0.),
            "the wheel scroll moved the list off its resting position"
        );

        // Click a row that is fully inside the list's viewport after the
        // scroll.
        let (visible_id, row_bounds) = thread_ids
            .iter()
            .find_map(|id| {
                let bounds =
                    visual.debug_bounds(test_debug_selector(format!("thread-row-{id}")))?;
                let fully_visible = bounds.origin.y > list_bounds.origin.y + px(20.)
                    && bounds.origin.y + bounds.size.height
                        < list_bounds.origin.y + list_bounds.size.height - px(20.);
                fully_visible.then(|| (id.clone(), bounds))
            })
            .expect("some thread row is fully visible after scrolling the list");
        visual.simulate_click(row_bounds.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.selected_thread_id.as_deref(),
                    Some(visible_id.as_str()),
                    "the row click selected its thread"
                );
                assert!(
                    app.pending_threads_list_scroll.borrow().is_none(),
                    "a sidebar row click never queues a list scroll"
                );
                assert_eq!(
                    app.threads_list_scroll.offset().y,
                    scrolled_offset,
                    "a sidebar row click must not move the threads list"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    async fn no_threads_shows_the_empty_state(cx: &mut TestAppContext) {
        let (_dir, _path, window, mut visual) = open_changeset_with_guide_panel(cx);
        window
            .update(cx, |app, _window, cx| {
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();

        visual
            .debug_bounds("threads-empty-state")
            .expect("empty state renders with no threads and no draft");
    }

    #[gpui::test]
    async fn the_draft_composer_renders_at_the_top_of_the_list(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let saved_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.ensure_open_changeset_review(Some(window), cx);
                    let review_id = app.current_review().expect("review").id.clone();
                    // A recent saved thread: even the newest saved row
                    // renders below the composer.
                    let mut saved = test_thread(&path, 1);
                    saved.created_at = i64::MAX;
                    let saved_id = saved.id.clone();
                    app.reviews
                        .mutate(&review_id, |review| review.threads.push(saved))
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
                    app.stage_thread_draft(window, cx);
                    saved_id
                })
            })
            .unwrap();
        cx.run_until_parked();

        let composer = visual
            .debug_bounds("thread-composer")
            .expect("composer renders in the list");
        let saved_row = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{saved_id}")))
            .expect("saved thread row renders");
        assert!(
            composer.origin.y < saved_row.origin.y,
            "the composer is the first row of the flat list"
        );
    }

    #[gpui::test]
    async fn the_composer_saves_on_thread_and_discards_on_cancel(cx: &mut TestAppContext) {
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
                    app.stage_thread_draft(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("thread-composer")
            .expect("composer renders in the list");

        // Cancel discards. `debug_bounds` never removes a selector once
        // painted (see `Frame::clear` and the analogous fix in
        // `status_footer`'s files-toggle test), so absence is asserted
        // against the draft state the composer's presence is gated on,
        // rather than against `debug_bounds("thread-composer")`.
        let cancel = visual.debug_bounds("thread-composer-cancel").unwrap();
        visual.simulate_click(cancel.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        app_entity.read_with(cx, |app, _| {
            assert!(
                app.thread_draft.is_none(),
                "cancel click cleared the draft state"
            );
            assert_eq!(app.thread_count(), 0, "cancel never saved a thread");
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
                    app.stage_thread_draft(window, cx);
                    app.thread_input
                        .update(cx, |state, cx| state.set_value("Ship it", window, cx));
                });
            })
            .unwrap();
        cx.run_until_parked();
        let save = visual.debug_bounds("thread-composer-save").unwrap();
        visual.simulate_click(save.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        app_entity.read_with(cx, |app, _| {
            assert!(app.thread_draft.is_none(), "saving clears the draft");
            assert_eq!(app.thread_count(), 1);
            assert_eq!(app.open_changeset_threads()[0].messages[0].body, "Ship it");
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
                    app.stage_thread_draft(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("thread-composer")
            .expect("composer renders before escape");

        visual.simulate_keystrokes("escape");
        cx.run_until_parked();

        // `debug_bounds` never removes a selector once painted (see
        // `Frame::clear`), so absence is asserted against the draft state
        // instead of `debug_bounds("thread-composer")`.
        app_entity.read_with(cx, |app, _| {
            assert!(app.thread_draft.is_none(), "escape discards the draft")
        });
    }

    #[gpui::test]
    async fn a_thread_renders_all_its_messages_in_order(cx: &mut TestAppContext) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let thread_id = window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let mut thread = test_thread(&path, 1);
                let thread_id = thread.id.clone();
                thread.messages = vec![
                    crate::reviews::ThreadMessage {
                        id: "m1".into(),
                        author: crate::reviews::MessageAuthor::Reviewer,
                        body: "first".into(),
                        created_at: 1,
                    },
                    crate::reviews::ThreadMessage {
                        id: "m2".into(),
                        author: crate::reviews::MessageAuthor::Agent,
                        body: "second".into(),
                        created_at: 2,
                    },
                    crate::reviews::ThreadMessage {
                        id: "m3".into(),
                        author: crate::reviews::MessageAuthor::Reviewer,
                        body: "third".into(),
                        created_at: 3,
                    },
                ];
                app.reviews
                    .mutate(&review_id, |review| review.threads.push(thread))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                thread_id
            })
            .unwrap();
        cx.run_until_parked();

        let first = visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-0")))
            .expect("first message renders");
        let second = visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-1")))
            .expect("second message renders");
        let third = visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-2")))
            .expect("third message renders");
        assert!(
            first.origin.y < second.origin.y && second.origin.y < third.origin.y,
            "messages render top to bottom, oldest first"
        );

        // Only the Agent-authored message (index 1) gets an "AI" tag; the
        // Reviewer messages, including the first, get none.
        visual
            .debug_bounds(test_debug_selector(format!(
                "thread-message-{thread_id}-1-agent-tag"
            )))
            .expect("the Agent message shows an AI tag");
        assert!(
            visual
                .debug_bounds(test_debug_selector(format!(
                    "thread-message-{thread_id}-0-agent-tag"
                )))
                .is_none(),
            "the first Reviewer message never renders an AI tag"
        );
        assert!(
            visual
                .debug_bounds(test_debug_selector(format!(
                    "thread-message-{thread_id}-2-agent-tag"
                )))
                .is_none(),
            "a later Reviewer message never renders an AI tag either"
        );
    }

    #[gpui::test]
    async fn agent_messages_render_as_markdown_and_reviewer_messages_stay_plain(
        cx: &mut TestAppContext,
    ) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let thread_id = window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let mut thread = test_thread(&path, 1);
                let thread_id = thread.id.clone();
                thread.messages = vec![
                    crate::reviews::ThreadMessage {
                        id: "m1".into(),
                        author: crate::reviews::MessageAuthor::Reviewer,
                        body: "plain **not markdown**".into(),
                        created_at: 1,
                    },
                    crate::reviews::ThreadMessage {
                        id: "m2".into(),
                        author: crate::reviews::MessageAuthor::Agent,
                        body: "# Heading\n\nSome *emphasis* text.".into(),
                        created_at: 2,
                    },
                ];
                app.reviews
                    .mutate(&review_id, |review| review.threads.push(thread))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                thread_id
            })
            .unwrap();
        // Markdown parsing runs on a background task, so the first frame can
        // render empty — park the executor until it settles before asserting.
        cx.run_until_parked();
        cx.run_until_parked();

        visual
            .debug_bounds(test_debug_selector(format!(
                "thread-message-md-{thread_id}-1"
            )))
            .expect("the Agent message renders through the markdown path");
        assert!(
            visual
                .debug_bounds(test_debug_selector(format!(
                    "thread-message-md-{thread_id}-0"
                )))
                .is_none(),
            "the Reviewer message never renders through the markdown path"
        );
    }

    #[gpui::test]
    async fn escape_discards_a_reply_draft(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let thread_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.changeset_panels.guide_open = true;
                    app.ensure_open_changeset_review(Some(window), cx);
                    let review_id = app.current_review().expect("review").id.clone();
                    let thread = test_thread(&path, 1);
                    let thread_id = thread.id.clone();
                    app.reviews
                        .mutate(&review_id, |review| review.threads.push(thread))
                        .expect("mutate review");
                    app.sidebar_tab = SidebarTab::Threads;
                    app.start_thread_reply(&thread_id, window, cx);
                    thread_id
                })
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("thread-reply-composer")
            .expect("reply composer renders before escape");
        app_entity.read_with(cx, |app, _| {
            assert_eq!(
                app.reply_draft_thread_id.as_deref(),
                Some(thread_id.as_str()),
                "starting a reply opens its composer"
            );
        });

        visual.simulate_keystrokes("escape");
        cx.run_until_parked();

        // `debug_bounds` never removes a selector once painted (see
        // `Frame::clear`), so absence is asserted against the reply-draft
        // state instead of `debug_bounds("thread-reply-composer")`.
        app_entity.read_with(cx, |app, _| {
            assert!(
                app.reply_draft_thread_id.is_none(),
                "escape discards the reply draft"
            );
            let threads = app.open_changeset_threads();
            let thread = threads
                .iter()
                .find(|thread| thread.id == thread_id)
                .expect("thread still present");
            assert_eq!(thread.messages.len(), 1, "escape never saved a message");
        });
    }

    #[gpui::test]
    async fn replying_appends_a_message_and_bubbles_the_thread_to_the_top(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let (older_id, newer_id) = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.changeset_panels.guide_open = true;
                    app.ensure_open_changeset_review(Some(window), cx);
                    let review_id = app.current_review().expect("review").id.clone();
                    let mut older = test_thread(&path, 1);
                    older.created_at = 10;
                    older.messages[0].created_at = 10;
                    let mut newer = test_thread(&path, 2);
                    newer.created_at = 20;
                    newer.messages[0].created_at = 20;
                    let older_id = older.id.clone();
                    let newer_id = newer.id.clone();
                    app.reviews
                        .mutate(&review_id, |review| review.threads.extend([older, newer]))
                        .expect("mutate review");
                    app.sidebar_tab = SidebarTab::Threads;
                    cx.notify();
                    (older_id, newer_id)
                })
            })
            .unwrap();
        cx.run_until_parked();

        // Sanity: before replying, the newer thread leads the list.
        let newer_before = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{newer_id}")))
            .expect("newer thread row renders");
        let older_before = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{older_id}")))
            .expect("older thread row renders");
        assert!(
            newer_before.origin.y < older_before.origin.y,
            "the newer thread leads before any reply"
        );

        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.start_thread_reply(&older_id, window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        visual
            .debug_bounds("thread-reply-composer")
            .expect("reply composer renders on the older thread");

        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.reply_input
                        .update(cx, |state, cx| state.set_value("Following up", window, cx));
                    app.save_thread_reply(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();

        app_entity.read_with(cx, |app, _| {
            assert!(
                app.reply_draft_thread_id.is_none(),
                "saving clears the reply draft"
            );
            let threads = app.open_changeset_threads();
            let older = threads
                .iter()
                .find(|thread| thread.id == older_id)
                .expect("older thread still present");
            assert_eq!(older.messages.len(), 2, "the reply appended a message");
            assert_eq!(older.messages[1].body, "Following up");
            assert_eq!(
                older.messages[1].author,
                crate::reviews::MessageAuthor::Reviewer
            );
            assert_eq!(
                app.selected_thread_id.as_deref(),
                Some(older_id.as_str()),
                "the replied-to thread stays selected"
            );
        });

        let older_after = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{older_id}")))
            .expect("older thread row still renders");
        let newer_after = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{newer_id}")))
            .expect("newer thread row still renders");
        assert!(
            older_after.origin.y < newer_after.origin.y,
            "replying bubbles the thread to the top of the list"
        );
    }

    #[gpui::test]
    async fn a_reply_draft_and_a_new_thread_draft_are_mutually_exclusive(cx: &mut TestAppContext) {
        let (_dir, path, app_entity, window, _visual) = open_two_commit_changeset_with_root(cx);
        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.changeset_panels.guide_open = true;
                    app.ensure_open_changeset_review(Some(window), cx);
                    let review_id = app.current_review().expect("review").id.clone();
                    let thread = test_thread(&path, 1);
                    let thread_id = thread.id.clone();
                    app.reviews
                        .mutate(&review_id, |review| review.threads.push(thread))
                        .expect("mutate review");

                    // Opening a reply cancels an in-progress new-thread draft.
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
                    app.stage_thread_draft(window, cx);
                    assert!(
                        app.thread_draft.is_some(),
                        "selection stages a new-thread draft"
                    );
                    app.start_thread_reply(&thread_id, window, cx);
                    assert!(
                        app.thread_draft.is_none(),
                        "starting a reply cancels the new-thread draft"
                    );
                    assert_eq!(
                        app.reply_draft_thread_id.as_deref(),
                        Some(thread_id.as_str())
                    );

                    // Staging a new-thread draft cancels an open reply.
                    app.stage_thread_draft(window, cx);
                    assert!(
                        app.thread_draft.is_some(),
                        "selection stages a new-thread draft again"
                    );
                    assert!(
                        app.reply_draft_thread_id.is_none(),
                        "staging a new-thread draft cancels the open reply"
                    );
                });
            })
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn a_thread_collapses_to_its_header_and_expands_back(cx: &mut TestAppContext) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let thread_id = window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let mut thread = test_thread(&path, 1);
                thread.messages.push(crate::reviews::ThreadMessage {
                    id: "reply".into(),
                    author: crate::reviews::MessageAuthor::Reviewer,
                    body: "a reply".into(),
                    created_at: 5,
                });
                let thread_id = thread.id.clone();
                app.reviews
                    .mutate(&review_id, |review| review.threads.push(thread))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                thread_id
            })
            .unwrap();
        cx.run_until_parked();

        // Expanded by default: the first message renders and no count badge
        // shows.
        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-0")))
            .expect("a newly created thread starts expanded");
        assert!(
            visual
                .debug_bounds(test_debug_selector(format!("thread-count-{thread_id}")))
                .is_none(),
            "an expanded thread shows no message-count badge"
        );

        let chevron = visual
            .debug_bounds(test_debug_selector(format!("thread-collapse-{thread_id}")))
            .expect("the collapse chevron renders");
        visual.simulate_click(chevron.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert!(
                    app.collapsed_thread_ids.contains(&thread_id),
                    "clicking the chevron collapses the thread"
                );
                assert_ne!(
                    app.selected_thread_id.as_deref(),
                    Some(thread_id.as_str()),
                    "clicking the chevron never selects the thread"
                );
            })
            .unwrap();

        let count = visual
            .debug_bounds(test_debug_selector(format!("thread-count-{thread_id}")))
            .expect("the collapsed row shows a message count");
        let header = visual
            .debug_bounds(test_debug_selector(format!("thread-row-{thread_id}")))
            .expect("the row still renders collapsed");
        assert!(
            count.origin.y >= header.origin.y
                && count.origin.y + count.size.height <= header.origin.y + header.size.height,
            "the message count sits within the collapsed row's header"
        );

        // Expand again via the same chevron.
        let chevron_again = visual
            .debug_bounds(test_debug_selector(format!("thread-collapse-{thread_id}")))
            .expect("the collapse chevron still renders");
        visual.simulate_click(chevron_again.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert!(
                    !app.collapsed_thread_ids.contains(&thread_id),
                    "clicking the chevron again expands the thread"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    async fn collapse_all_and_expand_all_toggle_every_thread(cx: &mut TestAppContext) {
        let (_dir, path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let (first_id, second_id) = window
            .update(cx, |app, _window, cx| {
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let mut first = test_thread(&path, 1);
                first.created_at = 10;
                first.messages[0].created_at = 10;
                let mut second = test_thread(&path, 2);
                second.created_at = 20;
                second.messages[0].created_at = 20;
                let ids = (first.id.clone(), second.id.clone());
                app.reviews
                    .mutate(&review_id, |review| review.threads.extend([first, second]))
                    .expect("mutate review");
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                ids
            })
            .unwrap();
        cx.run_until_parked();

        let collapse_all = visual
            .debug_bounds("threads-collapse-all")
            .expect("the collapse-all header control renders");
        visual.simulate_click(collapse_all.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert!(
                    app.collapsed_thread_ids.contains(&first_id)
                        && app.collapsed_thread_ids.contains(&second_id),
                    "collapse-all collapses every thread in the open changeset's review"
                );
            })
            .unwrap();
        visual
            .debug_bounds(test_debug_selector(format!("thread-count-{first_id}")))
            .expect("the first thread shows its collapsed count");
        visual
            .debug_bounds(test_debug_selector(format!("thread-count-{second_id}")))
            .expect("the second thread shows its collapsed count");

        let expand_all = visual
            .debug_bounds("threads-expand-all")
            .expect("the expand-all header control renders");
        visual.simulate_click(expand_all.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert!(
                    app.collapsed_thread_ids.is_empty(),
                    "expand-all clears every collapsed thread"
                );
            })
            .unwrap();
        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{first_id}-0")))
            .expect("the first thread's message renders again once expanded");
        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{second_id}-0")))
            .expect("the second thread's message renders again once expanded");
    }

    #[gpui::test]
    async fn a_thread_with_an_open_reply_stays_expanded_through_collapse_all_and_after_closing(
        cx: &mut TestAppContext,
    ) {
        let (_dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let thread_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.changeset_panels.guide_open = true;
                    app.ensure_open_changeset_review(Some(window), cx);
                    let review_id = app.current_review().expect("review").id.clone();
                    let thread = test_thread(&path, 1);
                    let thread_id = thread.id.clone();
                    app.reviews
                        .mutate(&review_id, |review| review.threads.push(thread))
                        .expect("mutate review");
                    app.sidebar_tab = SidebarTab::Threads;
                    // Collapsed up front, then a reply is opened on it: per
                    // the threads spec, opening the composer is a real
                    // expansion, not just a render mask.
                    app.collapsed_thread_ids.insert(thread_id.clone());
                    app.start_thread_reply(&thread_id, window, cx);
                    thread_id
                })
            })
            .unwrap();
        cx.run_until_parked();

        app_entity.read_with(cx, |app, _| {
            assert!(
                !app.collapsed_thread_ids.contains(&thread_id),
                "opening a reply composer removes the thread from the collapsed set"
            );
        });
        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-0")))
            .expect("the thread renders expanded once its reply composer opens");

        // Collapse-all runs while the composer is still open: the render
        // stays expanded (the render-time override in
        // `thread_row_collapsed`), and the thread is not folded into the
        // recorded collapsed set either (the skip in
        // `set_all_threads_collapsed`) — so nothing needs to un-collapse it
        // later.
        let collapse_all = visual
            .debug_bounds("threads-collapse-all")
            .expect("the collapse-all header control renders");
        visual.simulate_click(collapse_all.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        app_entity.read_with(cx, |app, _| {
            assert!(
                !app.collapsed_thread_ids.contains(&thread_id),
                "collapse-all skips the thread with an open reply composer"
            );
        });
        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-0")))
            .expect("the thread still renders expanded after collapse-all");

        // Save the reply: the composer closes, and the thread must stay
        // expanded rather than snapping back to collapsed.
        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.reply_input
                        .update(cx, |state, cx| state.set_value("Following up", window, cx));
                    app.save_thread_reply(window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();

        app_entity.read_with(cx, |app, _| {
            assert!(
                app.reply_draft_thread_id.is_none(),
                "saving closed the reply composer"
            );
            assert!(
                !app.collapsed_thread_ids.contains(&thread_id),
                "saving the reply does not re-collapse the thread"
            );
        });
        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{thread_id}-0")))
            .expect("the thread stays expanded after saving the reply");
    }

    #[gpui::test]
    async fn selecting_a_thread_from_the_diff_expands_it(cx: &mut TestAppContext) {
        let (_dir, head_sha) = init_repo_with_long_diff();
        let repo_path = _dir.path().to_path_buf();
        let path = "long.txt".to_string();
        let window = add_app_window(cx);

        let target_id = window
            .update(cx, |app, window, cx| {
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
                app.open_file_preview(path.clone(), cx);
                app.ensure_open_changeset_review(None, cx);
                let review_id = app.current_review().expect("review").id.clone();
                let target = crate::reviews::ReviewThread {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: path.clone(),
                    anchor: crate::reviews::ThreadAnchor {
                        side: crate::reviews::ThreadSide::New,
                        start_line: 1,
                        start_col: 0,
                        end_line: 1,
                        end_col: 12,
                        quoted_text: String::new(),
                    },
                    messages: vec![crate::reviews::ThreadMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        author: crate::reviews::MessageAuthor::Reviewer,
                        body: "the clicked one".into(),
                        created_at: 50,
                    }],
                    created_at: 50,
                    ..Default::default()
                };
                let target_id = target.id.clone();
                app.reviews
                    .mutate(&review_id, |review| review.threads.push(target))
                    .expect("mutate review");
                // Collapse it up front so the diff-click has something to
                // expand.
                app.collapsed_thread_ids.insert(target_id.clone());
                app.sidebar_tab = SidebarTab::Threads;
                cx.notify();
                target_id
            })
            .expect("open long-diff changeset with one collapsed thread");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(gpui::size(px(900.), px(360.)));

        visual
            .debug_bounds(test_debug_selector(format!("thread-count-{target_id}")))
            .expect("the thread starts collapsed, showing its count");

        let code = visual
            .debug_bounds("file-diff-code-new-0")
            .expect("first code row");
        visual.simulate_click(code.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                assert_eq!(
                    app.selected_thread_id.as_deref(),
                    Some(target_id.as_str()),
                    "clicking anchored diff text selects its thread"
                );
                assert!(
                    !app.collapsed_thread_ids.contains(&target_id),
                    "selecting a thread from the diff expands it"
                );
            })
            .unwrap();

        visual
            .debug_bounds(test_debug_selector(format!("thread-message-{target_id}-0")))
            .expect("the thread's message renders once expanded by the diff selection");
    }

    /// Stub CLI emitting one plain-text ask answer (no schema): init, an
    /// assistant text block, then a success result carrying the answer.
    const ASK_TRANSCRIPT: &str = r#"echo '{"type":"system","subtype":"init","session_id":"stub"}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Because it propagates errors."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Because it propagates errors."}'"#;

    /// A row-0 range selection on the new side, staging fodder for an agent
    /// draft.
    fn new_side_row0_selection() -> diff_selection::DiffSelection {
        diff_selection::DiffSelection {
            side: repo::DiffSide::New,
            anchor: diff_selection::DiffPoint { row: 0, column: 0 },
            head: diff_selection::DiffPoint { row: 0, column: 3 },
            goal_x: None,
        }
    }

    /// Poll until `predicate` holds against the app, driving the executor.
    /// The stub subprocess runs in real time on a clock the executor's fake
    /// timers never advance, so a real sleep separates executor turns (see
    /// `ai::mod`'s `wait_until`).
    fn wait_for_app(cx: &mut TestAppContext, app: &Entity<App>, predicate: impl Fn(&App) -> bool) {
        for _ in 0..200 {
            cx.run_until_parked();
            if app.read_with(cx, |app, _| predicate(app)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("condition not reached within 10s");
    }

    #[gpui::test]
    async fn asking_a_question_creates_an_agent_thread_and_appends_the_reply(
        cx: &mut TestAppContext,
    ) {
        let (dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let stub = stub_cli(dir.path(), ASK_TRANSCRIPT);
        // Stage the agent draft and type the question, then save through the
        // composer's own Save (the "Ask" button) so the button wiring — not
        // just the handler — is exercised.
        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.ai_enabled = true;
                    app.set_ai_cli_program(stub.clone(), cx);
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(pane, &path, new_side_row0_selection(), cx);
                    app.stage_agent_thread_draft(window, cx);
                    app.thread_input.update(cx, |state, cx| {
                        state.set_value("Why the question mark?", window, cx)
                    });
                });
            })
            .unwrap();
        cx.run_until_parked();

        let save = visual
            .debug_bounds("thread-composer-save")
            .expect("the agent composer offers a Save (Ask) control");
        visual.simulate_click(save.center(), gpui::Modifiers::none());
        cx.run_until_parked();

        let thread_id = app_entity.read_with(cx, |app, _| {
            app.selected_thread_id
                .clone()
                .expect("saving selects the new agent thread")
        });

        // The reviewer question persists immediately; the AI reply lands once
        // the stub turn completes.
        app_entity.read_with(cx, |app, _| {
            let thread = app
                .open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .expect("agent thread persisted");
            assert_eq!(thread.kind, crate::reviews::ReviewThreadKind::Agent);
            assert_eq!(thread.messages.len(), 1, "only the question so far");
        });

        wait_for_app(cx, &app_entity, |app| {
            app.open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .is_some_and(|thread| thread.messages.len() == 2)
        });
        cx.run_until_parked();

        app_entity.read_with(cx, |app, _| {
            let thread = app
                .open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .expect("agent thread present");
            assert_eq!(thread.messages.len(), 2, "reviewer question plus AI reply");
            assert_eq!(thread.messages[0].author, MessageAuthor::Reviewer);
            assert_eq!(thread.messages[1].author, MessageAuthor::Agent);
            assert_eq!(thread.messages[1].body, "Because it propagates errors.");
        });

        // The AI reply (message index 1) carries the "AI" tag.
        visual
            .debug_bounds(test_debug_selector(format!(
                "thread-message-{thread_id}-1-agent-tag"
            )))
            .expect("the AI reply shows an AI tag");
        visual
            .debug_bounds(test_debug_selector(format!(
                "thread-agent-disclaimer-{thread_id}"
            )))
            .expect("the agent thread shows the standing disclaimer once answered");
    }

    #[gpui::test]
    async fn a_running_agent_thread_shows_a_ticker_and_cancel(cx: &mut TestAppContext) {
        let (dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        // Init + a tool_use (so the ticker names a tool), then hang so the
        // turn stays Running until we cancel it.
        let stub = stub_cli(
            dir.path(),
            "echo '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"stub\"}'\n\
             echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{}}]}}'\n\
             sleep 300\n\
             exit 0",
        );
        let thread_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.ai_enabled = true;
                    app.set_ai_cli_program(stub.clone(), cx);
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(pane, &path, new_side_row0_selection(), cx);
                    app.stage_agent_thread_draft(window, cx);
                    app.thread_input
                        .update(cx, |state, cx| state.set_value("Is this safe?", window, cx));
                    app.save_agent_thread_draft(window, cx);
                    app.selected_thread_id
                        .clone()
                        .expect("agent thread selected")
                })
            })
            .unwrap();

        let running_selector = test_debug_selector(format!("thread-agent-running-{thread_id}"));
        let cancel_selector = test_debug_selector(format!("thread-agent-cancel-{thread_id}"));
        let mut ticker = None;
        for _ in 0..200 {
            cx.run_until_parked();
            if let Some(bounds) = visual.debug_bounds(running_selector) {
                ticker = Some(bounds);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        ticker.expect("the running ticker appears while the turn is in flight");
        let cancel = visual
            .debug_bounds(cancel_selector)
            .expect("the running state offers Cancel");

        visual.simulate_click(cancel.center(), gpui::Modifiers::none());
        wait_for_app(cx, &app_entity, |app| {
            !app.agent_thread_runs.contains_key(&thread_id)
        });

        app_entity.read_with(cx, |app, _| {
            let thread = app
                .open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .expect("agent thread present");
            assert_eq!(
                thread.messages.len(),
                1,
                "cancel leaves the thread with just the question — no partial reply"
            );
        });
    }

    #[gpui::test]
    async fn agent_failure_shows_an_error_with_retry(cx: &mut TestAppContext) {
        let (dir, path, app_entity, window, mut visual) = open_two_commit_changeset_with_root(cx);
        let stub = stub_cli(dir.path(), "echo 'boom' >&2\nexit 3");
        let thread_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.ai_enabled = true;
                    app.set_ai_cli_program(stub.clone(), cx);
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(pane, &path, new_side_row0_selection(), cx);
                    app.stage_agent_thread_draft(window, cx);
                    app.thread_input
                        .update(cx, |state, cx| state.set_value("Explain?", window, cx));
                    app.save_agent_thread_draft(window, cx);
                    app.selected_thread_id
                        .clone()
                        .expect("agent thread selected")
                })
            })
            .unwrap();

        wait_for_app(cx, &app_entity, |app| {
            app.agent_thread_errors.contains_key(&thread_id)
        });
        cx.run_until_parked();

        visual
            .debug_bounds(test_debug_selector(format!(
                "thread-agent-error-{thread_id}"
            )))
            .expect("the failed turn shows an error message");
        visual
            .debug_bounds(test_debug_selector(format!(
                "thread-agent-retry-{thread_id}"
            )))
            .expect("the failed turn offers Retry");
        app_entity.read_with(cx, |app, _| {
            let thread = app
                .open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .expect("agent thread present");
            assert_eq!(
                thread.messages.len(),
                1,
                "a failed turn appends no AI message; the question stays"
            );
        });
    }

    #[gpui::test]
    async fn replying_to_an_agent_thread_sends_a_follow_up_turn(cx: &mut TestAppContext) {
        let (dir, path, app_entity, window, _visual) = open_two_commit_changeset_with_root(cx);
        let stub = stub_cli(dir.path(), ASK_TRANSCRIPT);
        let thread_id = window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.settings.ai_enabled = true;
                    app.set_ai_cli_program(stub.clone(), cx);
                    let pane = app.workspace.active_pane();
                    app.set_diff_selection(pane, &path, new_side_row0_selection(), cx);
                    app.stage_agent_thread_draft(window, cx);
                    app.thread_input
                        .update(cx, |state, cx| state.set_value("Why?", window, cx));
                    app.save_agent_thread_draft(window, cx);
                    app.selected_thread_id
                        .clone()
                        .expect("agent thread selected")
                })
            })
            .unwrap();

        wait_for_app(cx, &app_entity, |app| {
            app.open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .is_some_and(|thread| thread.messages.len() == 2)
        });

        // The CLI session bound to the thread, kept for resume.
        let session_before = app_entity.read_with(cx, |app, _| {
            *app.agent_thread_sessions
                .get(&thread_id)
                .expect("the completed turn kept its session binding")
        });

        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    app.start_thread_reply(&thread_id, window, cx);
                    app.reply_input.update(cx, |state, cx| {
                        state.set_value("And is that safe here?", window, cx)
                    });
                    app.save_thread_reply(window, cx);
                });
            })
            .unwrap();

        wait_for_app(cx, &app_entity, |app| {
            app.open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .is_some_and(|thread| thread.messages.len() == 4)
        });
        cx.run_until_parked();

        app_entity.read_with(cx, |app, _| {
            let thread = app
                .open_changeset_threads()
                .into_iter()
                .find(|thread| thread.id == thread_id)
                .expect("agent thread present");
            assert_eq!(
                thread.messages.len(),
                4,
                "question, reply, follow-up question, follow-up reply"
            );
            assert_eq!(thread.messages[2].author, MessageAuthor::Reviewer);
            assert_eq!(thread.messages[2].body, "And is that safe here?");
            assert_eq!(thread.messages[3].author, MessageAuthor::Agent);
            // The follow-up resumed the same CLI session rather than starting
            // a fresh one.
            assert_eq!(
                app.agent_thread_sessions.get(&thread_id).copied(),
                Some(session_before),
                "the follow-up resumed the bound session"
            );
        });
    }
}
