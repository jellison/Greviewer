//! Top-level application entity and root view.

pub mod menu;
pub mod path_picker;

pub use menu::{
    bind_app_keys, build_app_menus, open_repository_key_binding, MenuSnapshot,
    GREVIEWER_MENU_LABEL, OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
};
pub use path_picker::{repository_prompt_options, GpuiPathPicker, PathPicker, PathPickerOutcome};

use gpui::prelude::FluentBuilder;
use gpui::{
    actions, div, point, px, rgb, AnyElement, AppContext, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, InteractiveElement, IntoElement, Modifiers, ParentElement, Render,
    ScrollHandle, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::notification::{Notification, NotificationList};
use similar::{DiffTag, TextDiff};
use std::path::PathBuf;

use crate::repo;

actions!(app, [OpenRepository, OpenChangeset, CloseChangeset]);

pub struct App {
    pub mode: Mode,
    pub selection: Selection,
    pub review_screen: ReviewScreen,
    pub selected_changed_file_path: Option<String>,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    file_diff_scroll: FileDiffScroll,
    focus_handle: FocusHandle,
}

struct FileDiffScroll {
    old: ScrollHandle,
    new: ScrollHandle,
    side_by_side: ScrollHandle,
}

impl FileDiffScroll {
    fn new() -> Self {
        Self {
            old: ScrollHandle::new(),
            new: ScrollHandle::new(),
            side_by_side: ScrollHandle::new(),
        }
    }

    fn handle_for(&self, side: repo::DiffSide) -> &ScrollHandle {
        match side {
            repo::DiffSide::Old => &self.old,
            repo::DiffSide::New => &self.new,
        }
    }

    fn reset(&self) {
        let origin = point(px(0.), px(0.));
        self.old.set_offset(origin);
        self.new.set_offset(origin);
        self.side_by_side.set_offset(origin);
    }
}

pub enum Mode {
    NoRepo,
    RepoOpen { repo: repo::OpenRepository },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Single {
        sha: String,
    },
    Range {
        start_sha: String,
        end_sha: String,
        shas: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewScreen {
    Graph,
    Changeset {
        sha: String,
        changeset: repo::ChangeSet,
    },
}

/// Emitted whenever `open_repository_at` fails. Carries the user-facing
/// message that was pushed onto the notification list. Tests subscribe to
/// this event to verify the error path because `gpui-component`'s
/// `Notification` keeps its message field private.
#[derive(Debug, Clone)]
pub struct OpenFailed(pub String);

impl EventEmitter<OpenFailed> for App {}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_with_picker(window, cx, Box::new(GpuiPathPicker))
    }

    pub fn new_with_picker(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
    ) -> Self {
        let notifications = cx.new(|cx| NotificationList::new(window, cx));
        let focus_handle = cx.focus_handle();

        window.focus(&focus_handle);
        cx.on_next_frame(window, |app, window, _cx| {
            window.focus(&app.focus_handle);
        });

        Self {
            mode: Mode::NoRepo,
            selection: Selection::None,
            review_screen: ReviewScreen::Graph,
            selected_changed_file_path: None,
            notifications,
            path_picker,
            file_diff_scroll: FileDiffScroll::new(),
            focus_handle,
        }
    }

    /// Show the OS folder picker. When the user picks a directory, hand the
    /// path off to `open_repository_at`. Cancellations are silent; picker
    /// errors surface through the notification list.
    pub fn prompt_and_open_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = self
            .path_picker
            .pick_path(repository_prompt_options(), window, cx);

        cx.spawn_in(window, async move |this, cx| {
            let outcome = task.await;
            this.update_in(cx, |app, window, cx| {
                app.handle_path_picker_outcome(outcome, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn handle_path_picker_outcome(
        &mut self,
        outcome: PathPickerOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            PathPickerOutcome::Picked(path) => self.open_repository_at(path, window, cx),
            PathPickerOutcome::Cancelled => {}
            PathPickerOutcome::Failed(message) => self.push_open_failed(message, window, cx),
        }
    }

    fn push_open_failed(&mut self, message: String, window: &mut Window, cx: &mut Context<Self>) {
        self.notifications.update(cx, |list, cx| {
            list.push(Notification::error(message.clone()), window, cx);
        });
        cx.emit(OpenFailed(message));
        cx.notify();
    }

    pub fn open_repository_at(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match repo::open_at(&path) {
            Ok(repo) => {
                self.mode = Mode::RepoOpen { repo };
                self.selection = Selection::None;
                self.review_screen = ReviewScreen::Graph;
                self.selected_changed_file_path = None;
                self.file_diff_scroll.reset();
                cx.notify();
            }
            Err(err) => {
                let message = err.to_string();
                self.push_open_failed(message, window, cx);
            }
        }
    }

    fn select_single_commit(&mut self, sha: String, cx: &mut Context<Self>) {
        self.selection = match &self.selection {
            Selection::Single { sha: selected_sha } if selected_sha == &sha => Selection::None,
            _ => Selection::Single { sha },
        };
        cx.notify();
    }

    fn select_commit(
        &mut self,
        sha: String,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !modifiers.shift {
            self.select_single_commit(sha, cx);
            return;
        }

        let Selection::Single { sha: start_sha } = &self.selection else {
            self.select_single_commit(sha, cx);
            return;
        };
        let start_sha = start_sha.clone();

        if start_sha == sha {
            return;
        }

        match self.range_shas_between(&start_sha, &sha) {
            Ok(Some(shas)) => {
                self.selection = Selection::Range {
                    start_sha,
                    end_sha: sha,
                    shas,
                };
                cx.notify();
            }
            Ok(None) => self.push_open_failed(
                "Those commits are not on a single ancestry path.".to_string(),
                window,
                cx,
            ),
            Err(err) => self.push_open_failed(err.to_string(), window, cx),
        }
    }

    fn range_shas_between(
        &self,
        start_sha: &str,
        end_sha: &str,
    ) -> Result<Option<Vec<String>>, repo::ChangeSetError> {
        let open_repo = match &self.mode {
            Mode::RepoOpen { repo } => repo,
            Mode::NoRepo => return Ok(None),
        };

        let Some(start_index) = open_repo
            .commits
            .iter()
            .position(|commit| commit.sha == start_sha)
        else {
            return Ok(None);
        };
        let Some(end_index) = open_repo
            .commits
            .iter()
            .position(|commit| commit.sha == end_sha)
        else {
            return Ok(None);
        };

        if !repo::commits_share_linear_ancestry(&open_repo.path, start_sha, end_sha)? {
            return Ok(None);
        }

        let first_index = start_index.min(end_index);
        let last_index = start_index.max(end_index);
        Ok(Some(
            open_repo.commits[first_index..=last_index]
                .iter()
                .map(|commit| commit.sha.clone())
                .collect(),
        ))
    }

    fn open_changeset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo_path = match &self.mode {
            Mode::RepoOpen { repo } => repo.path.clone(),
            Mode::NoRepo => return,
        };

        let changeset = match &self.selection {
            Selection::Single { sha } => repo::changeset_for_single_commit(&repo_path, sha),
            Selection::Range { shas, .. } => {
                let (Some(newest_sha), Some(oldest_sha)) = (shas.first(), shas.last()) else {
                    return;
                };
                repo::changeset_for_commit_range(&repo_path, oldest_sha, newest_sha)
            }
            Selection::None => return,
        };

        match changeset {
            Ok(changeset) => {
                if !self
                    .selected_changed_file_path
                    .as_ref()
                    .is_some_and(|path| changeset.files.iter().any(|file| &file.path == path))
                {
                    self.selected_changed_file_path = None;
                }
                let sha = changeset.commit_sha.clone();
                self.review_screen = ReviewScreen::Changeset { sha, changeset };
                cx.notify();
            }
            Err(err) => self.push_open_failed(err.to_string(), window, cx),
        }
    }

    fn close_changeset(&mut self, cx: &mut Context<Self>) {
        self.review_screen = ReviewScreen::Graph;
        cx.notify();
    }

    fn select_changed_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.file_diff_scroll.reset();
        self.selected_changed_file_path = Some(path);
        cx.notify();
    }

    fn is_commit_selected(&self, sha: &str) -> bool {
        match &self.selection {
            Selection::Single { sha: selected_sha } => selected_sha == sha,
            Selection::Range { shas, .. } => shas.iter().any(|selected_sha| selected_sha == sha),
            Selection::None => false,
        }
    }

    fn is_changed_file_selected(&self, file: &repo::ChangedFile) -> bool {
        self.selected_changed_file_path.as_deref() == Some(file.path.as_str())
    }

    #[cfg(test)]
    pub(crate) fn notification_count(&self, cx: &gpui::App) -> usize {
        self.notifications.read(cx).notifications().len()
    }

    #[cfg(test)]
    fn file_diff_old_scroll_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.file_diff_scroll.side_by_side.offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.file_diff_scroll.side_by_side.offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_max_offset(&self) -> gpui::Size<gpui::Pixels> {
        self.file_diff_scroll.side_by_side.max_offset()
    }

    fn render_no_repo(&self) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(20.))
                    .child("No repository open"),
            )
            .child(
                div()
                    .text_color(rgb(0x999999))
                    .text_size(px(14.))
                    .child("Open a repository to start a review."),
            )
    }

    fn render_repo_open(&self, repo: &repo::OpenRepository, cx: &mut Context<Self>) -> AnyElement {
        match &self.review_screen {
            ReviewScreen::Graph => self.render_graph_screen(repo, cx).into_any_element(),
            ReviewScreen::Changeset { sha, changeset } => self
                .render_changeset_screen(repo, sha, changeset, cx)
                .into_any_element(),
        }
    }

    fn render_graph_screen(
        &self,
        repo: &repo::OpenRepository,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let path_text = repo.path.display().to_string();
        let head_line = match &repo.head {
            Some(head) => format!("{} · {}", head.short_sha, head.summary),
            None => "No commits yet.".to_string(),
        };
        let can_open_changeset = matches!(
            self.selection,
            Selection::Single { .. } | Selection::Range { .. }
        );

        let title_row = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                div()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(16.))
                    .font_family("monospace")
                    .child(path_text),
            )
            .when(can_open_changeset, |row| {
                row.child(
                    div()
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x3b82f6))
                        .bg(rgb(0x1d283a))
                        .text_color(rgb(0xdbeafe))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .id("open-changeset")
                        .debug_selector(|| "open-changeset".to_string())
                        .on_click(cx.listener(|app, _event, window, cx| {
                            app.open_changeset(window, cx);
                        }))
                        .child("Open changeset"),
                )
            });

        let header = div()
            .flex()
            .flex_col()
            .w_full()
            .gap_1()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x2a2a2a))
            .child(title_row)
            .child(
                div()
                    .text_color(rgb(0x999999))
                    .text_size(px(13.))
                    .child(head_line),
            );

        let history = if repo.commits.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("commit-history-empty")
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("This repository has no commits to review.")
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .id("commit-history")
                .overflow_y_scroll()
                .children(
                    repo.commits
                        .iter()
                        .enumerate()
                        .map(|(index, commit)| {
                            self.render_commit_row(
                                index,
                                commit,
                                self.is_commit_selected(&commit.sha),
                                cx,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgb(0x171717))
            .child(header)
            .child(history)
    }

    fn render_changeset_screen(
        &self,
        repo: &repo::OpenRepository,
        sha: &str,
        changeset: &repo::ChangeSet,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let path_text = repo.path.display().to_string();
        let short_sha: String = sha.chars().take(7).collect();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(rgb(0x2a2a2a))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_color(rgb(0xe6e6e6))
                            .text_size(px(16.))
                            .font_family("monospace")
                            .child(path_text),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x999999))
                            .text_size(px(13.))
                            .child(format!("Changeset for {short_sha}")),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x4a4a4a))
                    .bg(rgb(0x242424))
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .id("close-changeset")
                    .debug_selector(|| "close-changeset".to_string())
                    .on_click(cx.listener(|app, _event, _window, cx| {
                        app.close_changeset(cx);
                    }))
                    .child("Close"),
            );

        let body: AnyElement = if changeset.files.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("changed-files-empty")
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("This changeset has no net file changes.")
                .into_any_element()
        } else {
            let selected_file = self.selected_changed_file(changeset);

            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(340.))
                        .h_full()
                        .min_h_0()
                        .id("changed-files")
                        .overflow_y_scroll()
                        .border_1()
                        .border_color(rgb(0x242424))
                        .children(
                            changeset
                                .files
                                .iter()
                                .enumerate()
                                .map(|(index, file)| {
                                    self.render_changed_file_row(
                                        index,
                                        file,
                                        self.is_changed_file_selected(file),
                                        cx,
                                    )
                                })
                                .collect::<Vec<_>>(),
                        ),
                )
                .child(self.render_file_detail(repo, changeset, selected_file))
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgb(0x171717))
            .child(header)
            .child(body)
    }

    fn selected_changed_file<'a>(
        &self,
        changeset: &'a repo::ChangeSet,
    ) -> Option<&'a repo::ChangedFile> {
        let selected_path = self.selected_changed_file_path.as_deref()?;
        changeset
            .files
            .iter()
            .find(|file| file.path == selected_path)
    }

    fn render_changed_file_row(
        &self,
        index: usize,
        file: &repo::ChangedFile,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = file.path.clone();
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let border_color = if selected {
            rgb(0x3b82f6)
        } else {
            rgb(0x242424)
        };
        let debug_selector = if selected {
            format!("selected-changed-file-row-{index}")
        } else {
            format!("changed-file-row-{index}")
        };

        div()
            .flex()
            .items_center()
            .w_full()
            .gap_3()
            .px_4()
            .py_2()
            .bg(row_bg)
            .border_b_1()
            .border_color(border_color)
            .cursor_pointer()
            .id(("changed-file-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.select_changed_file(path.clone(), cx);
            }))
            .child(
                div()
                    .w(px(72.))
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(change_kind_border(file.kind))
                    .bg(change_kind_background(file.kind))
                    .text_color(change_kind_text(file.kind))
                    .text_size(px(11.))
                    .font_family("monospace")
                    .child(change_kind_label(file.kind)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .text_color(rgb(0xe6e6e6))
                            .text_size(px(14.))
                            .font_family("monospace")
                            .child(file.path.clone()),
                    )
                    .when_some(file.old_path.clone(), |column, old_path| {
                        column.child(
                            div()
                                .text_color(rgb(0x8a8a8a))
                                .text_size(px(12.))
                                .font_family("monospace")
                                .child(format!("from {old_path}")),
                        )
                    }),
            )
    }

    fn render_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        selected_file: Option<&repo::ChangedFile>,
    ) -> AnyElement {
        match selected_file {
            Some(file) => {
                let title = file.path.clone();
                let kind = change_kind_label(file.kind);
                let content = match repo::file_diff_for_changed_file_between(
                    &repo.path,
                    &changeset.commit_sha,
                    changeset.base_sha.as_deref(),
                    file,
                ) {
                    Ok(diff) => render_file_diff_content(diff.content, &self.file_diff_scroll),
                    Err(err) => render_file_diff_error(err.to_string()),
                };

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h_0()
                    .overflow_hidden()
                    .id("file-detail-shell")
                    .debug_selector(|| "file-detail-shell".to_string())
                    .px_4()
                    .py_4()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_color(rgb(0xe6e6e6))
                                    .text_size(px(16.))
                                    .font_family("monospace")
                                    .child(title),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .border_color(change_kind_border(file.kind))
                                    .bg(change_kind_background(file.kind))
                                    .text_color(change_kind_text(file.kind))
                                    .text_size(px(11.))
                                    .font_family("monospace")
                                    .child(kind),
                            ),
                    )
                    .when_some(file.old_path.clone(), |detail, old_path| {
                        detail.child(
                            div()
                                .text_color(rgb(0x999999))
                                .text_size(px(12.))
                                .font_family("monospace")
                                .child(format!("Renamed from {old_path}")),
                        )
                    })
                    .child(content)
                    .into_any_element()
            }
            None => div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .id("file-detail-empty")
                .items_center()
                .justify_center()
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("Select a file to inspect its diff.")
                .into_any_element(),
        }
    }

    fn render_commit_row(
        &self,
        index: usize,
        commit: &repo::CommitInfo,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let marker = if commit.is_head { "HEAD" } else { "" };
        let secondary = format!("{} · {}", commit.author, commit.authored_date);
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let border_color = if selected {
            rgb(0x3b82f6)
        } else {
            rgb(0x242424)
        };
        let debug_selector = if selected {
            format!("selected-commit-row-{index}")
        } else {
            format!("commit-row-{index}")
        };
        let sha = commit.sha.clone();

        div()
            .flex()
            .items_center()
            .w_full()
            .gap_3()
            .px_4()
            .py_2()
            .bg(row_bg)
            .border_b_1()
            .border_color(border_color)
            .cursor_pointer()
            .id(("commit-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, event: &ClickEvent, window, cx| {
                app.select_commit(sha.clone(), event.modifiers(), window, cx);
            }))
            .child(
                div()
                    .w(px(48.))
                    .text_color(if commit.is_head {
                        rgb(0x7dd3fc)
                    } else {
                        rgb(0x555555)
                    })
                    .text_size(px(11.))
                    .font_family("monospace")
                    .child(marker),
            )
            .child(
                div()
                    .w(px(72.))
                    .text_color(rgb(0xa3e635))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .child(commit.short_sha.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .text_color(rgb(0xe6e6e6))
                            .text_size(px(14.))
                            .child(commit.summary.clone()),
                    )
                    .child(
                        div()
                            .text_color(rgb(0x8a8a8a))
                            .text_size(px(12.))
                            .child(secondary),
                    ),
            )
    }
}

