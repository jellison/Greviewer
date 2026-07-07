//! Window-chrome title bar: the `{repo} / {sha}` context segment shown in
//! changeset mode and the popover it opens. See
//! docs/specs/review/workflow.md and
//! docs/superpowers/specs/2026-06-07-titlebar-context-switcher-design.md.

use gpui::{
    div, px, AnyElement, Context, Div, Hsla, InteractiveElement, IntoElement, ParentElement,
    Stateful, StatefulInteractiveElement as _, Styled,
};
use gpui_component::input::Input;
use gpui_component::{Icon, TitleBar, TITLE_BAR_HEIGHT};

use super::{short_sha, worktree_title, App, Mode, ReviewScreen, Selection, MONO_FONT_FAMILY};
use crate::icons::LucideIcon;
use crate::repo::{ChangeKind, ChangeSet, CommitInfo};
use crate::theme::palette;

/// Number of commits the changeset reviews: the range length for a range,
/// otherwise one. Comparisons render their own labels and never ask.
fn reviewed_commit_count(selection: &Selection) -> usize {
    match selection {
        Selection::Range { shas, .. } if !shas.is_empty() => shas.len(),
        _ => 1,
    }
}

/// "1 commit" / "N commits".
fn commit_count_label(count: usize) -> String {
    if count == 1 {
        "1 commit".to_string()
    } else {
        format!("{count} commits")
    }
}

/// Label for the title-bar pill. Single commits and ranges show the newest
/// short sha plus the commit count; a comparison shows both endpoints in
/// git's three-dot notation.
fn context_pill_label(selection: &Selection, changeset: &ChangeSet) -> String {
    match selection {
        Selection::Pending => "pending".to_string(),
        Selection::Compare {
            base_sha,
            target_sha,
        } => format!("{}...{}", short_sha(base_sha), short_sha(target_sha)),
        // In a well-formed call `changeset.commit_sha` is the newest commit,
        // i.e. `shas[0]` for a range, so the pill always shows the newest
        // short sha.
        _ => format!(
            "{} · {}",
            short_sha(&changeset.commit_sha),
            commit_count_label(reviewed_commit_count(selection))
        ),
    }
}

/// The header's mono identifier line: `oldest … newest` short shas for a
/// range (`shas` is newest-first), the single short sha for a single commit,
/// `None` for a comparison (which shows its merge-base line instead).
fn endpoints_line(selection: &Selection, changeset: &ChangeSet) -> Option<String> {
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            let newest = short_sha(shas.first()?);
            let oldest = short_sha(shas.last()?);
            Some(format!("{oldest} \u{2026} {newest}"))
        }
        Selection::Compare { .. } | Selection::Pending => None,
        _ => Some(short_sha(&changeset.commit_sha)),
    }
}

/// Popover header title. Single commits and ranges read "Reviewing
/// {count}"; a comparison states the merge-preview direction.
fn popover_header_title(selection: &Selection) -> String {
    match selection {
        Selection::Pending => "Reviewing pending changes".to_string(),
        Selection::Compare {
            base_sha,
            target_sha,
        } => format!(
            "Merge preview: {} into {}",
            short_sha(target_sha),
            short_sha(base_sha)
        ),
        _ => format!(
            "Reviewing {}",
            commit_count_label(reviewed_commit_count(selection))
        ),
    }
}

