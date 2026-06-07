//! Window-chrome title bar: the `{repo} / {sha}` context segment shown in
//! changeset mode and the popover it opens. See
//! docs/specs/review/workflow.md and
//! docs/superpowers/specs/2026-06-07-titlebar-context-switcher-design.md.

use gpui::{
    div, px, rgb, AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled,
};
use gpui_component::{TitleBar, TITLE_BAR_HEIGHT};

use super::{App, Mode, ReviewScreen, Selection};
use crate::repo::{ChangeSet, CommitInfo};

/// First seven characters of a full commit sha, matching the short form the
/// graph and the removed diff header used.
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Label for the title-bar pill. A single-commit changeset shows just the
/// short sha; a range shows the newest short sha plus the commit count.
fn context_pill_label(selection: &Selection, changeset: &ChangeSet) -> String {
    // In a well-formed call `changeset.commit_sha` is the newest commit, i.e.
    // `shas[0]` for a range, so the pill always shows the newest short sha.
    let newest = short_sha(&changeset.commit_sha);
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            format!("{newest} · {} commits", shas.len())
        }
        _ => newest,
    }
}

/// Oldest…newest short shas for a range changeset; `None` for a single commit.
/// `shas` is newest-first, so the first entry is the newest endpoint.
fn range_endpoints(selection: &Selection) -> Option<(String, String)> {
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            let newest = shas.first()?;
            let oldest = shas.last()?;
            Some((short_sha(oldest), short_sha(newest)))
        }
        _ => None,
    }
}

/// Popover header title. A range reads "Reviewing N commits"; a single commit
/// reads the commit summary, falling back to its short sha when the commit is
/// not in the loaded window.
fn popover_header_title(
    selection: &Selection,
    changeset: &ChangeSet,
    commits: &[CommitInfo],
) -> String {
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            format!("Reviewing {} commits", shas.len())
        }
        _ => commits
            .iter()
            .find(|commit| commit.sha == changeset.commit_sha)
            .map(|commit| commit.summary.clone())
            .unwrap_or_else(|| short_sha(&changeset.commit_sha)),
    }
}

/// Total added and removed lines across every file in the changeset.
fn changeset_line_totals(changeset: &ChangeSet) -> (usize, usize) {
    changeset
        .files
        .iter()
        .fold((0, 0), |(added, removed), file| {
            (
                added + file.line_stats.added,
                removed + file.line_stats.removed,
            )
        })
}

impl App {
    /// The window-chrome title bar. Always shows the repo name when a
    /// repository is open; in changeset mode it also shows the clickable
    /// context pill that opens the diff popover.
    pub(crate) fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.mode {
            Mode::RepoOpen { repo } => {
                let repo_name = super::repository_title(&repo.path);
                let mut row = div().flex().items_center().child(
                    div()
                        .font_family("monospace")
                        .text_size(px(13.))
                        .text_color(rgb(0xe6e6e6))
                        .child(repo_name),
                );

                if let ReviewScreen::Changeset { changeset, .. } = &self.review_screen {
                    let label = context_pill_label(&self.selection, changeset);
                    row = row
                        .child(
                            div()
                                .mx_2()
                                .text_size(px(13.))
                                .text_color(rgb(0x5a5a5a))
                                .child("/"),
                        )
                        .child(
                            div()
                                .id("title-bar-context")
                                .debug_selector(|| "title-bar-context".to_string())
                                .px_2()
                                .py(px(2.))
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0x34507a))
                                .bg(rgb(0x1d283a))
                                .font_family("monospace")
                                .text_size(px(13.))
                                .text_color(rgb(0xdbeafe))
                                .cursor_pointer()
                                .on_click(cx.listener(|app, _event, _window, cx| {
                                    app.context_popover_open = !app.context_popover_open;
                                    cx.notify();
                                }))
                                .child(label),
                        );
                }

