//! Right-docked review-guide panel: renders the AI-generated review guide
//! for the open changeset. Only rendered from `render_changeset_screen`
//! while `settings.changeset_panels.guide_open && settings.ai_enabled`.
//! Builds on Task 6's guide-generation machinery in `src/app.rs`
//! (`start_guide_generation`, `cancel_guide_generation`, `guide_thread`,
//! `guide_error`). The behavior spec for this feature lands with Task 9.
//!
//! Panel state precedence (top to bottom wins): Running (a guide-generation
//! thread is in flight) > Failed (the last turn errored) > Done (a guide is
//! persisted on the current review) > Empty (nothing to show yet).

use super::*;

use crate::reviews::{ReviewGuide, ReviewGuideEntry};
use crate::theme::palette;

/// Inner padding of the panel's content column.
const GUIDE_PANEL_PADDING: f32 = 12.;

/// Reading-order notes: secondary to the file name, but bigger than the
/// file tree's 10px secondary size — notes are prose meant to be read.
const GUIDE_NOTE_TEXT_SIZE: f32 = 12.;

impl App {
    /// Render the review-guide panel's contents (see module docs for the
    /// state contract).
    pub(crate) fn render_guide_panel(
        &self,
        changeset: &repo::ChangeSet,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Cloned so the state checks below don't hold a borrow of
        // `self.reviews` across the `self.render_guide_*` calls, each of
        // which also needs `&self`.
        let persisted_guide = self
            .current_review()
            .and_then(|review| review.guide.clone());

        let body: AnyElement = if let Some(thread_id) = self.guide_thread {
            let latest_activity = self
                .ai_sessions
                .read(cx)
                .thread(thread_id)
                .and_then(|thread| thread.latest_activity.clone());
            self.render_guide_running_state(
                latest_activity,
                persisted_guide.as_ref(),
                changeset,
                cx,
            )
        } else if let Some(error) = self.guide_error.clone() {
            self.render_guide_failed_state(error, cx)
        } else if let Some(guide) = persisted_guide.as_ref() {
            self.render_guide_done_state(changeset, guide, cx)
        } else {
            self.render_guide_empty_state(cx)
        };

        // Match the file tree's typeface and scale so the two sidebars read
        // as one surface; children inherit unless they set a secondary size.
        div()
            .id("guide-panel")
            .font_family(MONO_FONT_FAMILY)
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .h_full()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .border_l_1()
            .border_color(palette().border)
            .bg(palette().surface)
            .text_size(px(FILE_TREE_TEXT_SIZE))
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                if app.guide_panel_hovered != *hovered {
                    app.guide_panel_hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("guide-scroll")
                    .debug_selector(|| "guide-scroll".to_string())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.guide_scroll)
                    .child(body),
            )
            .when(self.guide_panel_hovered, |panel| {
                panel.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .debug_selector(|| "guide-scrollbar".to_string())
                        .child(
                            Scrollbar::vertical(&self.guide_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
            })
    }

    fn render_guide_running_state(
        &self,
        latest_activity: Option<String>,
        persisted_guide: Option<&ReviewGuide>,
        changeset: &repo::ChangeSet,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ticker_text = match latest_activity {
            Some(name) => format!("Running {name}…"),
            None => "Working…".to_string(),
        };

        let ticker = div()
            .id("guide-ticker")
            .debug_selector(|| "guide-ticker".to_string())
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(div().text_color(palette().text).child(ticker_text))
            .child(self.render_guide_text_button(
                "guide-cancel",
                "Cancel",
                |app, cx| app.cancel_guide_generation(cx),
                cx,
            ));

        let mut column = div()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(GUIDE_PANEL_PADDING))
            .child(ticker);

        if let Some(guide) = persisted_guide {
            column = column.child(self.render_guide_content(changeset, guide, true, cx));
        }

        column.into_any_element()
    }

    fn render_guide_failed_state(&self, error: String, cx: &mut Context<Self>) -> AnyElement {
        // Belt and braces: a failure must never render as a bare Retry
        // button, even if some path hands us an empty message.
        let error = if error.trim().is_empty() {
            "Guide generation failed, and the AI reported no error message.".to_string()
        } else {
            error
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(GUIDE_PANEL_PADDING))
            .child(
                div()
                    .id("guide-error")
                    .debug_selector(|| "guide-error".to_string())
                    .text_color(palette().text_muted)
                    .child(error),
            )
            .child(self.render_guide_text_button(
                "guide-retry",
                "Retry",
                |app, cx| app.start_guide_generation(cx),
                cx,
            ))
            .into_any_element()
    }