fn render_file_diff_content(content: repo::FileDiffContent, scroll: &FileDiffScroll) -> AnyElement {
    match content {
        repo::FileDiffContent::Single { side, text } => {
            let label = match side {
                repo::DiffSide::Old => "Before",
                repo::DiffSide::New => "After",
            };
            let selector = match side {
                repo::DiffSide::Old => "file-diff-side-old",
                repo::DiffSide::New => "file-diff-side-new",
            };
            let cells = single_side_diff_rows(side, &text)
                .into_iter()
                .map(|row| match side {
                    repo::DiffSide::Old => row.old,
                    repo::DiffSide::New => row.new,
                })
                .collect::<Vec<_>>();

            render_file_diff_side(label, selector, cells, scroll.handle_for(side))
                .into_any_element()
        }
        repo::FileDiffContent::SideBySide { old_text, new_text } => {
            let rows = side_by_side_diff_rows(&old_text, &new_text);
            let old_cells = rows.iter().map(|row| row.old.clone()).collect::<Vec<_>>();
            let new_cells = rows.into_iter().map(|row| row.new).collect::<Vec<_>>();

            div()
                .flex()
                .flex_1()
                .gap_3()
                .min_h_0()
                .child(render_file_diff_side(
                    "Before",
                    "file-diff-side-old",
                    old_cells,
                    &scroll.side_by_side,
                ))
                .child(render_file_diff_side(
                    "After",
                    "file-diff-side-new",
                    new_cells,
                    &scroll.side_by_side,
                ))
                .into_any_element()
        }
        repo::FileDiffContent::Binary => div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .border_1()
            .border_color(rgb(0x2a2a2a))
            .bg(rgb(0x141414))
            .id("file-diff-binary")
            .debug_selector(|| "file-diff-binary".to_string())
            .text_color(rgb(0x999999))
            .text_size(px(14.))
            .child("No textual diff is available for this file.")
            .into_any_element(),
    }
}

