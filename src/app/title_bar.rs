//! Window-chrome title bar: the `{repo} / {sha}` context segment shown in
//! changeset mode and the popover it opens. See
//! docs/specs/review/workflow.md and
//! docs/superpowers/specs/2026-06-07-titlebar-context-switcher-design.md.

use gpui::{
    div, px, AnyElement, Context, Div, Hsla, InteractiveElement, IntoElement, ParentElement,
    Stateful, StatefulInteractiveElement as _, Styled,
};
use gpui_component::{TitleBar, TITLE_BAR_HEIGHT};

use super::{App, Mode, ReviewScreen, Selection, MONO_FONT_FAMILY};
use crate::repo::{ChangeKind, ChangeSet, CommitInfo};
use crate::theme::palette;

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

/// Per-kind file counts in the changeset, as (added, modified, deleted, renamed).
fn changeset_kind_counts(changeset: &ChangeSet) -> (usize, usize, usize, usize) {
    let mut counts = (0, 0, 0, 0);
    for file in &changeset.files {
        match file.kind {
            ChangeKind::Added => counts.0 += 1,
            ChangeKind::Modified => counts.1 += 1,
            ChangeKind::Deleted => counts.2 += 1,
            ChangeKind::Renamed => counts.3 += 1,
        }
    }
    counts
}

/// Short sha + summary for each commit in the changeset, newest first. The
/// summary is empty when the commit is not in the loaded history window.
fn popover_commit_rows(
    selection: &Selection,
    changeset: &ChangeSet,
    commits: &[CommitInfo],
) -> Vec<(String, String)> {
    let shas: Vec<String> = match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => shas.clone(),
        Selection::Single { sha } => vec![sha.clone()],
        // Unreachable from render_context_popover (a changeset always implies a
        // Single or Range selection); handled defensively. The single-element
        // result is suppressed by the `> 1` gate at the call site.
        _ => vec![changeset.commit_sha.clone()],
    };
    shas.into_iter()
        .map(|sha| {
            let summary = commits
                .iter()
                .find(|commit| commit.sha == sha)
                .map(|commit| commit.summary.clone())
                .unwrap_or_default();
            (short_sha(&sha), summary)
        })
        .collect()
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