    fn render_guide_done_state(
        &self,
        changeset: &repo::ChangeSet,
        guide: &ReviewGuide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(GUIDE_PANEL_PADDING))
            .child(self.render_guide_content(changeset, guide, false, cx))
            .child(self.render_guide_text_button(
                "guide-regenerate",
                "Regenerate",
                |app, cx| app.start_guide_generation(cx),
                cx,
            ))
            .child(
                div()
                    .text_color(palette().text_muted)
                    .text_size(px(FILE_TREE_SECONDARY_TEXT_SIZE))
                    .child("AI-generated — verify against the diff"),
            )
            .into_any_element()
    }

    fn render_guide_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(GUIDE_PANEL_PADDING))
            .child(div().text_color(palette().text_muted).child(
                "Generate an AI review guide: a summary of this changeset and a suggested reading order.",
            ))
            .child(self.render_guide_text_button(
                "guide-generate",
                "Generate guide",
                |app, cx| app.start_guide_generation(cx),
                cx,
            ))
            .into_any_element()
    }

    /// Summary paragraphs and review-order list shared by the Done state and
    /// the Running state's dimmed preview of a still-valid persisted guide.
    /// `muted` renders the summary and order-row paths in `text_muted`
    /// instead of `text`.
    fn render_guide_content(
        &self,
        changeset: &repo::ChangeSet,
        guide: &ReviewGuide,
        muted: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let summary_color = if muted {
            palette().text_muted
        } else {
            palette().text
        };
        let paragraphs = guide
            .summary
            .split("\n\n")
            .map(|paragraph| div().text_color(summary_color).child(paragraph.to_string()))
            .collect::<Vec<_>>();
        let summary = div()
            .id("guide-summary")
            .debug_selector(|| "guide-summary".to_string())
            .flex()
            .flex_col()
            .gap_2()
            .children(paragraphs);

        let paths: Vec<&str> = guide
            .review_order
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        let labels = disambiguated_file_labels(&paths);
        let order_rows = guide
            .review_order
            .iter()
            .zip(labels)
            .enumerate()
            .map(|(index, (entry, label))| {
                self.render_guide_order_row(index, entry, label, changeset, muted, cx)
            })
            .collect::<Vec<_>>();
        let order_list = div().flex().flex_col().gap_2().children(order_rows);

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(summary)
            .child(order_list)
    }

    /// One row in the guide's suggested reading order: index, change-kind
    /// marker (reusing the file tree's mapping), the file name first with a
    /// muted disambiguating path suffix only when another entry shares the
    /// name, and the note beneath in `text_muted`. Clicking opens the file's
    /// diff tab.
    fn render_guide_order_row(
        &self,
        index: usize,
        entry: &ReviewGuideEntry,
        label: FileLabel,
        changeset: &repo::ChangeSet,
        muted: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let matched_file = changeset.files.iter().find(|file| file.path == entry.path);
        let kind = matched_file
            .map(|file| file.kind)
            .unwrap_or(repo::ChangeKind::Modified);
        let selector = format!("guide-order-row-{index}");
        let marker = render_change_status_icon(
            kind,
            format!("{selector}-marker"),
            format!("{selector}-marker-icon"),
        );

        let path_color = if muted {
            palette().text_muted
        } else {
            palette().text
        };
        let path_row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_1()
            .child(div().text_color(path_color).child(label.name))
            .when_some(label.suffix, |row, suffix| {
                row.child(
                    div()
                        .text_color(palette().text_muted)
                        .text_size(px(GUIDE_NOTE_TEXT_SIZE))
                        .child(format!("({suffix})")),
                )
            });

        let path = entry.path.clone();
        let selector_for_closure = selector.clone();
        div()
            .id(("guide-order-row", index))
            .debug_selector(move || selector_for_closure.clone())
            .flex()
            .flex_col()
            .gap_1()
            .cursor_pointer()
            .hover(|style| style.bg(palette().ghost_element_hover))
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                app.open_file_preview(path.clone(), cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_color(palette().text_muted)
                            .child(format!("{}", index + 1)),
                    )
                    .child(marker)
                    .child(path_row),
            )
            .child(
                div()
                    .text_color(palette().text_muted)
                    .text_size(px(GUIDE_NOTE_TEXT_SIZE))
                    .child(entry.note.clone()),
            )
    }

    /// A small bordered text button, matching the review title bar's
    /// "Confirm delete" / "Mark complete" affordances. Wrapped in a plain
    /// flex row so the button hugs its label instead of being stretched to
    /// the column's full width.
    fn render_guide_text_button(
        &self,
        selector: &'static str,
        label: &'static str,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .child(self.render_guide_text_button_inner(selector, label, on_click, cx))
    }

    fn render_guide_text_button_inner(
        &self,
        selector: &'static str,
        label: &'static str,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(palette().border)
            .text_color(palette().accent)
            .text_size(px(FILE_TREE_DIFF_STAT_TEXT_SIZE))
            .cursor_pointer()
            .hover(|style| style.bg(palette().ghost_element_hover))
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| on_click(app, cx)))
            .child(label)
    }
}