fn render_file_diff_error(message: String) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(rgb(0x2a2a2a))
        .bg(rgb(0x141414))
        .id("file-diff-error")
        .debug_selector(|| "file-diff-error".to_string())
        .text_color(rgb(0xfca5a5))
        .text_size(px(14.))
        .child(message)
        .into_any_element()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffLineStatus {
    Unchanged,
    Added,
    Removed,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffLineCell {
    line_number: Option<usize>,
    text: String,
    status: DiffLineStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffRow {
    old: DiffLineCell,
    new: DiffLineCell,
}

fn single_side_diff_rows(side: repo::DiffSide, text: &str) -> Vec<DiffRow> {
    let status = match side {
        repo::DiffSide::Old => DiffLineStatus::Removed,
        repo::DiffSide::New => DiffLineStatus::Added,
    };

    content_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let visible = DiffLineCell {
                line_number: Some(index + 1),
                text: line,
                status,
            };

            match side {
                repo::DiffSide::Old => DiffRow {
                    old: visible,
                    new: empty_diff_cell(),
                },
                repo::DiffSide::New => DiffRow {
                    old: empty_diff_cell(),
                    new: visible,
                },
            }
        })
        .collect()
}

fn side_by_side_diff_rows(old_text: &str, new_text: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(old_text, new_text);
    let old_lines = diff.old_slices();
    let new_lines = diff.new_slices();
    let mut rows = Vec::new();

    for op in diff.ops() {
        match op.tag() {
            DiffTag::Equal => {
                for (old_index, new_index) in op.old_range().zip(op.new_range()) {
                    rows.push(DiffRow {
                        old: diff_cell(old_index, old_lines[old_index], DiffLineStatus::Unchanged),
                        new: diff_cell(new_index, new_lines[new_index], DiffLineStatus::Unchanged),
                    });
                }
            }
            DiffTag::Delete => {
                for old_index in op.old_range() {
                    rows.push(DiffRow {
                        old: diff_cell(old_index, old_lines[old_index], DiffLineStatus::Removed),
                        new: empty_diff_cell(),
                    });
                }
            }
            DiffTag::Insert => {
                for new_index in op.new_range() {
                    rows.push(DiffRow {
                        old: empty_diff_cell(),
                        new: diff_cell(new_index, new_lines[new_index], DiffLineStatus::Added),
                    });
                }
            }
            DiffTag::Replace => {
                let old_indices = op.old_range().collect::<Vec<_>>();
                let new_indices = op.new_range().collect::<Vec<_>>();
                let len = old_indices.len().max(new_indices.len());

                for index in 0..len {
                    let old = old_indices
                        .get(index)
                        .map(|old_index| {
                            diff_cell(*old_index, old_lines[*old_index], DiffLineStatus::Removed)
                        })
                        .unwrap_or_else(empty_diff_cell);
                    let new = new_indices
                        .get(index)
                        .map(|new_index| {
                            diff_cell(*new_index, new_lines[*new_index], DiffLineStatus::Added)
                        })
                        .unwrap_or_else(empty_diff_cell);

                    rows.push(DiffRow { old, new });
                }
            }
        }
    }

    rows
}