                row
            }
            Mode::NoRepo => div(),
        };

        TitleBar::new().child(content)
    }

    /// The diff "switcher" popover, shown when the context pill is active.
    /// Returns `None` unless a changeset is open and the popover is toggled on.
    /// Rendered as a full-window overlay: a transparent backdrop that dismisses
    /// on outside click, plus the anchored card.
    pub(crate) fn render_context_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.context_popover_open {
            return None;
        }
        let Mode::RepoOpen { repo } = &self.mode else {
            return None;
        };
        let ReviewScreen::Changeset { changeset, .. } = &self.review_screen else {
            return None;
        };

        let title = popover_header_title(&self.selection, changeset, &repo.commits);
        let endpoints = range_endpoints(&self.selection);
        let (added, removed) = changeset_line_totals(changeset);
        let file_count = changeset.files.len();

        let mut header = div().flex().flex_col().gap_1().p_3().child(
            div()
                .text_size(px(13.))
                .text_color(rgb(0xededed))
                .child(title),
        );
        if let Some((oldest, newest)) = endpoints {
            header = header.child(
                div()
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(rgb(0x8a8a93))
                    .child(format!("{oldest} \u{2026} {newest}")),
            );
        }

        let stat_row = |label: &str, value: AnyElement| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .text_size(px(12.))
                .child(div().text_color(rgb(0x8a8a93)).child(label.to_string()))
                .child(value)
        };

        let files_row = stat_row(
            "Files changed",
            div()
                .text_color(rgb(0xc7c7cf))
                .child(file_count.to_string())
                .into_any_element(),
        );
        let lines_row = stat_row(
            "Lines",
            div()
                .flex()
                .gap_2()
                .child(div().text_color(rgb(0x7ee787)).child(format!("+{added}")))
                .child(
                    div()
                        .text_color(rgb(0xf08a8a))
                        .child(format!("\u{2212}{removed}")),
                )
                .into_any_element(),
        );

        let close = div()
            .id("title-bar-context-close")
            .debug_selector(|| "title-bar-context-close".to_string())
            .m_2()
            .flex()
            .items_center()
            .justify_center()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(0x5a2a2a))
            .bg(rgb(0x2a1818))
            .text_size(px(12.))
            .text_color(rgb(0xf3b4b4))
            .cursor_pointer()
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.close_changeset(cx);
            }))
            .child("Close changeset");

        let card = div()
            .absolute()
            .top(TITLE_BAR_HEIGHT)
            .left(px(80.))
            .occlude()
            .w(px(380.))
            .bg(rgb(0x141417))
            .border_1()
            .border_color(rgb(0x34343a))
            .rounded_lg()
            .debug_selector(|| "title-bar-context-popover".to_string())
            .child(header)
            .child(files_row)
            .child(lines_row)
            .child(close);

        let backdrop = div()
            .id("title-bar-context-backdrop")
            .absolute()
            .inset_0()
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.context_popover_open = false;
                cx.notify();
            }));

        Some(
            div()
                .absolute()
                .inset_0()
                .child(backdrop)
                .child(card)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Mode, ReviewScreen};
    use crate::repo::OpenRepository;
    use crate::repo::{ChangeKind, ChangeSet, ChangedFile, CommitInfo, LineStats};
    use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
    use std::path::PathBuf;

    const PILL: &str = "title-bar-context";

    fn app_window(cx: &mut TestAppContext) -> WindowHandle<App> {
        cx.update(gpui_component::init);
        cx.add_window(App::new)
    }

    fn repo_named(name: &str, commits: Vec<CommitInfo>) -> OpenRepository {
        OpenRepository {
            path: PathBuf::from(format!("/tmp/{name}")),
            head: None,
            commits,
            has_more_commits: false,
        }
    }

    fn changeset_with(commit_sha: &str, files: Vec<ChangedFile>) -> ChangeSet {
        ChangeSet {
            commit_sha: commit_sha.to_string(),
            base_sha: None,
            files,
        }
    }

    fn file_with(added: usize, removed: usize) -> ChangedFile {
        ChangedFile {
            path: "file.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            is_binary: false,
            line_stats: LineStats { added, removed },
        }
    }

    fn commit_with(sha: &str, summary: &str) -> CommitInfo {
        CommitInfo {
            sha: sha.to_string(),
            short_sha: short_sha(sha),
            summary: summary.to_string(),
            author: "Tester".to_string(),
            authored_timestamp: 0,
            authored_date: "2026-06-07".to_string(),
            parent_shas: Vec::new(),
            branch_names: Vec::new(),
            parent_count: 0,
            is_head: false,
        }
    }

    #[test]
    fn pill_label_for_single_commit_is_the_short_sha() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(context_pill_label(&selection, &changeset), "abcdef1");
    }

    #[test]
    fn pill_label_for_range_appends_commit_count() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "1111111111111111".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(
            context_pill_label(&selection, &changeset),
            "abcdef1 · 3 commits"
        );
    }

    #[test]
    fn range_endpoints_are_oldest_then_newest() {
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(
            range_endpoints(&selection),
            Some(("0000000".to_string(), "abcdef1".to_string()))
        );
    }

    #[test]
    fn range_endpoints_is_none_for_single_commit() {
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(range_endpoints(&selection), None);
    }

    #[test]
    fn header_title_for_range_counts_commits() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "1111111111111111".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(
            popover_header_title(&selection, &changeset, &[]),
            "Reviewing 3 commits"
        );
    }

    #[test]
    fn header_title_for_single_commit_uses_the_summary() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        let commits = vec![commit_with("abcdef1234567890", "feat: do the thing")];
        assert_eq!(
            popover_header_title(&selection, &changeset, &commits),
            "feat: do the thing"
        );
    }

    #[test]
    fn header_title_falls_back_to_short_sha_when_commit_missing() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(popover_header_title(&selection, &changeset, &[]), "abcdef1");
    }

    #[test]
    fn line_totals_sum_every_file() {
        let changeset =
            changeset_with("abcdef1234567890", vec![file_with(10, 2), file_with(5, 95)]);
        assert_eq!(changeset_line_totals(&changeset), (15, 97));
    }

    #[gpui::test]
    async fn pill_is_hidden_in_graph_mode(cx: &mut TestAppContext) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![]),
                };
                app.review_screen = ReviewScreen::Graph;
                cx.notify();
            })
            .expect("set graph state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(PILL).is_none());
    }

    #[gpui::test]
    async fn pill_is_shown_in_changeset_mode(cx: &mut TestAppContext) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let changeset = changeset_with("abcdef1234567890", vec![file_with(3, 1)]);
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![commit_with("abcdef1234567890", "feat: thing")]),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: "abcdef1234567890".to_string(),
                    changeset,
                };
                app.selection = Selection::Single {
                    sha: "abcdef1234567890".to_string(),
                };
                cx.notify();
            })
            .expect("set changeset state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(PILL).is_some());
    }

    const POPOVER: &str = "title-bar-context-popover";
    const CLOSE: &str = "title-bar-context-close";

    fn open_changeset_window(cx: &mut TestAppContext) -> WindowHandle<App> {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let changeset = changeset_with("abcdef1234567890", vec![file_with(3, 1)]);
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![commit_with("abcdef1234567890", "feat: thing")]),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: "abcdef1234567890".to_string(),
                    changeset,
                };
                app.selection = Selection::Single {
                    sha: "abcdef1234567890".to_string(),
                };
                cx.notify();
            })
            .expect("set changeset state");
        window
    }

    #[gpui::test]
    async fn clicking_pill_opens_the_popover(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual.debug_bounds(PILL).expect("pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        assert!(visual.debug_bounds(POPOVER).is_some());
        window
            .read_with(cx, |app, _cx| assert!(app.context_popover_open))
            .expect("read open state");
    }

    #[gpui::test]
    async fn close_button_closes_the_changeset(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual.debug_bounds(PILL).expect("pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        let close = visual.debug_bounds(CLOSE).expect("close button bounds");
        visual.simulate_click(close.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert!(!app.context_popover_open);
                assert!(matches!(app.review_screen, ReviewScreen::Graph));
            })
            .expect("read closed state");
    }
}