/// The comparison's common-ancestor line for the popover header; `None` for
/// non-comparison changesets. An opened comparison always carries the merge
/// base as its `base_sha`.
fn comparison_merge_base_line(selection: &Selection, changeset: &ChangeSet) -> Option<String> {
    match selection {
        Selection::Compare { .. } => changeset
            .base_sha
            .as_deref()
            .map(|base| format!("vs merge base {}", short_sha(base))),
        _ => None,
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
/// `comparison_shas` is the commits a comparison would merge in, captured
/// when the changeset opened.
fn popover_commit_rows(
    selection: &Selection,
    changeset: &ChangeSet,
    commits: &[CommitInfo],
    comparison_shas: Option<&[String]>,
) -> Vec<(String, String)> {
    let shas: Vec<String> = match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => shas.clone(),
        Selection::Single { sha } => vec![sha.clone()],
        Selection::Compare { .. } => comparison_shas.map(<[String]>::to_vec).unwrap_or_default(),
        // Pending has no discrete commits to list.
        Selection::Pending => Vec::new(),
        // Unreachable from render_context_popover (a changeset always implies
        // a Single, Range, Compare, or Pending selection); handled defensively.
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

/// Compact path label for a switcher row: home-relative (`~/…`) when the
/// path is under `$HOME`, and leading-ellipsized to the trailing `max_chars`
/// characters when it is still too long — the tail of a worktree path is the
/// part that identifies it.
fn abbreviated_path(path: &std::path::Path, max_chars: usize) -> String {
    let home = std::env::var_os("HOME").map(|home| home.to_string_lossy().into_owned());
    abbreviated_path_with_home(path, home.as_deref(), max_chars)
}

/// Core of `abbreviated_path`, parameterized on the home directory so it is
/// unit-testable without touching the process environment. Only substitutes
/// `~` when `home` is a whole-component prefix of the path: the remainder
/// after stripping must be empty or start with a path separator, so a HOME of
/// `/Users/je` does not match inside `/Users/jellison/x`.
fn abbreviated_path_with_home(
    path: &std::path::Path,
    home: Option<&str>,
    max_chars: usize,
) -> String {
    let display = path.display().to_string();
    let display = match home {
        Some(home) if !home.is_empty() => {
            let home = home.strip_suffix('/').unwrap_or(home);
            match display.strip_prefix(home) {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
                _ => display,
            }
        }
        _ => display,
    };
    let chars: Vec<char> = display.chars().collect();
    if chars.len() <= max_chars {
        display
    } else {
        let tail: String = chars[chars.len() - max_chars..].iter().collect();
        format!("\u{2026}{tail}")
    }
}

impl App {
    /// The window-chrome title bar. Always shows the repo name when a
    /// repository is open; in changeset mode it also shows the clickable
    /// context pill that opens the diff popover.
    pub(crate) fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.mode {
            Mode::RepoOpen { repo } => {
                let repo_name = super::repository_title(&repo.main_path);
                let mut row = div()
                    .flex()
                    .items_center()
                    .child(
                        switcher_pill("title-bar-repo", palette().text, self.repo_switcher_open)
                            .on_click(cx.listener(|app, _event, _window, cx| {
                                app.repo_switcher_open = !app.repo_switcher_open;
                                app.context_popover_open = false;
                                app.worktree_switcher_open = false;
                                cx.notify();
                            }))
                            .child(repo_name),
                    )
                    .child(
                        div()
                            .mx_0p5()
                            .text_size(px(13.))
                            .text_color(palette().text_muted)
                            .child("/"),
                    )
                    .child(
                        switcher_pill(
                            "title-bar-worktree",
                            palette().text,
                            self.worktree_switcher_open,
                        )
                        .on_click(cx.listener(|app, _event, window, cx| {
                            let opening = !app.worktree_switcher_open;
                            if opening {
                                app.refresh_worktree_entries(window, cx);
                            }
                            app.worktree_switcher_open = opening;
                            app.repo_switcher_open = false;
                            app.context_popover_open = false;
                            cx.notify();
                        }))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            Icon::new(LucideIcon::GitBranchPlus)
                                .text_color(palette().text_muted)
                                .size(px(13.)),
                        )
                        .child(worktree_title(repo)),
                    );

                if let ReviewScreen::Changeset { changeset, .. } = &self.review_screen {
                    let label = context_pill_label(&self.selection, changeset);
                    let review_name = self.current_review().map(|review| review.name.clone());
                    row = row
                        .child(
                            div()
                                .mx_0p5()
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
                            .on_click(cx.listener(|app, _event, window, cx| {
                                app.context_popover_open = !app.context_popover_open;
                                app.repo_switcher_open = false;
                                app.worktree_switcher_open = false;
                                let name = app
                                    .current_review()
                                    .map(|review| review.name.clone())
                                    .unwrap_or_default();
                                app.review_name_input
                                    .update(cx, |state, cx| state.set_value(name, window, cx));
                                app.pending_delete_review = None;
                                cx.notify();
                            }))
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                Icon::new(LucideIcon::GitCompareArrows)
                                    .text_color(palette().text_muted)
                                    .size(px(13.)),
                            )
                            .children(review_name.map(|name| {
                                div()
                                    .debug_selector(|| "title-bar-review-name".to_string())
                                    .whitespace_nowrap()
                                    .child(format!("{name} \u{00b7}"))
                            }))
                            .child(label)
                            .children(
                                self.pr_badge_for_commit(&changeset.commit_sha).map(|id| {
                                    div()
                                        .debug_selector(|| "title-bar-pr-badge".to_string())
                                        .whitespace_nowrap()
                                        .text_color(palette().text_muted)
                                        .child(format!("\u{00b7} #{id}"))
                                }),
                            ),
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

        let title = popover_header_title(&self.selection);
        let endpoints = endpoints_line(&self.selection, changeset);
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
        let merge_base_line = comparison_merge_base_line(&self.selection, changeset);
        let commit_rows = popover_commit_rows(
            &self.selection,
            changeset,
            &repo.commits,
            self.comparison_commit_shas.as_deref(),
        );

        let mut header = div().flex().flex_col().gap_1().p_3().child(
            div()
                .text_size(px(13.))
                .text_color(palette().text)
                .child(title),
        );
        if let Some(endpoints) = endpoints {
            header = header.child(
                div()
                    .font_family(MONO_FONT_FAMILY)
                    .text_size(px(12.))
                    .text_color(palette().text_muted)
                    .debug_selector(|| "title-bar-context-endpoints".to_string())
                    .child(endpoints),
            );
        }
        if let Some(merge_base_line) = merge_base_line {
            header = header.child(
                div()
                    .font_family(MONO_FONT_FAMILY)
                    .text_size(px(12.))
                    .text_color(palette().text_muted)
                    .debug_selector(|| "title-bar-context-merge-base".to_string())
                    .child(merge_base_line),
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

        // The review row/block: no review controls for the pending changeset
        // (a review is anchored to a discrete changeset, not the working
        // tree); a "Start review" affordance when no review is attached yet;
        // otherwise the full review block (name, dates, complete toggle,
        // delete).
        let review_section: Option<AnyElement> = if matches!(self.selection, Selection::Pending) {
            None
        } else if let Some(review) = self.current_review() {
            let review_id = review.id.clone();
            let completed = review.status == crate::reviews::ReviewStatus::Completed;
            let mut dates = format!(
                "Started {} \u{00b7} Active {}",
                crate::repo::format_unix_date(review.created_at),
                crate::repo::format_unix_date(review.last_activity_at),
            );
            if let Some(completed_at) = review.completed_at {
                dates.push_str(&format!(
                    " \u{00b7} Completed {}",
                    crate::repo::format_unix_date(completed_at)
                ));
            }
            let delete_armed = self.pending_delete_review.as_deref() == Some(review_id.as_str());
            Some(
                div()
                    .debug_selector(|| "review-block".to_string())
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .border_t_1()
                    .border_color(palette().border)
                    .child(
                        div()
                            .debug_selector(|| "review-name-input".to_string())
                            .child(Input::new(&self.review_name_input)),
                    )
                    .child(
                        div()
                            .debug_selector(|| "review-dates".to_string())
                            .text_size(px(11.))
                            .text_color(palette().text_muted)
                            .child(dates),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("review-complete-toggle")
                                    .debug_selector(|| "review-complete-toggle".to_string())
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(palette().border)
                                    .text_size(px(12.))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |app, _event, window, cx| {
                                        let completed =
                                            app.current_review().is_some_and(|review| {
                                                review.status
                                                    == crate::reviews::ReviewStatus::Completed
                                            });
                                        app.set_current_review_completed(!completed, window, cx);
                                    }))
                                    .child(if completed {
                                        "Reopen review"
                                    } else {
                                        "Mark complete"
                                    }),
                            )
                            .child(if delete_armed {
                                div()
                                    .id("review-delete-armed")
                                    .debug_selector(|| "review-delete-armed".to_string())
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(palette().danger_border)
                                    .bg(palette().danger_bg)
                                    .text_color(palette().danger_fg)
                                    .text_size(px(12.))
                                    .cursor_pointer()
                                    .on_click(cx.listener({
                                        let review_id = review_id.clone();
                                        move |app, _event, window, cx| {
                                            app.delete_review(&review_id, window, cx);
                                        }
                                    }))
                                    .child("Confirm delete")
                            } else {
                                div()
                                    .id("review-delete")
                                    .debug_selector(|| "review-delete".to_string())
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(palette().border)
                                    .text_color(palette().danger_fg)
                                    .text_size(px(12.))
                                    .cursor_pointer()
                                    .on_click(cx.listener({
                                        let review_id = review_id.clone();
                                        move |app, _event, _window, cx| {
                                            app.pending_delete_review = Some(review_id.clone());
                                            cx.notify();
                                        }
                                    }))
                                    .child("Delete review\u{2026}")
                            }),
                    )
                    .into_any_element(),
            )
        } else {
            Some(
                div()
                    .id("review-start")
                    .debug_selector(|| "review-start".to_string())
                    .mx_2()
                    .mt_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .border_color(palette().border)
                    .text_size(px(12.))
                    .text_color(palette().accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|app, _event, window, cx| {
                        app.start_review(window, cx);
                        let name = app
                            .current_review()
                            .map(|review| review.name.clone())
                            .unwrap_or_default();
                        app.review_name_input
                            .update(cx, |state, cx| state.set_value(name, window, cx));
                    }))
                    .child("Start review")
                    .into_any_element(),
            )
        };

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

        // List every reviewed commit (newest first) with its summary so
        // reviewers see real context, not bare hashes. Empty only for a
        // defensive comparison whose introduced commits were not captured.
        let commit_list = (!commit_rows.is_empty()).then(|| {
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
            .children(review_section)
            .child(close);

        let backdrop = div()
            .id("title-bar-context-backdrop")
            .absolute()
            // See the worktree switcher's backdrop for why this starts below
            // the title bar rather than covering the whole window.
            .top(TITLE_BAR_HEIGHT)
            .bottom(px(0.))
            .left(px(0.))
            .right(px(0.))
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
    ///
    /// Anchored on the repository's main worktree, not the open path: with a
    /// linked worktree open, the repo pill and window title already read the
    /// main worktree's name (see `render_title_bar`), so the sibling scan and
    /// the current-repo marker must agree with that identity rather than the
    /// linked worktree's own directory.
    pub(crate) fn render_repo_switcher(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.repo_switcher_open {
            return None;
        }
        let Mode::RepoOpen { repo } = &self.mode else {
            return None;
        };

        let current = repo.main_path.clone();
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
            // See the worktree switcher's backdrop for why this starts below
            // the title bar rather than covering the whole window.
            .top(TITLE_BAR_HEIGHT)
            .bottom(px(0.))
            .left(px(0.))
            .right(px(0.))
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

    /// The worktree switcher popover, shown when the worktree pill is active.
    /// Lists the snapshot in `worktree_entries` (refreshed when the pill
    /// opened the popover): the main worktree first, each row naming the
    /// worktree with its branch, short sha, and abbreviated path, the current
    /// row marked. Selecting another worktree opens it exactly like a
    /// repository switch. Full-window overlay like the other two popovers.
    pub(crate) fn render_worktree_switcher(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.worktree_switcher_open {
            return None;
        }
        let Mode::RepoOpen { .. } = &self.mode else {
            return None;
        };

        let mut list = div().flex().flex_col().py_1();
        for (index, entry) in self.worktree_entries.iter().enumerate() {
            let title = if entry.is_main {
                "main worktree".to_string()
            } else {
                entry.name.clone()
            };
            let mut sub_parts: Vec<String> = Vec::new();
            if let Some(branch) = &entry.branch {
                sub_parts.push(branch.clone());
            }
            if let Some(sha) = &entry.short_sha {
                sub_parts.push(sha.clone());
            }
            sub_parts.push(abbreviated_path(&entry.path, 36));
            let subline = sub_parts.join(" \u{2022} ");

            let mut row = div()
                .id(("title-bar-worktree-row", index))
                .debug_selector(move || format!("title-bar-worktree-row-{index}"))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py_1()
                .cursor_pointer()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            div()
                                .font_family(MONO_FONT_FAMILY)
                                .text_size(px(12.))
                                .text_color(palette().text)
                                .child(title),
                        )
                        .child(
                            div()
                                .font_family(MONO_FONT_FAMILY)
                                .text_size(px(11.))
                                .text_color(palette().text_muted)
                                .child(subline),
                        ),
                );

            if entry.is_current {
                row = row
                    .child(
                        div()
                            .debug_selector(|| "title-bar-worktree-current".to_string())
                            .text_size(px(11.))
                            .text_color(palette().text_muted)
                            .child("\u{2713}"),
                    )
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.worktree_switcher_open = false;
                        cx.notify();
                    }));
            } else {
                let open_path = entry.path.clone();
                row = row.on_click(cx.listener(move |app, _event, window, cx| {
                    app.worktree_switcher_open = false;
                    app.open_repository_at(open_path.clone(), window, cx);
                }));
            }

            list = list.child(row);
        }

        let card = div()
            .absolute()
            .top(TITLE_BAR_HEIGHT)
            .left(px(44.))
            .occlude()
            .w(px(360.))
            .bg(palette().surface)
            .border_1()
            .border_color(palette().border)
            .rounded_lg()
            .debug_selector(|| "title-bar-worktree-switcher".to_string())
            .child(list);

        let backdrop = div()
            .id("title-bar-worktree-switcher-backdrop")
            .absolute()
            // The backdrop deliberately starts below the title bar rather
            // than covering the whole window: every pill that opens one of
            // these popovers lives inside the title bar strip, so a backdrop
            // that also covered that strip would sit on top of every pill's
            // own click handler. That caused a second click on this same
            // pill to first close the popover here and then immediately
            // reopen it in the pill's own toggle handler (both handlers see
            // the click), and made pills for the *other* popovers
            // unreachable while this one was open.
            .top(TITLE_BAR_HEIGHT)
            .bottom(px(0.))
            .left(px(0.))
            .right(px(0.))
            .on_click(cx.listener(|app, _event, _window, cx| {
                app.worktree_switcher_open = false;
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
        let path = PathBuf::from(format!("/tmp/{name}"));
        OpenRepository {
            main_path: path.clone(),
            path,
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
    fn pill_label_for_single_commit_appends_the_singular_count() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(
            context_pill_label(&selection, &changeset),
            "abcdef1 · 1 commit"
        );
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
    fn endpoints_line_for_range_reads_oldest_then_newest() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(
            endpoints_line(&selection, &changeset),
            Some("0000000 \u{2026} abcdef1".to_string())
        );
    }

    #[test]
    fn endpoints_line_for_single_commit_is_its_short_sha_once() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(
            endpoints_line(&selection, &changeset),
            Some("abcdef1".to_string())
        );
    }

    #[test]
    fn endpoints_line_is_none_for_comparisons() {
        let changeset = changeset_with("fedcba9876543210", vec![]);
        let selection = Selection::Compare {
            base_sha: "0123456789abcdef".to_string(),
            target_sha: "fedcba9876543210".to_string(),
        };
        assert_eq!(endpoints_line(&selection, &changeset), None);
    }

    #[test]
    fn header_title_for_range_counts_commits() {
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "1111111111111111".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(popover_header_title(&selection), "Reviewing 3 commits");
    }

    #[test]
    fn header_title_for_single_commit_reads_reviewing_one_commit() {
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(popover_header_title(&selection), "Reviewing 1 commit");
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

    #[gpui::test]
    async fn pill_for_pending_reads_pending(cx: &mut TestAppContext) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let changeset = changeset_with(crate::repo::PENDING_SHA, vec![file_with(3, 1)]);
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![]),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: crate::repo::PENDING_SHA.to_string(),
                    changeset,
                };
                app.selection = Selection::Pending;
                cx.notify();
            })
            .expect("set pending changeset state");

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
            popover_commit_rows(&selection, &changeset, &commits, None),
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
            popover_commit_rows(&selection, &changeset, &commits, None),
            vec![
                ("aaaaaaa".to_string(), "feat: newest".to_string()),
                ("bbbbbbb".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn context_pill_label_for_pending_reads_pending() {
        let changeset = ChangeSet {
            commit_sha: crate::repo::PENDING_SHA.to_string(),
            base_sha: None,
            files: Vec::new(),
        };
        assert_eq!(
            context_pill_label(&Selection::Pending, &changeset),
            "pending"
        );
    }

    #[test]
    fn pill_label_for_comparison_reads_base_dots_target() {
        let changeset = changeset_with("fedcba9876543210", vec![]);
        let selection = Selection::Compare {
            base_sha: "0123456789abcdef".to_string(),
            target_sha: "fedcba9876543210".to_string(),
        };
        assert_eq!(
            context_pill_label(&selection, &changeset),
            "0123456...fedcba9"
        );
    }

    #[test]
    fn header_title_for_comparison_states_direction() {
        let selection = Selection::Compare {
            base_sha: "0123456789abcdef".to_string(),
            target_sha: "fedcba9876543210".to_string(),
        };
        assert_eq!(
            popover_header_title(&selection),
            "Merge preview: fedcba9 into 0123456"
        );
    }

    #[test]
    fn merge_base_line_names_the_common_ancestor_only_for_comparisons() {
        let mut changeset = changeset_with("fedcba9876543210", vec![]);
        changeset.base_sha = Some("9999999888888888".to_string());
        let comparison = Selection::Compare {
            base_sha: "0123456789abcdef".to_string(),
            target_sha: "fedcba9876543210".to_string(),
        };
        assert_eq!(
            comparison_merge_base_line(&comparison, &changeset),
            Some("vs merge base 9999999".to_string())
        );

        let single = Selection::Single {
            sha: "fedcba9876543210".to_string(),
        };
        assert_eq!(comparison_merge_base_line(&single, &changeset), None);
    }

    #[test]
    fn commit_rows_for_comparison_use_the_captured_introduced_commits() {
        let changeset = changeset_with("aaaaaaa1111111", vec![]);
        let selection = Selection::Compare {
            base_sha: "ccccccc3333333".to_string(),
            target_sha: "aaaaaaa1111111".to_string(),
        };
        let commits = vec![commit_with("aaaaaaa1111111", "feat: newest")];
        let introduced = vec!["aaaaaaa1111111".to_string(), "bbbbbbb2222222".to_string()];
        assert_eq!(
            popover_commit_rows(&selection, &changeset, &commits, Some(&introduced)),
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

    #[gpui::test]
    async fn popover_for_comparison_names_the_merge_base_and_lists_introduced_commits(
        cx: &mut TestAppContext,
    ) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let mut changeset = changeset_with("aaaaaaa1111111", vec![file_with(3, 1)]);
                changeset.base_sha = Some("ddddddd4444444".to_string());
                app.mode = Mode::RepoOpen {
                    repo: repo_named(
                        "Demo",
                        vec![
                            commit_with("aaaaaaa1111111", "feat: newest"),
                            commit_with("bbbbbbb2222222", "fix: oldest"),
                            commit_with("ccccccc3333333", "base tip"),
                        ],
                    ),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: "aaaaaaa1111111".to_string(),
                    changeset,
                };
                app.selection = Selection::Compare {
                    base_sha: "ccccccc3333333".to_string(),
                    target_sha: "aaaaaaa1111111".to_string(),
                };
                app.comparison_commit_shas = Some(vec![
                    "aaaaaaa1111111".to_string(),
                    "bbbbbbb2222222".to_string(),
                ]);
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("set comparison changeset state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("title-bar-context-popover").is_some());
        assert!(
            visual
                .debug_bounds("title-bar-context-merge-base")
                .is_some(),
            "the popover names the comparison's merge base"
        );
        assert!(visual.debug_bounds("title-bar-context-commit-0").is_some());
        assert!(visual.debug_bounds("title-bar-context-commit-1").is_some());
    }

    #[gpui::test]
    async fn popover_for_single_commit_lists_its_commit(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, _window, cx| {
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("open popover");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("title-bar-context-popover").is_some());
        assert!(
            visual.debug_bounds("title-bar-context-endpoints").is_some(),
            "a single commit shows its short sha on the endpoints line"
        );
        assert!(
            visual.debug_bounds("title-bar-context-commit-0").is_some(),
            "a single commit lists its one commit row"
        );
    }

    #[gpui::test]
    async fn popover_for_pending_shows_no_commit_list_or_identifier_line(cx: &mut TestAppContext) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let changeset = changeset_with(crate::repo::PENDING_SHA, vec![file_with(3, 1)]);
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![]),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: crate::repo::PENDING_SHA.to_string(),
                    changeset,
                };
                app.selection = Selection::Pending;
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("set pending changeset state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("title-bar-context-popover").is_some());
        assert!(
            visual.debug_bounds("title-bar-context-endpoints").is_none(),
            "pending has no commit identifier line"
        );
        assert!(
            visual
                .debug_bounds("title-bar-context-merge-base")
                .is_none(),
            "pending is not a comparison"
        );
        assert!(
            visual.debug_bounds("title-bar-context-commits").is_none(),
            "pending renders no commit list"
        );
    }

    #[gpui::test]
    async fn popover_for_a_one_commit_comparison_lists_the_introduced_commit(
        cx: &mut TestAppContext,
    ) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let mut changeset = changeset_with("aaaaaaa1111111", vec![file_with(3, 1)]);
                changeset.base_sha = Some("ddddddd4444444".to_string());
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![commit_with("aaaaaaa1111111", "feat: newest")]),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: "aaaaaaa1111111".to_string(),
                    changeset,
                };
                app.selection = Selection::Compare {
                    base_sha: "ccccccc3333333".to_string(),
                    target_sha: "aaaaaaa1111111".to_string(),
                };
                app.comparison_commit_shas = Some(vec!["aaaaaaa1111111".to_string()]);
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("set one-commit comparison state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("title-bar-context-commit-0").is_some(),
            "a comparison introducing one commit still lists it"
        );
    }

    const REPO_NAME: &str = "title-bar-repo";
    const SWITCHER: &str = "title-bar-repo-switcher";
    const WORKTREE_PILL: &str = "title-bar-worktree";

    #[test]
    fn worktree_title_is_main_for_the_primary_and_the_dir_name_otherwise() {
        let (_parent, main, worktree_path) = crate::app::test_support::fixture_repo_with_worktree();
        let primary = crate::repo::open_at(&main).expect("open main");
        let linked = crate::repo::open_at(&worktree_path).expect("open worktree");
        assert_eq!(worktree_title(&primary), "main");
        assert_eq!(worktree_title(&linked), "feature");
    }

    #[gpui::test]
    async fn worktree_pill_renders_whenever_a_repo_is_open(cx: &mut TestAppContext) {
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
        assert!(visual.debug_bounds(WORKTREE_PILL).is_some());
    }

    #[gpui::test]
    async fn clicking_the_worktree_pill_opens_its_switcher_and_closes_the_others(
        cx: &mut TestAppContext,
    ) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        window
            .update(cx, |app, _window, cx| {
                app.repo_switcher_open = true;
                cx.notify();
            })
            .expect("open repo switcher");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual
            .debug_bounds(WORKTREE_PILL)
            .expect("worktree pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert!(app.worktree_switcher_open);
                assert!(!app.repo_switcher_open);
                assert!(!app.context_popover_open);
            })
            .expect("read popover states");
    }

    #[gpui::test]
    async fn opening_the_repo_switcher_closes_the_worktree_switcher(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        window
            .update(cx, |app, _window, cx| {
                app.worktree_switcher_open = true;
                cx.notify();
            })
            .expect("open worktree switcher");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let name = visual.debug_bounds(REPO_NAME).expect("repo name bounds");
        visual.simulate_click(name.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert!(app.repo_switcher_open);
                assert!(!app.worktree_switcher_open);
            })
            .expect("read popover states");
    }

    const WORKTREE_SWITCHER: &str = "title-bar-worktree-switcher";

    /// Open the worktree switcher by clicking the pill, so the entries
    /// snapshot refreshes exactly as it does for a user.
    fn click_worktree_pill(visual: &mut VisualTestContext) {
        let pill = visual
            .debug_bounds("title-bar-worktree")
            .expect("worktree pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());
    }

    #[gpui::test]
    async fn worktree_switcher_falls_back_to_the_current_worktree_when_the_registry_read_fails(
        cx: &mut TestAppContext,
    ) {
        let (dir, _sha) = crate::app::test_support::init_repo_with_one_commit();
        let window = window_with_repo_open(cx, dir.path());
        cx.run_until_parked();

        // Delete the repository's `.git` directory so `list_worktrees` fails
        // to even open the repo, exercising `refresh_worktree_entries`'s
        // error branch. `dir` is a tempdir owned by this test, so removing
        // its `.git` subdirectory is the test destroying its own fixture.
        std::fs::remove_dir_all(dir.path().join(".git")).expect("delete fixture .git dir");

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_worktree_pill(&mut visual);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("title-bar-worktree-row-0").is_some());
        assert!(visual.debug_bounds("title-bar-worktree-row-1").is_none());
        assert!(visual.debug_bounds("title-bar-worktree-current").is_some());
        window
            .read_with(cx, |app, cx| {
                assert_eq!(app.worktree_entries.len(), 1);
                assert_eq!(app.notification_count(cx), 1);
            })
            .expect("read fallback worktree entries");
    }

    #[gpui::test]
    async fn worktree_switcher_lists_both_worktrees_and_marks_the_current(cx: &mut TestAppContext) {
        let (_parent, main, _worktree_path) =
            crate::app::test_support::fixture_repo_with_worktree();
        let window = window_with_repo_open(cx, &main);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_worktree_pill(&mut visual);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(WORKTREE_SWITCHER).is_some());
        assert!(visual.debug_bounds("title-bar-worktree-row-0").is_some());
        assert!(visual.debug_bounds("title-bar-worktree-row-1").is_some());
        assert!(visual.debug_bounds("title-bar-worktree-current").is_some());
    }

    #[gpui::test]
    async fn clicking_another_worktree_switches_the_context(cx: &mut TestAppContext) {
        let (_parent, main, worktree_path) = crate::app::test_support::fixture_repo_with_worktree();
        let window = window_with_repo_open(cx, &main);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_worktree_pill(&mut visual);
        cx.run_until_parked();

        // Row 0 is the main worktree (current); row 1 is "feature".
        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("title-bar-worktree-row-1")
            .expect("feature row bounds");
        visual.simulate_click(row.center(), Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                match &app.mode {
                    Mode::RepoOpen { repo } => {
                        assert_eq!(repo.path, worktree_path);
                        assert_eq!(repo.main_path, main);
                    }
                    Mode::NoRepo => panic!("expected a repo to be open"),
                }
                assert!(!app.worktree_switcher_open, "switching closes the popover");
            })
            .expect("read switched context");
    }

    #[gpui::test]
    async fn clicking_the_current_worktree_row_only_closes_the_popover(cx: &mut TestAppContext) {
        let (_parent, main, _worktree_path) =
            crate::app::test_support::fixture_repo_with_worktree();
        let window = window_with_repo_open(cx, &main);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_worktree_pill(&mut visual);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("title-bar-worktree-row-0")
            .expect("current row bounds");
        visual.simulate_click(row.center(), Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert!(!app.worktree_switcher_open);
                match &app.mode {
                    Mode::RepoOpen { repo } => assert_eq!(repo.path, main),
                    Mode::NoRepo => panic!("expected a repo to be open"),
                }
            })
            .expect("read unchanged context");
    }

    #[gpui::test]
    async fn clicking_the_worktree_pill_twice_closes_its_popover(cx: &mut TestAppContext) {
        let (_parent, paths) = parent_with_repos(&["solo"]);
        let window = window_with_repo_open(cx, &paths[0]);
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_worktree_pill(&mut visual);
        cx.run_until_parked();
        window
            .read_with(cx, |app, _cx| assert!(app.worktree_switcher_open))
            .expect("read opened state");

        let mut visual = VisualTestContext::from_window(*window, cx);
        click_worktree_pill(&mut visual);
        cx.run_until_parked();
        window
            .read_with(cx, |app, _cx| assert!(!app.worktree_switcher_open))
            .expect("read closed state");
    }

    #[gpui::test]
    async fn clicking_the_context_pill_closes_the_worktree_switcher(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, _window, cx| {
                app.worktree_switcher_open = true;
                cx.notify();
            })
            .expect("open worktree switcher");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual.debug_bounds(PILL).expect("context pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert!(app.context_popover_open);
                assert!(!app.worktree_switcher_open);
            })
            .expect("read popover states");
    }

    #[test]
    fn abbreviated_path_keeps_short_paths_and_leading_ellipsizes_long_ones() {
        let short = Path::new("/a/b");
        assert_eq!(abbreviated_path(short, 40), "/a/b");

        let long = Path::new("/very/long/path/that/goes/on/and/on/forever/repo");
        let label = abbreviated_path(long, 12);
        assert!(label.starts_with('\u{2026}'));
        assert!(label.ends_with("forever/repo"));
        assert_eq!(label.chars().count(), 13); // ellipsis + 12 kept chars
    }

    #[test]
    fn abbreviated_path_with_home_substitutes_an_exact_home_prefix() {
        let path = Path::new("/Users/jellison/x");
        assert_eq!(
            abbreviated_path_with_home(path, Some("/Users/jellison"), 40),
            "~/x"
        );
    }

    #[test]
    fn abbreviated_path_with_home_ignores_a_partial_component_prefix() {
        // HOME="/Users/je" is a byte-prefix of "/Users/jellison/x" but not a
        // whole path component, so no substitution should occur.
        let path = Path::new("/Users/jellison/x");
        assert_eq!(
            abbreviated_path_with_home(path, Some("/Users/je"), 40),
            "/Users/jellison/x"
        );
    }

    #[test]
    fn abbreviated_path_with_home_handles_a_trailing_slash_on_home() {
        let path = Path::new("/Users/jellison/x");
        assert_eq!(
            abbreviated_path_with_home(path, Some("/Users/jellison/"), 40),
            "~/x"
        );
    }

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
    async fn switcher_anchors_on_the_main_worktree_when_a_linked_worktree_is_open(
        cx: &mut TestAppContext,
    ) {
        // The fixture's "primary" (main) and "feature" (linked worktree) live
        // side by side in the same parent folder, so both are visible to the
        // sibling scan; only "primary" should ever be marked current, even
        // though the open repository's own path is the "feature" worktree.
        let (_parent, main, worktree_path) = crate::app::test_support::fixture_repo_with_worktree();
        let window = window_with_repo_open(cx, &worktree_path);
        window
            .update(cx, |app, _window, cx| {
                app.repo_switcher_open = true;
                cx.notify();
            })
            .expect("open switcher");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(SWITCHER).is_some());
        // "feature" sorts before "primary" alphabetically → row 0 is
        // "feature", row 1 is "primary".
        let feature_row = visual
            .debug_bounds("title-bar-repo-sibling-0")
            .expect("feature row bounds");
        let primary_row = visual
            .debug_bounds("title-bar-repo-sibling-1")
            .expect("primary row bounds");
        let current_marker = visual
            .debug_bounds("title-bar-repo-current")
            .expect("a current marker is shown");
        // The current marker must sit inside the "primary" row (the main
        // worktree), not the open "feature" worktree's own row.
        assert!(
            primary_row.contains(&current_marker.center()),
            "the main worktree's row is marked current"
        );
        assert!(
            !feature_row.contains(&current_marker.center()),
            "the linked worktree's own row is not marked current"
        );

        window
            .read_with(cx, |app, _cx| match &app.mode {
                Mode::RepoOpen { repo } => {
                    assert_eq!(repo.path, worktree_path);
                    assert_eq!(repo.main_path, main);
                }
                Mode::NoRepo => panic!("expected a repo to be open"),
            })
            .expect("read open repo");
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

    const REVIEW_START: &str = "review-start";
    const REVIEW_BLOCK: &str = "review-block";
    const REVIEW_DELETE: &str = "review-delete";
    const REVIEW_DELETE_ARMED: &str = "review-delete-armed";
    const REVIEW_COMPLETE_TOGGLE: &str = "review-complete-toggle";
    const REVIEW_NAME: &str = "title-bar-review-name";

    #[gpui::test]
    async fn start_review_affordance_creates_and_shows_the_review_block(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual.debug_bounds(PILL).expect("pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        let start = visual.debug_bounds(REVIEW_START).expect("start affordance");
        visual.simulate_click(start.center(), Modifiers::none());

        // `debug_bounds` accumulates for the window's lifetime rather than
        // resetting per frame (see gpui's `Frame::clear`), so a selector that
        // rendered earlier in this same window can't be asserted absent here.
        // The review block replacing the affordance is instead verified via
        // `review_block_replaces_the_start_affordance_for_an_attached_review`
        // below, which opens the popover fresh on a window whose review
        // already exists.
        assert!(visual.debug_bounds(REVIEW_BLOCK).is_some());
        window
            .read_with(cx, |app, _cx| {
                let review = app.current_review().expect("review created");
                assert_eq!(review.name, "feat: thing");
            })
            .expect("review exists");
    }

    #[gpui::test]
    async fn review_block_replaces_the_start_affordance_for_an_attached_review(
        cx: &mut TestAppContext,
    ) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, window, cx| {
                app.start_review(window, cx);
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("start review and open popover");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(REVIEW_BLOCK).is_some());
        assert!(
            visual.debug_bounds(REVIEW_START).is_none(),
            "an attached review shows the block, not the start affordance"
        );
    }

    #[gpui::test]
    async fn pending_changeset_popover_offers_no_review_controls(cx: &mut TestAppContext) {
        let window = app_window(cx);
        window
            .update(cx, |app, _window, cx| {
                let changeset = changeset_with(crate::repo::PENDING_SHA, vec![file_with(3, 1)]);
                app.mode = Mode::RepoOpen {
                    repo: repo_named("Demo", vec![]),
                };
                app.review_screen = ReviewScreen::Changeset {
                    sha: crate::repo::PENDING_SHA.to_string(),
                    changeset,
                };
                app.selection = Selection::Pending;
                app.context_popover_open = true;
                cx.notify();
            })
            .expect("set pending changeset state");

        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(REVIEW_START).is_none());
        assert!(visual.debug_bounds(REVIEW_BLOCK).is_none());
    }

    #[gpui::test]
    async fn delete_control_arms_then_deletes_and_detaches(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, window, cx| {
                app.start_review(window, cx);
            })
            .expect("start");
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual.debug_bounds(PILL).expect("pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        let delete = visual.debug_bounds(REVIEW_DELETE).expect("delete control");
        visual.simulate_click(delete.center(), Modifiers::none());
        let armed = visual
            .debug_bounds(REVIEW_DELETE_ARMED)
            .expect("armed control");
        visual.simulate_click(armed.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert!(app.current_review().is_none());
                // Changeset stays open: deleting only detaches.
                assert!(matches!(app.review_screen, ReviewScreen::Changeset { .. }));
            })
            .expect("deleted and detached");
    }

    #[gpui::test]
    async fn complete_toggle_flips_status_and_stays_attached(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, window, cx| {
                app.start_review(window, cx);
            })
            .expect("start");
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        let pill = visual.debug_bounds(PILL).expect("pill bounds");
        visual.simulate_click(pill.center(), Modifiers::none());

        let toggle = visual
            .debug_bounds(REVIEW_COMPLETE_TOGGLE)
            .expect("complete toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                let review = app.current_review().expect("still attached");
                assert_eq!(review.status, crate::reviews::ReviewStatus::Completed);
                assert!(review.completed_at.is_some());
            })
            .expect("read completed state");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds(REVIEW_COMPLETE_TOGGLE)
            .expect("reopen toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                let review = app.current_review().expect("still attached");
                assert_eq!(review.status, crate::reviews::ReviewStatus::Active);
                assert!(review.completed_at.is_none());
            })
            .expect("read reopened state");
    }

    #[gpui::test]
    async fn pill_shows_the_review_name_when_attached(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds(REVIEW_NAME).is_none(), "no review yet");

        window
            .update(cx, |app, window, cx| {
                app.start_review(window, cx);
            })
            .expect("start");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let short_bounds = visual.debug_bounds(REVIEW_NAME).expect("name renders");

        // The pill is fluid: a long name widens the box on a single line
        // rather than wrapping inside a capped width.
        window
            .update(cx, |app, window, cx| {
                assert!(app.rename_current_review(
                    "a review name comfortably wider than any fixed cap",
                    window,
                    cx,
                ));
            })
            .expect("rename to a long name");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let long_bounds = visual.debug_bounds(REVIEW_NAME).expect("renamed renders");
        assert!(
            long_bounds.size.width > px(160.),
            "the name box must grow past the old 160px cap, got {:?}",
            long_bounds.size.width,
        );
        assert_eq!(
            long_bounds.size.height, short_bounds.size.height,
            "a long name must stay on one line instead of wrapping",
        );
    }

    const PR_BADGE: &str = "title-bar-pr-badge";

    fn pr_with(id: u64, source_tip_sha: &str) -> crate::bitbucket::PullRequest {
        crate::bitbucket::PullRequest {
            id,
            title: format!("PR {id} title"),
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
            source_tip_sha: source_tip_sha.to_string(),
        }
    }

    #[gpui::test]
    async fn context_pill_shows_pr_badge_when_primary_commit_is_a_source_tip(
        cx: &mut TestAppContext,
    ) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, _window, cx| {
                app.install_bitbucket_for_test(
                    vec![pr_with(42, "abcdef1234567890")],
                    crate::app::PrSectionState::Loaded,
                    cx,
                );
                cx.notify();
            })
            .expect("install PR");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds(PR_BADGE).is_some(),
            "badge renders when the primary commit is a PR source tip"
        );
    }

    #[gpui::test]
    async fn context_pill_hides_pr_badge_for_non_tip_commit(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, _window, cx| {
                app.install_bitbucket_for_test(
                    vec![pr_with(7, "0000000000000000000000000000000000000000")],
                    crate::app::PrSectionState::Loaded,
                    cx,
                );
                cx.notify();
            })
            .expect("install PR");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds(PR_BADGE).is_none(),
            "no badge when the primary commit is not any PR's source tip"
        );
    }

    #[gpui::test]
    async fn context_pill_badge_uses_lowest_id_when_tips_collide(cx: &mut TestAppContext) {
        let window = open_changeset_window(cx);
        window
            .update(cx, |app, _window, cx| {
                app.install_bitbucket_for_test(
                    vec![
                        pr_with(8, "abcdef1234567890"),
                        pr_with(3, "abcdef1234567890"),
                    ],
                    crate::app::PrSectionState::Loaded,
                    cx,
                );
                cx.notify();
            })
            .expect("install PRs");
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.pr_badge_for_commit("abcdef1234567890"),
                    Some(3),
                    "lowest id wins when several PRs share a source tip"
                );
            })
            .expect("read badge id");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds(PR_BADGE).is_some(),
            "badge still renders when several PRs share a tip"
        );
    }

    #[gpui::test]
    async fn pressing_enter_in_the_review_name_field_rejects_a_blank_rename(
        cx: &mut TestAppContext,
    ) {
        // The rename-input's reset-on-rejection path lives in a `Blur`/`Enter`
        // subscription that calls `InputState::set_value`, which needs a
        // `&mut Window` reached through `gpui_component::Root` (see
        // `pressing_enter_in_the_branch_filter_does_not_open_a_changeset` in
        // app.rs for the same wrapper requirement), so this test wraps the
        // app the same way rather than using the plain `app_window` helper.
        cx.update(gpui_component::init);
        let mut app_entity: Option<gpui::Entity<App>> = None;
        let window = cx.add_window(|window, cx| {
            let app = gpui::AppContext::new(cx, |cx| App::new(window, cx));
            app_entity = Some(app.clone());
            gpui_component::Root::new(app, window, cx)
        });
        let app_entity = app_entity.expect("app entity captured");

        window
            .update(cx, |_root, window, cx| {
                app_entity.update(cx, |app, cx| {
                    let changeset = changeset_with("abcdef1234567890", vec![file_with(3, 1)]);
                    app.mode = Mode::RepoOpen {
                        repo: repo_named(
                            "Demo",
                            vec![commit_with("abcdef1234567890", "feat: thing")],
                        ),
                    };
                    app.review_screen = ReviewScreen::Changeset {
                        sha: "abcdef1234567890".to_string(),
                        changeset,
                    };
                    app.selection = Selection::Single {
                        sha: "abcdef1234567890".to_string(),
                    };
                    app.start_review(window, cx);
                    // The input is only mounted while the popover renders it
                    // (see `render_context_popover`), so open it before
                    // focusing the field.
                    app.context_popover_open = true;
                    let name_focus = gpui::Focusable::focus_handle(&app.review_name_input, cx);
                    window.focus(&name_focus);
                    app.review_name_input
                        .update(cx, |state, cx| state.set_value("   ", window, cx));
                    cx.notify();
                });
            })
            .expect("open changeset, start review, focus and blank the name field");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_keystrokes("enter");
        cx.run_until_parked();

        app_entity.read_with(cx, |app, cx| {
            assert_eq!(
                app.current_review().expect("still attached").name,
                "feat: thing",
                "a blank rename is rejected and does not overwrite the review's name"
            );
            assert_eq!(
                app.review_name_input.read(cx).value().as_ref(),
                "feat: thing",
                "the input resets to the review's current name after a rejected rename"
            );
        });
    }
}