/// A reading-order entry's display label: file name first, plus the shortest
/// trailing directory suffix needed to tell it apart from other entries with
/// the same name (Zed-style tab disambiguation). `suffix` is `None` when the
/// name alone is unique within the guide.
struct FileLabel {
    name: String,
    suffix: Option<String>,
}

/// Compute display labels for the guide's paths. Names that appear once get
/// no suffix; duplicates get the fewest trailing directory components that
/// distinguish them from every other same-named entry — usually just the
/// containing folder.
fn disambiguated_file_labels(paths: &[&str]) -> Vec<FileLabel> {
    fn split(path: &str) -> (Vec<&str>, &str) {
        match path.rsplit_once('/') {
            Some((dir, file)) => (dir.split('/').collect(), file),
            None => (Vec::new(), path),
        }
    }

    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for path in paths {
        *name_counts.entry(split(path).1).or_default() += 1;
    }

    paths
        .iter()
        .map(|path| {
            let (dirs, name) = split(path);
            if name_counts[name] < 2 || dirs.is_empty() {
                return FileLabel {
                    name: name.to_string(),
                    suffix: None,
                };
            }
            let other_dirs: Vec<Vec<&str>> = paths
                .iter()
                .filter(|other| **other != *path)
                .filter_map(|other| {
                    let (odirs, oname) = split(other);
                    (oname == name).then_some(odirs)
                })
                .collect();
            // Grow the trailing suffix until no same-named entry shares it.
            // A shorter other-path can't share an equal-length suffix, so it
            // never forces growth; label lengths differing is distinction
            // enough.
            let mut take = 1;
            while take < dirs.len() {
                let suffix = &dirs[dirs.len() - take..];
                let unique = other_dirs
                    .iter()
                    .all(|odirs| odirs.len() < take || &odirs[odirs.len() - take..] != suffix);
                if unique {
                    break;
                }
                take += 1;
            }
            FileLabel {
                name: name.to_string(),
                suffix: Some(dirs[dirs.len() - take..].join("/")),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};

    #[test]
    fn unique_file_names_need_no_path_suffix() {
        let labels =
            disambiguated_file_labels(&["internal/config.json", "alpaca/somethingelse.txt"]);
        assert_eq!(labels[0].name, "config.json");
        assert_eq!(labels[0].suffix, None);
        assert_eq!(labels[1].name, "somethingelse.txt");
        assert_eq!(labels[1].suffix, None);
    }

    #[test]
    fn duplicate_names_show_just_the_containing_folder() {
        let labels = disambiguated_file_labels(&[
            "deep/nested/internal/config.json",
            "alpaca/config.json",
            "somethingelse.txt",
        ]);
        assert_eq!(labels[0].suffix.as_deref(), Some("internal"));
        assert_eq!(labels[1].suffix.as_deref(), Some("alpaca"));
        assert_eq!(labels[2].suffix, None);
    }

    #[test]
    fn duplicate_names_in_same_named_folders_grow_the_suffix() {
        let labels = disambiguated_file_labels(&[
            "services/auth/src/main.rs",
            "services/billing/src/main.rs",
        ]);
        assert_eq!(labels[0].suffix.as_deref(), Some("auth/src"));
        assert_eq!(labels[1].suffix.as_deref(), Some("billing/src"));
    }

    #[test]
    fn a_root_level_duplicate_keeps_no_suffix_while_the_nested_one_gets_one() {
        let labels = disambiguated_file_labels(&["config.json", "internal/config.json"]);
        assert_eq!(labels[0].suffix, None);
        assert_eq!(labels[1].suffix.as_deref(), Some("internal"));
    }

    /// Stub CLI that emits a valid guide JSON result for any prompt. The
    /// review-order path matches the file `init_repo_with_two_commits`
    /// changes. A local copy of `app`'s own private test helper of the same
    /// name (see Task 6's `app::tests::guide_stub_transcript`) — that one
    /// lives inside `app.rs`'s private `mod tests` and isn't reachable from
    /// this sibling module's own test module.
    fn guide_stub_transcript(path_in_repo: &str) -> String {
        // A literal `\n` inside the single-quoted `echo` argument is not
        // safe here: some `/bin/sh` builtins (e.g. dash's `echo`) interpret
        // backslash escapes by default and would split this one JSON line
        // into two, breaking the parse. Keep the summary a single line.
        format!(
            r#"echo '{{"type":"system","subtype":"init","session_id":"stub"}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"{{\"summary\":\"Behavior summary.\",\"review_order\":[{{\"path\":\"{path_in_repo}\",\"note\":\"read first\"}}]}}"}}'"#
        )
    }

    /// Open a two-commit repo's changeset with AI enabled and the guide panel
    /// docked open. Returns the dir (kept alive), the changed file's path,
    /// the window, and the visual context.
    fn open_changeset_with_guide_panel(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        String,
        WindowHandle<App>,
        VisualTestContext,
    ) {
        let (dir, head_sha) = init_repo_with_two_commits();
        let repo_path = dir.path().to_path_buf();
        let changed_path = "hello.txt".to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.settings.ai_enabled = true;
                app.settings.changeset_panels.guide_open = true;
                app.open_repository_at(repo_path, window, cx);
                app.select_single_commit(head_sha, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset with guide panel");
        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        (dir, changed_path, window, visual)
    }

    #[gpui::test]
    async fn empty_state_shows_generate_and_done_state_shows_the_guide(cx: &mut TestAppContext) {
        let (dir, changed_path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let stub = stub_cli(dir.path(), &guide_stub_transcript(&changed_path));
        window
            .update(cx, |app, _window, cx| {
                app.set_ai_cli_program(stub.clone(), cx);
            })
            .unwrap();
        cx.run_until_parked();

        let generate = visual
            .debug_bounds("guide-generate")
            .expect("empty state shows a Generate button");
        visual.simulate_click(generate.center(), Modifiers::none());
        cx.run_until_parked();

        for _ in 0..200 {
            cx.run_until_parked();
            let done = window
                .read_with(cx, |app, _| {
                    app.current_review()
                        .is_some_and(|review| review.guide.is_some())
                })
                .unwrap();
            if done {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("guide-summary").is_some(),
            "done state renders the summary"
        );
        assert!(
            visual.debug_bounds("guide-order-row-0").is_some(),
            "done state renders the review-order row"
        );
        assert!(
            visual.debug_bounds("guide-regenerate").is_some(),
            "done state offers Regenerate"
        );
        window
            .read_with(cx, |app, _| {
                assert!(app.guide_thread.is_none(), "generation finished");
            })
            .unwrap();
    }

    #[gpui::test]
    async fn guide_content_taller_than_the_panel_scrolls_vertically(cx: &mut TestAppContext) {
        use gpui::size;

        let (_dir, changed_path, window, mut visual) = open_changeset_with_guide_panel(cx);

        // Seed a long guide straight onto the review — scrolling is a render
        // concern, so no CLI turn is needed to exercise it.
        window
            .update(cx, |app, window, cx| {
                app.ensure_open_changeset_review(Some(window), cx);
                let id = app.current_review().expect("review exists").id.clone();
                let guide = ReviewGuide {
                    summary: "One behavior change.".to_string(),
                    review_order: (0..40)
                        .map(|_| ReviewGuideEntry {
                            path: changed_path.clone(),
                            note: "a repeated entry to force overflow".to_string(),
                        })
                        .collect(),
                    generated_at: 1,
                };
                app.reviews
                    .mutate(&id, |review| review.guide = Some(guide))
                    .expect("seed guide");
                cx.notify();
            })
            .unwrap();

        visual.simulate_resize(size(px(700.), px(300.)));
        cx.run_until_parked();
        visual
            .debug_bounds("guide-scroll")
            .expect("scroll container rendered");

        let v_max = window
            .read_with(cx, |app, _| app.guide_scroll.max_offset())
            .expect("scroll handle readable");
        assert!(
            v_max.height > px(0.),
            "forty order rows in a 300px panel must overflow vertically, got {v_max:?}"
        );
    }

    #[gpui::test]
    async fn order_row_shows_a_change_marker_but_no_per_file_stats(cx: &mut TestAppContext) {
        let (dir, changed_path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let stub = stub_cli(dir.path(), &guide_stub_transcript(&changed_path));
        window
            .update(cx, |app, _window, cx| {
                app.set_ai_cli_program(stub.clone(), cx);
                app.start_guide_generation(cx);
            })
            .unwrap();

        for _ in 0..200 {
            cx.run_until_parked();
            let done = window
                .read_with(cx, |app, _| {
                    app.current_review()
                        .is_some_and(|review| review.guide.is_some())
                })
                .unwrap();
            if done {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("guide-order-row-0-marker").is_some(),
            "the reading-order row keeps the file list's change-kind marker"
        );
        // Never painted in this window, so absence is provable here (unlike
        // toggled-away selectors — see the status_footer test comment).
        assert!(
            visual.debug_bounds("guide-order-row-0-stats").is_none(),
            "per-file line stats were dropped from reading-order rows"
        );
    }

    #[gpui::test]
    async fn clicking_an_order_row_opens_that_files_diff_tab(cx: &mut TestAppContext) {
        let (dir, changed_path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let stub = stub_cli(dir.path(), &guide_stub_transcript(&changed_path));
        window
            .update(cx, |app, _window, cx| {
                app.set_ai_cli_program(stub.clone(), cx);
                app.start_guide_generation(cx);
            })
            .unwrap();

        for _ in 0..200 {
            cx.run_until_parked();
            let done = window
                .read_with(cx, |app, _| {
                    app.current_review()
                        .is_some_and(|review| review.guide.is_some())
                })
                .unwrap();
            if done {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        cx.run_until_parked();

        let row = visual
            .debug_bounds("guide-order-row-0")
            .expect("review-order row rendered");
        visual.simulate_click(row.center(), Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _| {
                let pane = app.workspace.active_pane();
                let key = app
                    .workspace
                    .active_item(pane)
                    .map(|item| item.key().to_string());
                assert_eq!(key.as_deref(), Some(changed_path.as_str()));
            })
            .unwrap();
    }

    #[gpui::test]
    async fn failed_state_shows_error_and_retry(cx: &mut TestAppContext) {
        let (dir, _changed_path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let stub = stub_cli(dir.path(), "echo 'boom' >&2\nexit 3");
        window
            .update(cx, |app, _window, cx| {
                app.set_ai_cli_program(stub.clone(), cx);
                app.start_guide_generation(cx);
            })
            .unwrap();

        for _ in 0..200 {
            cx.run_until_parked();
            let done = window
                .read_with(cx, |app, _| app.guide_error.is_some())
                .unwrap();
            if done {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("guide-error").is_some(),
            "failed state shows the error message"
        );
        assert!(
            visual.debug_bounds("guide-retry").is_some(),
            "failed state offers Retry"
        );
    }

    #[gpui::test]
    async fn running_state_shows_ticker_and_cancel(cx: &mut TestAppContext) {
        let (dir, _changed_path, window, mut visual) = open_changeset_with_guide_panel(cx);
        let stub = stub_cli(
            dir.path(),
            "echo '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"stub\"}'\n\
             echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\"input\":{}}]}}'\n\
             sleep 300\n\
             exit 0",
        );
        window
            .update(cx, |app, _window, cx| {
                app.set_ai_cli_program(stub.clone(), cx);
                app.start_guide_generation(cx);
            })
            .unwrap();

        let mut ticker_bounds = None;
        for _ in 0..200 {
            cx.run_until_parked();
            if let Some(bounds) = visual.debug_bounds("guide-ticker") {
                ticker_bounds = Some(bounds);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        ticker_bounds.expect("ticker appears while the thread is running");
        assert!(
            visual.debug_bounds("guide-cancel").is_some(),
            "running state offers Cancel"
        );

        let cancel = visual.debug_bounds("guide-cancel").expect("cancel button");
        visual.simulate_click(cancel.center(), Modifiers::none());

        for _ in 0..200 {
            cx.run_until_parked();
            let cancelled = window
                .read_with(cx, |app, _| app.guide_thread.is_none())
                .unwrap();
            if cancelled {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        window
            .read_with(cx, |app, _| {
                assert!(app.guide_thread.is_none(), "cancel clears the thread");
            })
            .unwrap();
    }
}