fn diff_cell(line_index: usize, line: &str, status: DiffLineStatus) -> DiffLineCell {
    DiffLineCell {
        line_number: Some(line_index + 1),
        text: trim_line_ending(line),
        status,
    }
}

fn empty_diff_cell() -> DiffLineCell {
    DiffLineCell {
        line_number: None,
        text: String::new(),
        status: DiffLineStatus::Empty,
    }
}

fn render_file_diff_side(
    label: &'static str,
    selector: &'static str,
    cells: Vec<DiffLineCell>,
    scroll_handle: &ScrollHandle,
) -> impl IntoElement {
    let scroll_selector = match selector {
        "file-diff-side-old" => "file-diff-side-old-scroll",
        "file-diff-side-new" => "file-diff-side-new-scroll",
        _ => "file-diff-side-scroll",
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .min_h_0()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(0x2a2a2a))
        .bg(rgb(0x141414))
        .id(selector)
        .debug_selector(move || selector.to_string())
        .child(
            div()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(rgb(0x2a2a2a))
                .text_color(rgb(0x999999))
                .text_size(px(12.))
                .font_family("monospace")
                .child(label),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .id(scroll_selector)
                .debug_selector(move || scroll_selector.to_string())
                .overflow_y_scroll()
                .scrollbar_width(px(12.))
                .track_scroll(scroll_handle)
                .children(
                    cells
                        .into_iter()
                        .enumerate()
                        .map(|(index, cell)| render_file_diff_line(selector, index, cell))
                        .collect::<Vec<_>>(),
                ),
        )
}