/// Shared chrome for the two window-chrome switchers (repo and diff context).
/// At rest the switcher is plain text; on hover a subtle neutral pill appears,
/// and while its popover is `open` a stronger fill marks it as pressed. The
/// caller adds its own `on_click` handler and child label.
fn switcher_pill(id: &'static str, text_color: Hsla, open: bool) -> Stateful<Div> {
    let hover_bg = palette().element_hover;
    let active_bg = palette().element_bg;
    let pill = div()
        .id(id)
        .debug_selector(move || id.to_string())
        .font_family(MONO_FONT_FAMILY)
        .text_size(px(13.))
        .text_color(text_color)
        .px_2()
        .py_0p5()
        .rounded_md()
        .cursor_pointer();
    if open {
        pill.bg(active_bg).hover(move |s| s.bg(active_bg))
    } else {
        pill.hover(move |s| s.bg(hover_bg))
    }
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
                    switcher_pill("title-bar-repo", palette().text, self.repo_switcher_open)
                        .on_click(cx.listener(|app, _event, _window, cx| {
                            app.repo_switcher_open = !app.repo_switcher_open;
                            app.context_popover_open = false;
                            cx.notify();
                        }))
                        .child(repo_name),
                );

                if let ReviewScreen::Changeset { changeset, .. } = &self.review_screen {
                    let label = context_pill_label(&self.selection, changeset);
                    row = row
                        .child(
                            div()
                                .mx_2()
                                .text_size(px(13.))
                                .text_color(palette().text_muted)
                                .child("/"),
                        )
                        .child(
                            switcher_pill(
                                "title-bar-context",
                                palette().accent,
                                self.context_popover_open,
                            )
                            .on_click(cx.listener(|app, _event, _window, cx| {
                                app.context_popover_open = !app.context_popover_open;
                                app.repo_switcher_open = false;
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
        let (added_files, modified_files, deleted_files, renamed_files) =
            changeset_kind_counts(changeset);
        let mut kind_parts: Vec<String> = Vec::new();
        if added_files > 0 {
            kind_parts.push(format!("{added_files} added"));
        }
        if modified_files > 0 {
            kind_parts.push(format!("{modified_files} modified"));
        }
        if deleted_files > 0 {
            kind_parts.push(format!("{deleted_files} deleted"));
        }
        if renamed_files > 0 {
            kind_parts.push(format!("{renamed_files} renamed"));
        }
        let commit_rows = popover_commit_rows(&self.selection, changeset, &repo.commits);

        let mut header = div().flex().flex_col().gap_1().p_3().child(
            div()
                .text_size(px(13.))
                .text_color(palette().text)
                .child(title),
        );
        if let Some((oldest, newest)) = endpoints {
            header = header.child(
                div()
                    .font_family(MONO_FONT_FAMILY)
                    .text_size(px(12.))
                    .text_color(palette().text_muted)
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
                .child(
                    div()
                        .text_color(palette().text_muted)
                        .child(label.to_string()),
                )
                .child(value)
        };

        let files_row = stat_row(
            "Files changed",
            div()
                .text_color(palette().text)
                .child(file_count.to_string())
                .into_any_element(),
        );
        let lines_row = stat_row(
            "Lines",
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .text_color(palette().diff_added_fg)
                        .child(format!("+{added}")),
                )
                .child(
                    div()
                        .text_color(palette().diff_removed_fg)
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
            .border_color(palette().danger_border)
            .bg(palette().danger_bg)
            .text_size(px(12.))
            .text_color(palette().danger_fg)
            .cursor_pointer()
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.close_changeset(cx);
            }))
            .child("Close changeset");

        let kind_row = (!kind_parts.is_empty()).then(|| {
            div()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(palette().border)
                .text_size(px(12.))
                .text_color(palette().text_muted)
                .child(kind_parts.join(" \u{00b7} "))
        });

        // For a range, list the commits (newest first) with their summaries so
        // reviewers see real context, not bare hashes. A single commit's summary
        // is already the header title, so its one-row list is omitted.
        let commit_list = (commit_rows.len() > 1).then(|| {
            let mut list = div()
                .id("title-bar-context-commits")
                .flex()
                .flex_col()
                .max_h(px(168.))
                .overflow_y_scroll()
                .border_t_1()
                .border_color(palette().border);
            for (index, (sha, summary)) in commit_rows.iter().enumerate() {
                list = list.child(
                    div()
                        .id(("title-bar-context-commit", index))
                        .debug_selector(move || format!("title-bar-context-commit-{index}"))
                        .flex()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .text_size(px(12.))
                        .child(
                            div()
                                .font_family(MONO_FONT_FAMILY)
                                .text_color(palette().accent)
                                .child(sha.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .text_color(palette().text)
                                .child(summary.clone()),
                        ),
                );
            }
            list
        });

        let card = div()
            .absolute()
            .top(TITLE_BAR_HEIGHT)
            .left(px(80.))
            .occlude()
            .w(px(380.))
            .bg(palette().surface)
            .border_1()
            .border_color(palette().border)
            .rounded_lg()
            .debug_selector(|| "title-bar-context-popover".to_string())
            .child(header)
            .child(files_row)
            .child(lines_row)
            .children(kind_row)
            .children(commit_list)
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

    /// The repo switcher popover, shown when the repo name is active. Lists the
    /// git repositories sitting alongside the open one in its parent folder
    /// (the current repo marked), with an `Open repository…` escape hatch to the
    /// folder picker. Returns `None` unless a repository is open and the switcher
    /// is toggled on. Like the context popover, it is a full-window overlay: a
    /// transparent backdrop that dismisses on outside click, plus the card.
    pub(crate) fn render_repo_switcher(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.repo_switcher_open {
            return None;
        }
        let Mode::RepoOpen { repo } = &self.mode else {
            return None;
        };

        let current = repo.path.clone();
        let parent_label = current
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        let siblings = crate::repo::sibling_repositories(&current);
        let other_count = siblings.iter().filter(|path| **path != current).count();

        let header = div()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(palette().border)
            .font_family(MONO_FONT_FAMILY)
            .text_size(px(12.))
            .text_color(palette().text_muted)
            .child(parent_label);

        let body = if other_count == 0 {
            div()
                .id("title-bar-repo-switcher-empty")
                .debug_selector(|| "title-bar-repo-switcher-empty".to_string())
                .px_3()
                .py_2()
                .text_size(px(12.))
                .text_color(palette().text_muted)
                .child("No other repositories in this folder.")
                .into_any_element()
        } else {
            let mut list = div().flex().flex_col();
            for (index, path) in siblings.iter().enumerate() {
                let is_current = *path == current;
                let name = super::repository_title(path);
                let mut row = div()
                    .id(("title-bar-repo-sibling", index))
                    .debug_selector(move || format!("title-bar-repo-sibling-{index}"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .text_size(px(12.))
                    .child(
                        div()
                            .font_family(MONO_FONT_FAMILY)
                            .text_color(palette().text)
                            .child(name),
                    );

                if is_current {
                    row = row.child(
                        div()
                            .debug_selector(|| "title-bar-repo-current".to_string())
                            .text_size(px(11.))
                            .text_color(palette().text_muted)
                            .child("current"),
                    );
                } else {
                    let open_path = path.clone();
                    row = row.cursor_pointer().on_click(cx.listener(
                        move |app, _event, window, cx| {
                            app.open_repository_at(open_path.clone(), window, cx);
                        },
                    ));
                }

                list = list.child(row);
            }
            list.into_any_element()
        };

        let open = div()
            .id("title-bar-repo-open")
            .debug_selector(|| "title-bar-repo-open".to_string())
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(palette().border)
            .text_size(px(12.))
            .text_color(palette().accent)
            .cursor_pointer()
            .on_click(cx.listener(|app, _event, window, cx| {
                app.repo_switcher_open = false;
                app.prompt_and_open_repository(window, cx);
            }))
            .child("Open repository\u{2026}");

        let card = div()
            .absolute()
            .top(TITLE_BAR_HEIGHT)
            .left(px(8.))
            .occlude()
            .w(px(320.))
            .bg(palette().surface)
            .border_1()
            .border_color(palette().border)
            .rounded_lg()
            .debug_selector(|| "title-bar-repo-switcher".to_string())
            .child(header)
            .child(body)
            .child(open);

        let backdrop = div()
            .id("title-bar-repo-switcher-backdrop")
            .absolute()
            .inset_0()
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.repo_switcher_open = false;
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
    use std::path::{Path, PathBuf};

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
            branches: Vec::new(),
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
            branch_labels: Vec::new(),
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

    #[test]
    fn kind_counts_tally_each_change_kind() {
        let mut changeset = changeset_with("abcdef1234567890", vec![]);
        changeset.files = vec![
            ChangedFile {
                path: "a.rs".to_string(),
                old_path: None,
                kind: ChangeKind::Added,
                is_binary: false,
                line_stats: LineStats {
                    added: 1,
                    removed: 0,
                },
            },
            ChangedFile {
                path: "b.rs".to_string(),
                old_path: None,
                kind: ChangeKind::Modified,
                is_binary: false,
                line_stats: LineStats {
                    added: 1,
                    removed: 1,
                },
            },
            ChangedFile {
                path: "c.rs".to_string(),
                old_path: None,
                kind: ChangeKind::Modified,
                is_binary: false,
                line_stats: LineStats {
                    added: 0,
                    removed: 2,
                },
            },
            ChangedFile {
                path: "d.rs".to_string(),
                old_path: None,
                kind: ChangeKind::Deleted,
                is_binary: false,
                line_stats: LineStats {
                    added: 0,
                    removed: 5,
                },
            },
            ChangedFile {
                path: "e.rs".to_string(),
                old_path: Some("old_e.rs".to_string()),
                kind: ChangeKind::Renamed,
                is_binary: false,
                line_stats: LineStats {
                    added: 0,
                    removed: 0,
                },
            },
        ];
        assert_eq!(changeset_kind_counts(&changeset), (1, 2, 1, 1));
    }

    #[test]
    fn commit_rows_preserve_selection_order_with_summaries() {
        let changeset = changeset_with("aaaaaaa1111111", vec![]);
        let selection = Selection::Range {
            start_sha: "ccccccc3333333".to_string(),
            end_sha: "aaaaaaa1111111".to_string(),
            shas: vec![
                "aaaaaaa1111111".to_string(),
                "bbbbbbb2222222".to_string(),
                "ccccccc3333333".to_string(),
            ],
        };
        let commits = vec![
            commit_with("aaaaaaa1111111", "feat: newest"),
            commit_with("bbbbbbb2222222", "fix: middle"),
            commit_with("ccccccc3333333", "chore: oldest"),
        ];
        assert_eq!(
            popover_commit_rows(&selection, &changeset, &commits),
            vec![
                ("aaaaaaa".to_string(), "feat: newest".to_string()),
                ("bbbbbbb".to_string(), "fix: middle".to_string()),
                ("ccccccc".to_string(), "chore: oldest".to_string()),
            ]
        );
    }

    #[test]
    fn commit_rows_use_empty_summary_when_commit_not_loaded() {
        let changeset = changeset_with("aaaaaaa1111111", vec![]);
        let selection = Selection::Range {
            start_sha: "bbbbbbb2222222".to_string(),
            end_sha: "aaaaaaa1111111".to_string(),
            shas: vec!["aaaaaaa1111111".to_string(), "bbbbbbb2222222".to_string()],
        };
        // Only the newest commit is loaded.
        let commits = vec![commit_with("aaaaaaa1111111", "feat: newest")];
        assert_eq!(
            popover_commit_rows(&selection, &changeset, &commits),
            vec![
                ("aaaaaaa".to_string(), "feat: newest".to_string()),
                ("bbbbbbb".to_string(), String::new()),
            ]
        );
    }

    #[gpui::test]
    async fn popover_lists_range_commits(cx: &mut TestAppContext) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let changeset = changeset_with("aaaaaaa1111111", vec![file_with(3, 1)]);
                app.mode = Mode::RepoOpen {
                    repo: repo_named(
                        "Demo",
                        vec![
                            commit_with("aaaaaaa1111111", "feat: newest"),
                            commit_with("bbbbbbb2222222", "fix: oldest"),
                        ],
                    ),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: "aaaaaaa1111111".to_string(),
                    changeset,
                };
                app.selection = Selection::Range {
                    start_sha: "bbbbbbb2222222".to_string(),
                    end_sha: "aaaaaaa1111111".to_string(),
                    shas: vec!["aaaaaaa1111111".to_string(), "bbbbbbb2222222".to_string()],
                };
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("set range changeset state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("title-bar-context-popover").is_some());
        assert!(visual.debug_bounds("title-bar-context-commit-0").is_some());
        assert!(visual.debug_bounds("title-bar-context-commit-1").is_some());
    }

    const REPO_NAME: &str = "title-bar-repo";
    const SWITCHER: &str = "title-bar-repo-switcher";

    /// Create a parent directory containing one git repository per name and
    /// return the parent (kept alive) plus the canonicalized repo paths.
    fn parent_with_repos(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let parent = tempfile::tempdir().expect("create parent tempdir");
        let paths = names
            .iter()
            .map(|name| {
                let path = parent.path().join(name);
                git2::Repository::init(&path).expect("init sibling repo");
                path.canonicalize().expect("canonicalize repo path")
            })
            .collect();
        (parent, paths)
    }

    /// Open `path` (a real on-disk repository) in a fresh app window.
    fn window_with_repo_open(cx: &mut TestAppContext, path: &Path) -> WindowHandle<App> {
        let window = app_window(cx);
        let repo = crate::repo::open_at(path).expect("open repo");
        window
            .update(cx, |app, _window, cx| {
                app.mode = Mode::RepoOpen { repo };
                app.review_screen = ReviewScreen::Graph;
                cx.notify();
            })
            .expect("set repo-open state");
        window
    }

    #[gpui::test]
    async fn repo_name_is_present_in_graph_mode(cx: &mut TestAppContext) {
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
        assert!(visual.debug_bounds(REPO_NAME).is_some());
    }

    #[gpui::test]
    async fn clicking_repo_name_opens_the_switcher(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let name = visual.debug_bounds(REPO_NAME).expect("repo name bounds");
        visual.simulate_click(name.center(), Modifiers::none());

        assert!(visual.debug_bounds(SWITCHER).is_some());
        window
            .read_with(cx, |app, _cx| assert!(app.repo_switcher_open))
            .expect("read switcher open state");
    }

    #[gpui::test]
    async fn switcher_lists_siblings_and_marks_the_current_repo(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["alpha", "beta"]);
        // Open beta; alpha is its sibling.
        let beta = paths.into_iter().find(|p| p.ends_with("beta")).unwrap();
        let window = window_with_repo_open(cx, &beta);
        window
            .update(cx, |app, _window, cx| {
                app.repo_switcher_open = true;
                cx.notify();
            })
            .expect("open switcher");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(SWITCHER).is_some());
        // Two siblings (alpha, beta) sorted → rows 0 and 1.
        assert!(visual.debug_bounds("title-bar-repo-sibling-0").is_some());
        assert!(visual.debug_bounds("title-bar-repo-sibling-1").is_some());
        // The current repo (beta) carries a marker.
        assert!(visual.debug_bounds("title-bar-repo-current").is_some());
    }

    #[gpui::test]
    async fn clicking_a_sibling_switches_the_repo(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["alpha", "beta"]);
        let alpha = paths.iter().find(|p| p.ends_with("alpha")).unwrap().clone();
        let beta = paths.iter().find(|p| p.ends_with("beta")).unwrap().clone();
        let window = window_with_repo_open(cx, &beta);
        window
            .update(cx, |app, _window, cx| {
                app.repo_switcher_open = true;
                cx.notify();
            })
            .expect("open switcher");
        cx.run_until_parked();

        // alpha sorts first → row 0.
        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("title-bar-repo-sibling-0")
            .expect("alpha row bounds");
        visual.simulate_click(row.center(), Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.mode {
                Mode::RepoOpen { repo } => assert_eq!(repo.path, alpha),
                Mode::NoRepo => panic!("expected a repo to be open"),
            })
            .expect("read switched repo");
    }

    #[gpui::test]
    async fn switcher_shows_empty_state_when_no_other_repos(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        window
            .update(cx, |app, _window, cx| {
                app.repo_switcher_open = true;
                cx.notify();
            })
            .expect("open switcher");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual
            .debug_bounds("title-bar-repo-switcher-empty")
            .is_some());
        assert!(visual.debug_bounds("title-bar-repo-sibling-1").is_none());
    }

    #[gpui::test]
    async fn switcher_has_an_open_repository_footer(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        window
            .update(cx, |app, _window, cx| {
                app.repo_switcher_open = true;
                cx.notify();
            })
            .expect("open switcher");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("title-bar-repo-open").is_some());
    }

    #[gpui::test]
    async fn opening_the_switcher_closes_the_context_popover(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        window
            .update(cx, |app, _window, cx| {
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("open context popover");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let name = visual.debug_bounds(REPO_NAME).expect("repo name bounds");
        visual.simulate_click(name.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert!(app.repo_switcher_open);
                assert!(!app.context_popover_open);
            })
            .expect("read popover states");
    }
}