fn render_file_diff_line(
    pane_selector: &'static str,
    row_index: usize,
    cell: DiffLineCell,
) -> impl IntoElement {
    let line_number = cell
        .line_number
        .map(|line_number| line_number.to_string())
        .unwrap_or_default();
    let pane_offset = match pane_selector {
        "file-diff-side-old" => 0,
        "file-diff-side-new" => 1,
        _ => 2,
    };
    let id_index = row_index * 3 + pane_offset;
    let row_selector = diff_line_debug_selector(cell.status);
    let row_bg = diff_line_background(cell.status);
    let text_color = match cell.status {
        DiffLineStatus::Empty => rgb(0x666666),
        _ => rgb(0xe6e6e6),
    };

    div()
        .flex()
        .items_start()
        .min_h(px(18.))
        .bg(row_bg)
        .id(("file-diff-line", id_index))
        .debug_selector(move || row_selector.to_string())
        .child(
            div()
                .w(px(48.))
                .px_2()
                .text_color(rgb(0x666666))
                .text_size(px(12.))
                .font_family("monospace")
                .child(line_number),
        )
        .child(
            div()
                .flex_1()
                .px_2()
                .text_color(text_color)
                .text_size(px(12.))
                .font_family("monospace")
                .child(cell.text),
        )
}

fn diff_line_debug_selector(status: DiffLineStatus) -> &'static str {
    match status {
        DiffLineStatus::Unchanged => "file-diff-row-unchanged",
        DiffLineStatus::Added => "file-diff-row-added",
        DiffLineStatus::Removed => "file-diff-row-removed",
        DiffLineStatus::Empty => "file-diff-row-empty",
    }
}

fn diff_line_background(status: DiffLineStatus) -> gpui::Rgba {
    match status {
        DiffLineStatus::Unchanged => rgb(0x141414),
        DiffLineStatus::Added => rgb(0x132b1a),
        DiffLineStatus::Removed => rgb(0x341b1b),
        DiffLineStatus::Empty => rgb(0x101010),
    }
}

fn content_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.lines().map(str::to_string).collect()
}

fn trim_line_ending(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

fn change_kind_label(kind: repo::ChangeKind) -> &'static str {
    match kind {
        repo::ChangeKind::Added => "Added",
        repo::ChangeKind::Modified => "Modified",
        repo::ChangeKind::Deleted => "Deleted",
        repo::ChangeKind::Renamed => "Renamed",
    }
}

fn change_kind_background(kind: repo::ChangeKind) -> gpui::Rgba {
    match kind {
        repo::ChangeKind::Added => rgb(0x132b1a),
        repo::ChangeKind::Modified => rgb(0x1d283a),
        repo::ChangeKind::Deleted => rgb(0x341b1b),
        repo::ChangeKind::Renamed => rgb(0x2f2a14),
    }
}

fn change_kind_border(kind: repo::ChangeKind) -> gpui::Rgba {
    match kind {
        repo::ChangeKind::Added => rgb(0x2f7d46),
        repo::ChangeKind::Modified => rgb(0x3b82f6),
        repo::ChangeKind::Deleted => rgb(0x8b3a3a),
        repo::ChangeKind::Renamed => rgb(0x9a7b22),
    }
}

fn change_kind_text(kind: repo::ChangeKind) -> gpui::Rgba {
    match kind {
        repo::ChangeKind::Added => rgb(0x86efac),
        repo::ChangeKind::Modified => rgb(0xdbeafe),
        repo::ChangeKind::Deleted => rgb(0xfca5a5),
        repo::ChangeKind::Renamed => rgb(0xfde68a),
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.mode {
            Mode::NoRepo => self.render_no_repo().into_any_element(),
            Mode::RepoOpen { repo } => self.render_repo_open(repo, cx),
        };

        div()
            .relative()
            .w_full()
            .h_full()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|app, _: &OpenRepository, window, cx| {
                app.prompt_and_open_repository(window, cx);
            }))
            .on_action(cx.listener(|app, _: &OpenChangeset, window, cx| {
                app.open_changeset(window, cx);
            }))
            .on_action(cx.listener(|app, _: &CloseChangeset, _window, cx| {
                app.close_changeset(cx);
            }))
            .child(body)
            .child(self.notifications.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        side_by_side_diff_rows, single_side_diff_rows, App, CloseChangeset, DiffLineStatus, Mode,
        OpenChangeset, OpenFailed, ReviewScreen, Selection,
    };
    use crate::repo::{ChangeKind, DiffSide};
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use std::fs;

    fn init_repo_with_one_commit() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");

        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "Add hello.txt", &tree, &[])
            .expect("create commit");

        drop(tree);
        drop(index);
        drop(repo);

        (dir, oid.to_string())
    }

    fn init_repo_with_two_commits() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let root_oid = commit_all(&repo, "Add hello.txt", &[]);

        fs::write(dir.path().join("hello.txt"), "hello world\n").expect("update file");
        let update_oid = commit_all(&repo, "Update hello.txt", &[root_oid]);

        drop(repo);

        (dir, update_oid.to_string())
    }

    fn init_repo_with_three_commits() -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("range.txt"), "one\n").expect("write file");
        let first_oid = commit_all(&repo, "First", &[]);

        fs::write(dir.path().join("range.txt"), "two\n").expect("update file");
        let second_oid = commit_all(&repo, "Second", &[first_oid]);

        fs::write(dir.path().join("range.txt"), "three\n").expect("update file again");
        let third_oid = commit_all(&repo, "Third", &[second_oid]);

        drop(repo);

        (
            dir,
            vec![
                third_oid.to_string(),
                second_oid.to_string(),
                first_oid.to_string(),
            ],
        )
    }

    fn init_repo_with_diverged_history() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("base.txt"), "base\n").expect("write base file");
        let root_oid = commit_all(&repo, "Root", &[]);

        fs::write(dir.path().join("left.txt"), "left\n").expect("write left file");
        let left_oid = commit_all_to_ref(&repo, Some("refs/heads/left"), "Left", &[root_oid]);

        fs::remove_file(dir.path().join("left.txt")).expect("remove left file");
        fs::write(dir.path().join("right.txt"), "right\n").expect("write right file");
        let right_oid = commit_all_to_ref(&repo, Some("refs/heads/right"), "Right", &[root_oid]);

        fs::write(dir.path().join("left.txt"), "left\n").expect("restore left file");
        let merge_oid = commit_all_to_ref(&repo, None, "Merge", &[left_oid, right_oid]);
        repo.reference("refs/heads/master", merge_oid, true, "update test HEAD")
            .expect("point HEAD branch at merge");

        drop(repo);

        (dir, left_oid.to_string(), right_oid.to_string())
    }

    fn init_repo_with_long_diff() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let old_text = (1..=160)
            .map(|line| format!("old line {line:03}\n"))
            .collect::<String>();
        fs::write(dir.path().join("long.txt"), old_text).expect("write old file");
        let root_oid = commit_all(&repo, "Add long file", &[]);

        let new_text = (1..=160)
            .map(|line| format!("new line {line:03}\n"))
            .collect::<String>();
        fs::write(dir.path().join("long.txt"), new_text).expect("write new file");
        let update_oid = commit_all(&repo, "Update long file", &[root_oid]);

        drop(repo);

        (dir, update_oid.to_string())
    }

    fn commit_all(repo: &Repository, message: &str, parents: &[git2::Oid]) -> git2::Oid {
        commit_all_to_ref(repo, Some("HEAD"), message, parents)
    }

    fn commit_all_to_ref(
        repo: &Repository,
        update_ref: Option<&str>,
        message: &str,
        parents: &[git2::Oid],
    ) -> git2::Oid {
        let mut index = repo.index().expect("open index");
        index.clear().expect("clear index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        let parent_commits = parents
            .iter()
            .map(|oid| repo.find_commit(*oid).expect("find parent commit"))
            .collect::<Vec<_>>();
        let parent_refs = parent_commits.iter().collect::<Vec<_>>();

        repo.commit(update_ref, &sig, &sig, message, &tree, &parent_refs)
            .expect("create commit")
    }

    #[test]
    fn line_diff_single_side_added_marks_lines_as_added() {
        let rows = single_side_diff_rows(DiffSide::New, "first\nsecond\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].new.status, DiffLineStatus::Added);
        assert_eq!(rows[0].new.line_number, Some(1));
        assert_eq!(rows[0].new.text, "first");
        assert_eq!(rows[0].old.status, DiffLineStatus::Empty);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
    }

    #[test]
    fn line_diff_single_side_deleted_marks_lines_as_removed() {
        let rows = single_side_diff_rows(DiffSide::Old, "first\nsecond\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[0].old.line_number, Some(1));
        assert_eq!(rows[0].old.text, "first");
        assert_eq!(rows[0].new.status, DiffLineStatus::Empty);
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
    }

    #[test]
    fn line_diff_side_by_side_aligns_equal_removed_and_added_rows() {
        let rows = side_by_side_diff_rows("same\nold\n", "same\nnew\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].old.status, DiffLineStatus::Unchanged);
        assert_eq!(rows[0].new.status, DiffLineStatus::Unchanged);
        assert_eq!(rows[0].old.text, "same");
        assert_eq!(rows[0].new.text, "same");
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
        assert_eq!(rows[1].old.text, "old");
        assert_eq!(rows[1].new.text, "new");
    }

    #[test]
    fn line_diff_side_by_side_pads_uneven_replacements() {
        let rows = side_by_side_diff_rows("one\ntwo\n", "one\nalpha\nbeta\n");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
        assert_eq!(rows[2].old.status, DiffLineStatus::Empty);
        assert_eq!(rows[2].new.status, DiffLineStatus::Added);
        assert_eq!(rows[2].new.text, "beta");
    }

    #[gpui::test]
    async fn renders_placeholder(cx: &mut TestAppContext) {
        let _window = cx.add_window(App::new);
        // The contract: booting the App in the gpui test harness must not panic
        // and the entity must construct successfully. Once the App gains user-
        // visible interactivity, this test grows to assert on observable events.
    }

    #[gpui::test]
    async fn opening_a_real_repo_advances_to_repo_open_mode(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().to_path_buf();

        let repo = Repository::init(&path).expect("init repo");
        fs::write(path.join("hello.txt"), "hello\n").expect("write file");
        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");
        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        repo.commit(Some("HEAD"), &sig, &sig, "Add hello.txt", &tree, &[])
            .expect("create commit");
        drop(tree);
        drop(index);
        drop(repo);

        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("update window");

        window
            .read_with(cx, |app, _cx| match &app.mode {
                Mode::RepoOpen { repo } => {
                    let head = repo.head.as_ref().expect("head present");
                    assert_eq!(head.summary, "Add hello.txt");
                }
                Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
            })
            .expect("read window");
    }

    #[gpui::test]
    async fn selecting_commits_toggles_single_selection(cx: &mut TestAppContext) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, _window, cx| {
                app.select_single_commit("first-sha".to_string(), cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "first-sha".to_string(),
                    },
                );

                app.select_single_commit("second-sha".to_string(), cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "second-sha".to_string(),
                    },
                );

                app.select_single_commit("second-sha".to_string(), cx);
                assert_eq!(app.selection, Selection::None);
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn shift_clicking_linear_commits_selects_an_inclusive_range(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_three_commits();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let tip_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("tip commit row debug bounds");
        visual.simulate_click(tip_bounds.center(), Modifiers::none());

        let root_bounds = visual
            .debug_bounds("commit-row-2")
            .expect("root commit row debug bounds");
        visual.simulate_click(root_bounds.center(), Modifiers::shift());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Range {
                        start_sha: shas[0].clone(),
                        end_sha: shas[2].clone(),
                        shas: shas.clone(),
                    },
                );
            })
            .expect("read range selection");

        visual
            .debug_bounds("selected-commit-row-0")
            .expect("selected tip row debug bounds");
        visual
            .debug_bounds("selected-commit-row-1")
            .expect("selected middle row debug bounds");
        visual
            .debug_bounds("selected-commit-row-2")
            .expect("selected root row debug bounds");
    }

    #[gpui::test]
    async fn shift_clicking_diverged_commits_preserves_the_original_selection(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);

        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(left_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(right_sha, Modifiers::shift(), window, cx);

                assert_eq!(app.selection, Selection::Single { sha: left_sha });
            })
            .expect("attempt invalid range selection");
    }

    #[gpui::test]
    async fn opening_changeset_requires_a_selection(cx: &mut TestAppContext) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_changeset(window, cx);
                assert_eq!(app.review_screen, ReviewScreen::Graph);

                app.select_single_commit("selected-sha".to_string(), cx);
                app.open_changeset(window, cx);
                assert_eq!(app.review_screen, ReviewScreen::Graph);
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn opening_changeset_loads_changed_files(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex.clone(), cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { sha, changeset } => {
                    assert_eq!(sha, &oid_hex);
                    assert_eq!(changeset.commit_sha, oid_hex);
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "hello.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Added);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read changeset");
    }

    #[gpui::test]
    async fn opening_range_changeset_renders_rollup_changed_files(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_three_commits();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let tip_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("tip commit row debug bounds");
        visual.simulate_click(tip_bounds.center(), Modifiers::none());

        let root_bounds = visual
            .debug_bounds("commit-row-2")
            .expect("root commit row debug bounds");
        visual.simulate_click(root_bounds.center(), Modifiers::shift());

        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { sha, changeset } => {
                    assert_eq!(sha, &shas[0]);
                    assert_eq!(changeset.commit_sha, shas[0]);
                    assert_eq!(changeset.base_sha, None);
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "range.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Added);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read range changeset");

        visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
    }

    #[gpui::test]
    async fn selecting_changed_file_records_path(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.select_changed_file("hello.txt".to_string(), cx);

                assert_eq!(
                    app.selected_changed_file_path,
                    Some("hello.txt".to_string()),
                );
            })
            .expect("select changed file");
    }

    #[gpui::test]
    async fn reopening_changeset_preserves_valid_changed_file_selection(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.select_changed_file("hello.txt".to_string(), cx);
                app.close_changeset(cx);
                app.open_changeset(window, cx);

                assert_eq!(
                    app.selected_changed_file_path,
                    Some("hello.txt".to_string()),
                );
            })
            .expect("reopen changeset");
    }

    #[gpui::test]
    async fn opening_changeset_clears_stale_changed_file_selection(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.selected_changed_file_path = Some("missing.txt".to_string());
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);

                assert_eq!(app.selected_changed_file_path, None);
            })
            .expect("open changeset");
    }

    #[gpui::test]
    async fn closing_changeset_returns_to_graph_and_preserves_selection(cx: &mut TestAppContext) {
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.select_single_commit("selected-sha".to_string(), cx);
                app.open_changeset(window, cx);
                app.close_changeset(cx);

                assert_eq!(app.review_screen, ReviewScreen::Graph);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "selected-sha".to_string(),
                    },
                );
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn dispatching_open_and_close_changeset_actions_updates_review_screen(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex.clone(), cx);
            })
            .expect("select commit");

        cx.dispatch_action(*window, OpenChangeset);

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { sha, changeset } => {
                    assert_eq!(sha, &oid_hex);
                    assert_eq!(changeset.files.len(), 1);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read open changeset state");

        cx.dispatch_action(*window, CloseChangeset);

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.review_screen, ReviewScreen::Graph);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: oid_hex.clone(),
                    },
                );
            })
            .expect("read closed changeset state");
    }

    #[gpui::test]
    async fn clicking_changeset_affordances_enters_and_exits_review_mode(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex.clone(), cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");

        visual.simulate_click(open_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { sha, changeset } => {
                    assert_eq!(sha, &oid_hex);
                    assert_eq!(changeset.files.len(), 1);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read opened review state");

        visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");

        let close_bounds = visual
            .debug_bounds("close-changeset")
            .expect("close changeset debug bounds");

        visual.simulate_click(close_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.review_screen, ReviewScreen::Graph);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: oid_hex.clone(),
                    },
                );
            })
            .expect("read closed review state");
    }

    #[gpui::test]
    async fn clicking_a_commit_row_toggles_selection(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("commit row debug bounds");

        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: oid_hex.clone(),
                    },
                );
            })
            .expect("read selected state");

        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.selection, Selection::None);
            })
            .expect("read cleared state");
    }

    #[gpui::test]
    async fn clicking_open_changeset_renders_changed_files(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");

        visual.simulate_click(open_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_detail_shell(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("selected-changed-file-row-0")
            .expect("selected changed file row debug bounds");
        visual
            .debug_bounds("file-detail-shell")
            .expect("file detail shell debug bounds");

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selected_changed_file_path,
                    Some("hello.txt".to_string()),
                );
            })
            .expect("read selected changed file");
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_text_diff_content(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_line_highlights(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed line row debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("added line row debug bounds");
    }

    #[gpui::test]
    async fn scrolling_long_file_diff_moves_the_diff_scroll_area(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.select_changed_file("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-new-scroll")
            .expect("new file diff scroll debug bounds");
        let max_offset = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_max_offset())
            .expect("read new diff scroll max offset");
        assert!(
            max_offset.height > px(0.),
            "long diff should exceed the visible diff scroll area"
        );

        let before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset before wheel");
        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });
        let after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset after wheel");

        assert!(
            after.y < before.y,
            "wheel scroll should move the diff content upward"
        );
    }

    #[gpui::test]
    async fn scrolling_new_side_of_side_by_side_diff_scrolls_old_side(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.select_changed_file("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-new-scroll")
            .expect("new file diff scroll debug bounds");
        let old_before = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset before wheel");
        let new_before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });

        let old_after = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset after wheel");
        let new_after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset after wheel");

        assert!(
            new_after.y < new_before.y,
            "wheel scroll should move the new side upward"
        );
        assert_ne!(
            old_after.y, old_before.y,
            "old side should move when the new side scrolls"
        );
        assert_eq!(
            old_after.y, new_after.y,
            "old side should stay aligned with new side"
        );
    }

    #[gpui::test]
    async fn scrolling_old_side_of_side_by_side_diff_scrolls_new_side(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.select_changed_file("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-old-scroll")
            .expect("old file diff scroll debug bounds");
        let old_before = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset before wheel");
        let new_before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });

        let old_after = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset after wheel");
        let new_after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset after wheel");

        assert!(
            old_after.y < old_before.y,
            "wheel scroll should move the old side upward"
        );
        assert_ne!(
            new_after.y, new_before.y,
            "new side should move when the old side scrolls"
        );
        assert_eq!(
            old_after.y, new_after.y,
            "new side should stay aligned with old side"
        );
    }

    #[gpui::test]
    async fn opening_a_non_repo_stays_in_no_repo_mode_and_emits_a_notification(
        cx: &mut TestAppContext,
    ) {
        use std::cell::RefCell;
        use std::rc::Rc;

        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().to_path_buf();

        // The notification path renders gpui-component widgets that look up
        // the active theme; the theme global is installed by `gpui_component::init`.
        cx.update(gpui_component::init);

        let window = cx.add_window(App::new);
        let app_entity = window.entity(cx).expect("get app entity");

        // Subscribe to the App's `OpenFailed` event before triggering the open;
        // `Notification` keeps its message private, so this event is the only
        // public observation point for the user-facing string.
        let captured: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let captured_clone = captured.clone();
        let _subscription = app_entity.update(cx, |_, cx| {
            cx.subscribe(&app_entity, move |_, _, event: &OpenFailed, _| {
                captured_clone.borrow_mut().push(event.0.clone());
            })
        });

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("update window");

        cx.run_until_parked();

        window
            .read_with(cx, |app, cx| {
                assert!(matches!(app.mode, Mode::NoRepo), "mode unchanged");
                assert_eq!(
                    app.notification_count(cx),
                    1,
                    "exactly one notification pushed",
                );
            })
            .expect("read window");

        let events = captured.borrow();
        assert_eq!(events.len(), 1, "exactly one OpenFailed event emitted");
        assert!(
            events[0].contains("isn't a Git repository"),
            "notification body matches NotARepository, got {:?}",
            events[0],
        );
    }
}
