//! Top-level application entity and root view.

pub mod menu;
pub mod path_picker;
mod title_bar;

pub use menu::{
    bind_app_keys, build_app_menus, open_repository_key_binding, quit_application_key_binding,
    MenuSnapshot, GREVIEWER_MENU_LABEL, OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
    QUIT_APPLICATION_KEYSTROKE,
};
pub use path_picker::{repository_prompt_options, GpuiPathPicker, PathPicker, PathPickerOutcome};

use gpui::prelude::FluentBuilder;
use gpui::{
    actions, canvas, div, point, px, rgb, AnyElement, AppContext, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, InteractiveElement, IntoElement, Modifiers, ParentElement,
    PathBuilder, Pixels, Render, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement,
    Styled, Window,
};
use gpui_component::notification::{Notification, NotificationList};
use gpui_component::resizable::{h_resizable, resizable_panel, ResizableState};
use gpui_component::scroll::{Scrollbar, ScrollbarAxis, ScrollbarShow};
use gpui_component::tooltip::Tooltip;
use gpui_component::Icon;
use similar::{DiffTag, TextDiff};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::icons::LucideIcon;
use crate::settings::{self, RecentRepository, Settings, MAX_RECENT_REPOSITORIES};
use crate::{graph, repo};

actions!(
    app,
    [
        OpenRepository,
        OpenChangeset,
        CloseChangeset,
        QuitApplication
    ]
);

const FILE_TREE_FONT_FAMILY: &str = "BerkeleyMono Nerd Font";
const FILE_TREE_INDENT_WIDTH: f32 = 16.;
const FILE_TREE_ROW_HEIGHT: f32 = 24.;
const FILE_TREE_TEXT_SIZE: f32 = 14.;
const FILE_TREE_ROW_TEXT_LINE_HEIGHT: f32 = 20.;
const FILE_TREE_SECONDARY_TEXT_SIZE: f32 = 10.;
const FILE_TREE_BADGE_TEXT_SIZE: f32 = 9.;
const FILE_TREE_DIFF_STAT_TEXT_SIZE: f32 = 13.;
const FILE_TREE_FOLDER_ICON_SIZE: f32 = 16.;
const FILE_TREE_STATUS_ICON_SIZE: f32 = 14.;
const FILE_TREE_INDENT_GUIDE_WIDTH: f32 = 1.;
const FILE_TREE_GUIDE_TO_ITEM_GAP: f32 = 4.;
const FILE_TREE_CONTROL_BUTTON_SIZE: f32 = 22.;
const FILE_TREE_CONTROL_ICON_SIZE: f32 = 15.;

pub struct App {
    pub mode: Mode,
    pub selection: Selection,
    pub review_screen: ReviewScreen,
    pub selected_changed_file_path: Option<String>,
    pub file_list_mode: FileListMode,
    pub settings: Settings,
    collapsed_file_tree_paths: BTreeSet<String>,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    settings_store_path: Option<PathBuf>,
    file_diff_scroll: FileDiffScroll,
    commit_history_scroll: ScrollHandle,
    file_tree_scroll: ScrollHandle,
    /// True while the cursor is anywhere over the file-tree panel; gates the
    /// hover-revealed scrollbar overlay.
    file_tree_hovered: bool,
    changeset_resizable: Entity<ResizableState>,
    focus_handle: FocusHandle,
    /// Whether the title-bar context popover (the diff "switcher") is open.
    context_popover_open: bool,
    /// Whether the title-bar repo switcher (sibling-repository list) is open.
    repo_switcher_open: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileListMode {
    Changed,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileListEntry {
    Changed(repo::ChangedFile),
    Unchanged(repo::RepositoryFile),
}

impl FileListEntry {
    fn path(&self) -> &str {
        match self {
            FileListEntry::Changed(file) => &file.path,
            FileListEntry::Unchanged(file) => &file.path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileTreeRow {
    Folder {
        name: String,
        path: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        name: String,
        entry: FileListEntry,
        depth: usize,
    },
}

impl FileTreeRow {
    fn path(&self) -> &str {
        match self {
            FileTreeRow::Folder { path, .. } => path,
            FileTreeRow::File { entry, .. } => entry.path(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FileTreeBranch {
    folders: BTreeMap<String, FileTreeBranch>,
    files: Vec<FileTreeLeaf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileTreeLeaf {
    name: String,
    entry: FileListEntry,
}

/// Emitted whenever `open_repository_at` fails. Carries the user-facing
/// message that was pushed onto the notification list. Tests subscribe to
/// this event to verify the error path because `gpui-component`'s
/// `Notification` keeps its message field private.
#[derive(Debug, Clone)]
pub struct OpenFailed(pub String);

impl EventEmitter<OpenFailed> for App {}

/// Emitted immediately before the app asks gpui to quit. The gpui test
/// platform does not expose a quit flag, so app-shell tests observe this event
/// to verify that the public key-dispatch path reached the quit handler.
#[derive(Debug, Clone)]
pub struct QuitRequested;

impl EventEmitter<QuitRequested> for App {}

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings_store_path = settings::default_store_path();
        let settings = settings_store_path
            .as_deref()
            .map(settings::load)
            .unwrap_or_default();

        Self::new_with_picker_settings_and_store_path(
            window,
            cx,
            Box::new(GpuiPathPicker),
            settings,
            settings_store_path,
        )
    }

    pub fn new_with_picker(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
    ) -> Self {
        Self::new_with_picker_and_settings(window, cx, path_picker, Settings::default())
    }

    pub fn new_with_recent_repositories(
        window: &mut Window,
        cx: &mut Context<Self>,
        recent_repositories: Vec<RecentRepository>,
    ) -> Self {
        let settings = Settings {
            recent_repositories,
        };
        Self::new_with_picker_and_settings(window, cx, Box::new(GpuiPathPicker), settings)
    }

    #[cfg(test)]
    fn new_with_settings_store_path(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings_store_path: PathBuf,
    ) -> Self {
        let settings = settings::load(&settings_store_path);

        Self::new_with_picker_settings_and_store_path(
            window,
            cx,
            Box::new(GpuiPathPicker),
            settings,
            Some(settings_store_path),
        )
    }

    fn new_with_picker_and_settings(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
        settings: Settings,
    ) -> Self {
        Self::new_with_picker_settings_and_store_path(window, cx, path_picker, settings, None)
    }

    fn new_with_picker_settings_and_store_path(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
        settings: Settings,
        settings_store_path: Option<PathBuf>,
    ) -> Self {
        let notifications = cx.new(|cx| NotificationList::new(window, cx));
        let changeset_resizable = cx.new(|_| ResizableState::default());
        let focus_handle = cx.focus_handle();

        window.focus(&focus_handle);
        cx.on_next_frame(window, |app, window, _cx| {
            window.focus(&app.focus_handle);
        });

        let mut settings = settings;
        let mut mode = Mode::NoRepo;
        if settings
            .recent_repositories
            .first()
            .is_some_and(|first| first.available)
        {
            // The guard above guarantees a first entry, so indexing is sound.
            let path = settings.recent_repositories[0].path.clone();
            match repo::open_at(&path) {
                Ok(repo) => {
                    window.set_window_title(&repository_title(&repo.path));
                    mode = Mode::RepoOpen { repo };
                }
                // Construction-time fallback: no `&mut self` and no notification
                // list yet, so unlike `open_recent_repository` we mark the entry
                // unavailable and fall back to the recent screen silently.
                Err(_) => {
                    settings.recent_repositories[0].available = false;
                    if let Some(store_path) = settings_store_path.as_deref() {
                        let _ = settings::save(store_path, &settings);
                    }
                }
            }
        }

        Self {
            mode,
            selection: Selection::None,
            review_screen: ReviewScreen::Graph,
            selected_changed_file_path: None,
            file_list_mode: FileListMode::Changed,
            settings,
            collapsed_file_tree_paths: BTreeSet::new(),
            notifications,
            path_picker,
            settings_store_path,
            file_diff_scroll: FileDiffScroll::new(),
            commit_history_scroll: ScrollHandle::new(),
            file_tree_scroll: ScrollHandle::new(),
            file_tree_hovered: false,
            changeset_resizable,
            focus_handle,
            context_popover_open: false,
            repo_switcher_open: false,
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
            Ok(repo) => self.apply_open_repository(repo, window, cx),
            Err(err) => {
                let message = err.to_string();
                self.push_open_failed(message, window, cx);
            }
        }
    }

    fn open_recent_repository(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match repo::open_at(&path) {
            Ok(repo) => self.apply_open_repository(repo, window, cx),
            Err(err) => {
                self.mark_recent_repository_unavailable(&path);
                self.persist_settings();
                self.push_open_failed(err.to_string(), window, cx);
            }
        }
    }

    fn apply_open_repository(
        &mut self,
        repo: repo::OpenRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.set_window_title(&repository_title(&repo.path));
        let recent_path = repo.path.clone();

        self.mode = Mode::RepoOpen { repo };
        self.selection = Selection::None;
        self.review_screen = ReviewScreen::Graph;
        self.selected_changed_file_path = None;
        self.file_list_mode = FileListMode::Changed;
        self.collapsed_file_tree_paths.clear();
        self.record_recent_repository(recent_path);
        self.persist_settings();
        self.file_diff_scroll.reset();
        self.commit_history_scroll.set_offset(point(px(0.), px(0.)));
        self.file_tree_scroll.set_offset(point(px(0.), px(0.)));
        self.file_tree_hovered = false;
        self.context_popover_open = false;
        self.repo_switcher_open = false;
        cx.notify();
    }

    fn record_recent_repository(&mut self, path: PathBuf) {
        let recents = &mut self.settings.recent_repositories;
        recents.retain(|recent| recent.path != path);
        recents.insert(0, RecentRepository::available(path));
        recents.truncate(MAX_RECENT_REPOSITORIES);
    }

    fn mark_recent_repository_unavailable(&mut self, path: &PathBuf) {
        if let Some(recent) = self
            .settings
            .recent_repositories
            .iter_mut()
            .find(|recent| recent.path == *path)
        {
            recent.available = false;
        }
    }

    fn remove_recent_repository(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.settings
            .recent_repositories
            .retain(|recent| recent.path.as_path() != path);
        self.persist_settings();
        cx.notify();
    }

    fn persist_settings(&self) {
        if let Some(path) = &self.settings_store_path {
            let _ = settings::save(path, &self.settings);
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

        if !open_repo
            .commits
            .iter()
            .any(|commit| commit.sha == start_sha)
        {
            return Ok(None);
        }
        if !open_repo.commits.iter().any(|commit| commit.sha == end_sha) {
            return Ok(None);
        }

        if !repo::commits_share_linear_ancestry(&open_repo.path, start_sha, end_sha)? {
            return Ok(None);
        }

        Ok(commit_ancestry_path(&open_repo.commits, start_sha, end_sha))
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
                self.file_tree_scroll.set_offset(point(px(0.), px(0.)));
                self.file_tree_hovered = false;
                let sha = changeset.commit_sha.clone();
                self.review_screen = ReviewScreen::Changeset { sha, changeset };
                self.context_popover_open = false;
                cx.notify();
            }
            Err(err) => self.push_open_failed(err.to_string(), window, cx),
        }
    }

    fn close_changeset(&mut self, cx: &mut Context<Self>) {
        self.review_screen = ReviewScreen::Graph;
        self.context_popover_open = false;
        cx.notify();
    }

    fn quit_application(&mut self, cx: &mut Context<Self>) {
        cx.emit(QuitRequested);
        cx.quit();
    }

    fn select_changed_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.file_diff_scroll.reset();
        self.selected_changed_file_path = Some(path);
        cx.notify();
    }

    fn set_file_list_mode(&mut self, mode: FileListMode, cx: &mut Context<Self>) {
        if self.file_list_mode == mode {
            return;
        }

        self.file_list_mode = mode;
        cx.notify();
    }

    fn toggle_file_list_mode(&mut self, cx: &mut Context<Self>) {
        let next = match self.file_list_mode {
            FileListMode::Changed => FileListMode::All,
            FileListMode::All => FileListMode::Changed,
        };
        self.set_file_list_mode(next, cx);
    }

    /// Enumerate every folder in the tree built from `entries`, paired with
    /// whether it collapses by default in the current view. Used by the
    /// collapse-all / expand-all controls, which need the full folder universe
    /// regardless of what is currently visible.
    fn file_tree_folder_defaults(&self, entries: &[FileListEntry]) -> Vec<(String, bool)> {
        let collapse_unchanged_by_default = matches!(self.file_list_mode, FileListMode::All);
        let changed_ancestor_paths = if collapse_unchanged_by_default {
            changed_file_ancestor_paths(entries)
        } else {
            BTreeSet::new()
        };

        let mut root = FileTreeBranch::default();
        for entry in entries.iter().cloned() {
            insert_file_tree_entry(&mut root, entry);
        }

        let mut folders = Vec::new();
        collect_file_tree_folder_defaults(
            &root,
            "",
            collapse_unchanged_by_default,
            &changed_ancestor_paths,
            &mut folders,
        );
        folders
    }

    /// Drive every folder to `collapsed`. The collapse model stores a delta
    /// XOR'd against per-folder defaults, so a folder is recorded only when the
    /// desired state differs from its default.
    fn apply_folder_collapse(
        &mut self,
        folders: &[(String, bool)],
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        self.collapsed_file_tree_paths = folders
            .iter()
            .filter(|(_, collapsed_by_default)| collapsed != *collapsed_by_default)
            .map(|(path, _)| path.clone())
            .collect();
        cx.notify();
    }

    fn toggle_file_tree_folder(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.collapsed_file_tree_paths.insert(path.clone()) {
            self.collapsed_file_tree_paths.remove(&path);
        }
        cx.notify();
    }

    fn load_older_commits_after_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.commit_history_distance_from_bottom_after_scroll(event, window) > px(84.) {
            return;
        }

        self.load_older_commits(window, cx);
    }

    fn commit_history_distance_from_bottom_after_scroll(
        &self,
        event: &ScrollWheelEvent,
        window: &Window,
    ) -> Pixels {
        let max_offset = self.commit_history_scroll.max_offset().height;
        let current_offset = self.commit_history_scroll.offset().y;
        let delta = event.delta.pixel_delta(window.line_height()).y;
        let next_offset = (current_offset + delta).clamp(-max_offset, px(0.));

        max_offset + next_offset
    }

    fn load_older_commits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (path, oldest_sha) = match &self.mode {
            Mode::RepoOpen { repo } if repo.has_more_commits => {
                let Some(oldest_commit) = repo.commits.last() else {
                    return;
                };
                (repo.path.clone(), oldest_commit.sha.clone())
            }
            _ => return,
        };

        match repo::load_commits_after(&path, &oldest_sha) {
            Ok(page) => {
                if let Mode::RepoOpen { repo } = &mut self.mode {
                    repo.commits.extend(page.commits);
                    repo.has_more_commits = page.has_more;
                    cx.notify();
                }
            }
            Err(err) => self.push_open_failed(err.to_string(), window, cx),
        }
    }

    fn is_commit_selected(&self, sha: &str) -> bool {
        match &self.selection {
            Selection::Single { sha: selected_sha } => selected_sha == sha,
            Selection::Range { shas, .. } => shas.iter().any(|selected_sha| selected_sha == sha),
            Selection::None => false,
        }
    }

    fn is_file_path_selected(&self, path: &str) -> bool {
        self.selected_changed_file_path.as_deref() == Some(path)
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

    fn render_no_repo(&self, cx: &mut Context<Self>) -> gpui::Div {
        let recent_repositories = if self.settings.recent_repositories.is_empty() {
            None
        } else {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .w(px(520.))
                    .max_w_full()
                    .mt_6()
                    .border_1()
                    .border_color(rgb(0x2a2a2a))
                    .bg(rgb(0x141414))
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(rgb(0x242424))
                            .text_color(rgb(0x999999))
                            .text_size(px(12.))
                            .child("Recent repositories"),
                    )
                    .children(
                        self.settings
                            .recent_repositories
                            .iter()
                            .enumerate()
                            .map(|(index, recent)| {
                                self.render_recent_repository_row(index, recent, cx)
                            })
                            .collect::<Vec<_>>(),
                    ),
            )
        };

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
            .when_some(recent_repositories, |view, recent| view.child(recent))
    }

    fn render_recent_repository_row(
        &self,
        index: usize,
        recent: &RecentRepository,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open_path = recent.path.clone();
        let remove_path = recent.path.clone();
        let display_path = recent.path.display().to_string();
        let debug_selector = if recent.available {
            format!("recent-repository-row-{index}")
        } else {
            format!("unavailable-recent-repository-row-{index}")
        };
        let remove_selector = format!("unavailable-recent-repository-remove-{index}");
        let path_color = if recent.available {
            rgb(0xe6e6e6)
        } else {
            rgb(0x777777)
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .gap_3()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x242424))
            .cursor_pointer()
            .id(("recent-repository-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, window, cx| {
                app.open_recent_repository(open_path.clone(), window, cx);
            }))
            .child(
                div()
                    .flex_1()
                    .font_family("monospace")
                    .text_size(px(13.))
                    .text_color(path_color)
                    .child(display_path),
            )
            .when(!recent.available, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .border_1()
                                .border_color(rgb(0x5a2a2a))
                                .bg(rgb(0x241818))
                                .text_color(rgb(0xfca5a5))
                                .text_size(px(11.))
                                .child("Unavailable"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .border_1()
                                .border_color(rgb(0x3a3a3a))
                                .bg(rgb(0x1f1f1f))
                                .text_color(rgb(0xbdbdbd))
                                .text_size(px(11.))
                                .cursor_pointer()
                                .id(("unavailable-recent-repository-remove", index))
                                .debug_selector(move || remove_selector.clone())
                                .on_click(cx.listener(move |app, _event, _window, cx| {
                                    app.remove_recent_repository(&remove_path, cx);
                                    cx.stop_propagation();
                                }))
                                .child("Remove"),
                        ),
                )
            })
    }

    fn render_repo_open(&self, repo: &repo::OpenRepository, cx: &mut Context<Self>) -> AnyElement {
        match &self.review_screen {
            ReviewScreen::Graph => self.render_graph_screen(repo, cx).into_any_element(),
            ReviewScreen::Changeset { changeset, .. } => self
                .render_changeset_screen(repo, changeset, cx)
                .into_any_element(),
        }
    }

    fn render_graph_screen(
        &self,
        repo: &repo::OpenRepository,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let can_open_changeset = matches!(
            self.selection,
            Selection::Single { .. } | Selection::Range { .. }
        );

        let history = if repo.commits.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("commit-history-empty")
                .debug_selector(|| "commit-history-empty".to_string())
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("This repository has no commits to review.")
        } else {
            let graph_commits = repo
                .commits
                .iter()
                .map(|commit| graph::GraphCommit {
                    sha: commit.sha.clone(),
                    authored_timestamp: commit.authored_timestamp,
                    parent_shas: commit.parent_shas.clone(),
                })
                .collect::<Vec<_>>();
            let graph_rows = graph::layout_graph(&graph_commits);
            let max_graph_lanes = graph_rows
                .iter()
                .map(|row| row.lane_count)
                .max()
                .unwrap_or(1);

            let commit_rows = repo
                .commits
                .iter()
                .zip(graph_rows.iter())
                .enumerate()
                .map(|(index, (commit, graph_row))| {
                    self.render_commit_row(
                        index,
                        commit,
                        graph_row,
                        max_graph_lanes,
                        self.is_commit_selected(&commit.sha),
                        cx,
                    )
                })
                .collect::<Vec<_>>();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .id("commit-history")
                .overflow_y_scroll()
                .scrollbar_width(px(12.))
                .track_scroll(&self.commit_history_scroll)
                .on_scroll_wheel(cx.listener(|app, event, window, cx| {
                    app.load_older_commits_after_scroll(event, window, cx);
                }))
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .w_full()
                        .children(commit_rows)
                        .child(render_commit_graph_history_overlay(
                            &graph_rows,
                            max_graph_lanes,
                        )),
                )
        };

        div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgb(0x171717))
            .child(history)
            // The open-changeset control floats over the top-right of the graph,
            // outside the scroll area, so it stays pinned while the history
            // scrolls. The repo/HEAD context that used to sit beside it is
            // moving into the window bar.
            .when(can_open_changeset, |screen| {
                screen.child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .right(px(2.))
                        .occlude()
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
            })
    }

    fn render_changeset_screen(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let body: AnyElement = match self.file_list_entries(repo, changeset) {
            Ok(entries) => {
                let selected_path = self
                    .selected_changed_file_path
                    .as_deref()
                    .filter(|path| entries.iter().any(|entry| entry.path() == *path));

                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        h_resizable("changeset-split")
                            .with_state(&self.changeset_resizable)
                            .child(
                                resizable_panel()
                                    .size(px(340.))
                                    .child(self.render_file_list(entries, cx)),
                            )
                            .child(resizable_panel().child(self.render_file_detail(
                                repo,
                                changeset,
                                selected_path,
                            ))),
                    )
                    .into_any_element()
            }
            Err(err) => render_file_diff_error(err.to_string()),
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgb(0x171717))
            .child(body)
    }

    fn file_list_entries(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
    ) -> Result<Vec<FileListEntry>, repo::ChangeSetError> {
        match self.file_list_mode {
            FileListMode::Changed => Ok(changeset
                .files
                .iter()
                .cloned()
                .map(FileListEntry::Changed)
                .collect()),
            FileListMode::All => {
                repo::files_at_commit(&repo.path, &changeset.commit_sha).map(|files| {
                    files
                        .into_iter()
                        .map(|file| {
                            changeset
                                .files
                                .iter()
                                .find(|changed_file| changed_file.path == file.path)
                                .cloned()
                                .map(FileListEntry::Changed)
                                .unwrap_or(FileListEntry::Unchanged(file))
                        })
                        .collect()
                })
            }
        }
    }

    fn render_file_list(
        &self,
        entries: Vec<FileListEntry>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let folder_defaults = self.file_tree_folder_defaults(&entries);
        let rows = self.file_tree_rows(entries);
        let list_content: AnyElement = if rows.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("changed-files-empty")
                .debug_selector(|| "changed-files-empty".to_string())
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("This changeset has no net file changes.")
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                // items_start() prevents the cross-axis stretch that would pin the
                // inner wrapper to the viewport width, defeating horizontal scrolling.
                .items_start()
                .flex_1()
                .min_h_0()
                .id("changed-files-scroll")
                .debug_selector(|| "changed-files-scroll".to_string())
                .overflow_scroll()
                .track_scroll(&self.file_tree_scroll)
                .child(
                    // Inner flex_none wrapper sizes to the widest row's natural
                    // width instead of being stretched to the viewport, which is
                    // what enables horizontal scrolling. Every row uses w_full() to
                    // fill this wrapper so all rows share a uniform background width.
                    div().flex().flex_col().flex_none().children(
                        rows.iter()
                            .enumerate()
                            .map(|(index, row)| self.render_file_tree_row(index, row, cx))
                            .collect::<Vec<_>>(),
                    ),
                )
                .into_any_element()
        };

        div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .min_h_0()
            .id("changed-files")
            .debug_selector(|| "changed-files".to_string())
            .border_1()
            .border_color(rgb(0x242424))
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                if app.file_tree_hovered != *hovered {
                    app.file_tree_hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(list_content)
            .when(self.file_tree_hovered, |container| {
                container.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .debug_selector(|| "file-tree-scrollbar".to_string())
                        .child(
                            Scrollbar::new(&self.file_tree_scroll)
                                .axis(ScrollbarAxis::Both)
                                // Always: render both tracks whenever the
                                // hover gate has placed this overlay. The
                                // component's Hover mode keys off the scroll
                                // area's own hover, not the whole panel's, so
                                // we gate visibility ourselves via the `.when`
                                // above and let the component always paint.
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
            })
            .child(self.render_file_tree_controls(folder_defaults, cx))
    }

    fn file_tree_rows(&self, entries: Vec<FileListEntry>) -> Vec<FileTreeRow> {
        // In "all files" mode the tree includes folders that hold no changes,
        // which buries the diff in noise. Collapse those by default while
        // keeping the folders that lead to a changed file expanded so the user
        // can still see the diff at a glance. Manual toggles override this.
        let collapse_unchanged_by_default = matches!(self.file_list_mode, FileListMode::All);
        let changed_ancestor_paths = if collapse_unchanged_by_default {
            changed_file_ancestor_paths(&entries)
        } else {
            BTreeSet::new()
        };

        let mut root = FileTreeBranch::default();

        for entry in entries {
            insert_file_tree_entry(&mut root, entry);
        }

        let mut rows = Vec::new();
        append_file_tree_rows(
            &root,
            0,
            "",
            &self.collapsed_file_tree_paths,
            collapse_unchanged_by_default,
            &changed_ancestor_paths,
            &mut rows,
        );
        rows
    }

    /// The icon-only controls that float over the top-right of the file tree:
    /// a show-all-files toggle plus collapse-all / expand-all. The controls sit
    /// outside the scroll area so they stay pinned while the tree scrolls.
    fn render_file_tree_controls(
        &self,
        folder_defaults: Vec<(String, bool)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let collapse_folders = folder_defaults.clone();
        let expand_folders = folder_defaults;
        let show_all_active = matches!(self.file_list_mode, FileListMode::All);

        div()
            .absolute()
            .top(px(2.))
            .right(px(2.))
            .flex()
            .items_center()
            .gap(px(2.))
            // The controls float over the first tree row; occlude so clicks
            // land on the buttons instead of falling through to the row.
            .occlude()
            .child(self.render_file_tree_icon_button(
                LucideIcon::ListTree,
                "file-list-mode-toggle",
                "Show all files",
                show_all_active,
                |app, _window, cx| app.toggle_file_list_mode(cx),
                cx,
            ))
            .child(self.render_file_tree_icon_button(
                LucideIcon::ChevronsDownUp,
                "file-tree-collapse-all",
                "Collapse all",
                false,
                move |app, _window, cx| app.apply_folder_collapse(&collapse_folders, true, cx),
                cx,
            ))
            .child(self.render_file_tree_icon_button(
                LucideIcon::ChevronsUpDown,
                "file-tree-expand-all",
                "Expand all",
                false,
                move |app, _window, cx| app.apply_folder_collapse(&expand_folders, false, cx),
                cx,
            ))
    }

    fn render_file_tree_icon_button(
        &self,
        icon: LucideIcon,
        selector: &'static str,
        tooltip: &'static str,
        active: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let background = if active { rgb(0x1d283a) } else { rgb(0x202020) };
        let text_color = if active { rgb(0xdbeafe) } else { rgb(0x999999) };

        div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .flex()
            .items_center()
            .justify_center()
            .size(px(FILE_TREE_CONTROL_BUTTON_SIZE))
            .rounded(px(4.))
            .bg(background)
            .text_color(text_color)
            .cursor_pointer()
            .hover(|style| style.bg(rgb(0x2c3a4f)).text_color(rgb(0xdbeafe)))
            .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
            .on_click(cx.listener(move |app, _event, window, cx| on_click(app, window, cx)))
            .child(
                Icon::new(icon)
                    .text_color(text_color)
                    .size(px(FILE_TREE_CONTROL_ICON_SIZE)),
            )
    }

    fn render_file_tree_row(
        &self,
        index: usize,
        row: &FileTreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            FileTreeRow::Folder {
                name,
                path,
                depth,
                collapsed,
            } => self
                .render_file_tree_folder_row(index, name, path, *depth, *collapsed, cx)
                .into_any_element(),
            FileTreeRow::File { name, entry, depth } => {
                let selected = self.is_file_path_selected(row.path());
                match entry {
                    FileListEntry::Changed(file) => self
                        .render_changed_file_row(index, file, selected, *depth, name, cx)
                        .into_any_element(),
                    FileListEntry::Unchanged(file) => self
                        .render_unchanged_file_row(index, file, selected, *depth, name, cx)
                        .into_any_element(),
                }
            }
        }
    }

    fn render_file_tree_folder_row(
        &self,
        index: usize,
        name: &str,
        path: &str,
        depth: usize,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = path.to_string();
        let debug_selector = format!("file-tree-folder-{}", debug_path_fragment(&path));
        let path_fragment = debug_path_fragment(&path);

        div()
            .flex()
            .items_center()
            .w_full()
            .min_h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_2()
            .bg(rgb(0x171717))
            .cursor_pointer()
            .id(("file-tree-folder", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.toggle_file_tree_folder(path.clone(), cx);
            }))
            .child(render_file_tree_indent_guides(depth, &path_fragment))
            .child(render_file_tree_folder_icon(
                &path_fragment,
                collapsed,
                rgb(0x8aa6bd),
            ))
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(0x8aa6bd))
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
                    .font_family(FILE_TREE_FONT_FAMILY)
                    .whitespace_nowrap()
                    .child(name.to_string()),
            )
    }

    fn render_changed_file_row(
        &self,
        index: usize,
        file: &repo::ChangedFile,
        selected: bool,
        depth: usize,
        display_name: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = file.path.clone();
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let debug_selector = if selected {
            format!("selected-changed-file-row-{index}")
        } else {
            format!("changed-file-row-{index}")
        };
        let path_fragment = debug_path_fragment(&file.path);
        let kind_selector = format!("changed-file-kind-{path_fragment}");
        let binary_selector = format!("changed-file-binary-indicator-{path_fragment}");
        let rename_source_selector = format!("changed-file-rename-source-{path_fragment}");
        let status_icon_selector = format!("changed-file-status-icon-{path_fragment}");
        let diff_stat_selector = format!("changed-file-diff-stat-{path_fragment}");
        let file_name_selector = format!("changed-file-name-{path_fragment}");
        let deleted_strike_selector = format!("changed-file-deleted-strike-{path_fragment}");

        div()
            .flex()
            .items_center()
            .w_full()
            .min_h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_2()
            .bg(row_bg)
            .cursor_pointer()
            .id(("changed-file-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.select_changed_file(path.clone(), cx);
            }))
            .child(render_file_tree_indent_guides(depth, &path_fragment))
            .child(render_change_status_icon(
                file.kind,
                kind_selector,
                status_icon_selector,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .gap_2()
                    .child(render_file_tree_file_name(
                        file_name_selector,
                        display_name,
                        file.kind == repo::ChangeKind::Deleted,
                        deleted_strike_selector,
                    ))
                    .when(file.is_binary, |row| {
                        row.child(
                            div()
                                .px_1()
                                .py_0p5()
                                .border_1()
                                .border_color(rgb(0x525252))
                                .bg(rgb(0x242424))
                                .text_color(rgb(0xbdbdbd))
                                .text_size(px(FILE_TREE_BADGE_TEXT_SIZE))
                                .font_family(FILE_TREE_FONT_FAMILY)
                                .debug_selector(move || binary_selector.clone())
                                .child("Binary"),
                        )
                    })
                    .when_some(file.old_path.clone(), |row, old_path| {
                        row.child(
                            div()
                                .text_color(rgb(0x8a8a8a))
                                .text_size(px(FILE_TREE_SECONDARY_TEXT_SIZE))
                                .font_family(FILE_TREE_FONT_FAMILY)
                                .whitespace_nowrap()
                                .debug_selector(move || rename_source_selector.clone())
                                .child(format!("from {old_path}")),
                        )
                    }),
            )
            .child(render_file_diff_stat(diff_stat_selector, file.line_stats))
    }

    fn render_unchanged_file_row(
        &self,
        index: usize,
        file: &repo::RepositoryFile,
        selected: bool,
        depth: usize,
        display_name: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = file.path.clone();
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let debug_selector = if selected {
            format!("selected-unchanged-file-row-{index}")
        } else {
            format!("unchanged-file-row-{index}")
        };

        div()
            .flex()
            .items_center()
            .w_full()
            .min_h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_2()
            .bg(row_bg)
            .cursor_pointer()
            .id(("unchanged-file-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.select_changed_file(path.clone(), cx);
            }))
            .child(render_file_tree_indent_guides(
                depth,
                &debug_path_fragment(&file.path),
            ))
            .child(render_file_tree_file_icon(
                format!("unchanged-file-icon-{}", debug_path_fragment(&file.path)),
                rgb(0x6f7d87),
            ))
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(0xb8c0c7))
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
                    .font_family(FILE_TREE_FONT_FAMILY)
                    .whitespace_nowrap()
                    .child(display_name.to_string()),
            )
    }

    fn render_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        selected_path: Option<&str>,
    ) -> AnyElement {
        match selected_path {
            Some(path) => {
                if let Some(file) = changeset.files.iter().find(|file| file.path == path) {
                    return self.render_changed_file_detail(repo, changeset, file);
                }

                self.render_read_only_file_detail(repo, changeset, path)
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

    fn render_changed_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        file: &repo::ChangedFile,
    ) -> AnyElement {
        let title = file.path.clone();
        let kind = change_kind_label(file.kind);
        let rename_source_selector = format!(
            "file-detail-rename-source-{}",
            debug_path_fragment(&file.path)
        );
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
                        .debug_selector(move || rename_source_selector.clone())
                        .child(format!("Renamed from {old_path}")),
                )
            })
            .child(content)
            .into_any_element()
    }

    fn render_read_only_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        path: &str,
    ) -> AnyElement {
        let content = match repo::file_content_at_commit(&repo.path, &changeset.commit_sha, path) {
            Ok(content) => render_file_content(content.content, &self.file_diff_scroll),
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
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(16.))
                    .font_family("monospace")
                    .child(path.to_string()),
            )
            .child(content)
            .into_any_element()
    }

    fn render_commit_row(
        &self,
        index: usize,
        commit: &repo::CommitInfo,
        _graph_row: &graph::GraphRow,
        max_graph_lanes: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
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
            .h(px(COMMIT_ROW_HEIGHT))
            .gap_3()
            .px_4()
            .bg(row_bg)
            .when(commit_row_separator_width() > 0., |row| {
                row.border_b(px(commit_row_separator_width()))
                    .border_color(commit_row_separator_color(selected))
            })
            .cursor_pointer()
            .id(("commit-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, event: &ClickEvent, window, cx| {
                app.select_commit(sha.clone(), event.modifiers(), window, cx);
            }))
            .child(render_commit_graph_gutter_spacer(max_graph_lanes))
            .child(
                div()
                    .w(px(COMMIT_HASH_WIDTH))
                    .flex_shrink_0()
                    .text_color(rgb(0xa3e635))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .debug_selector(move || format!("commit-hash-{index}"))
                    .child(commit.short_sha.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(14.))
                    .truncate()
                    .debug_selector(move || format!("commit-summary-{index}"))
                    .child(commit.summary.clone()),
            )
            .child(
                div()
                    .w(px(COMMIT_AUTHOR_WIDTH))
                    .flex_shrink_0()
                    .min_w_0()
                    .text_color(rgb(0xa3a3a3))
                    .text_size(px(12.))
                    .truncate()
                    .debug_selector(move || format!("commit-author-{index}"))
                    .child(commit.author.clone()),
            )
            .child(
                div()
                    .w(px(COMMIT_TIME_WIDTH))
                    .flex_shrink_0()
                    .text_color(rgb(0x8a8a8a))
                    .text_size(px(12.))
                    .debug_selector(move || format!("commit-time-{index}"))
                    .child(commit.authored_date.clone()),
            )
            .child(render_commit_ref_labels(index, commit))
    }
}

const COMMIT_ROW_HEIGHT: f32 = 44.;
const COMMIT_ROW_HORIZONTAL_PADDING: f32 = 16.;
const COMMIT_HASH_WIDTH: f32 = 72.;
const COMMIT_AUTHOR_WIDTH: f32 = 168.;
const COMMIT_TIME_WIDTH: f32 = 96.;

fn commit_row_separator_width() -> f32 {
    0.
}

fn commit_row_separator_color(selected: bool) -> gpui::Rgba {
    if selected {
        rgb(0x3b82f6)
    } else {
        rgb(0x242424)
    }
}

fn render_commit_ref_labels(row_index: usize, commit: &repo::CommitInfo) -> gpui::Div {
    let mut labels = Vec::new();
    if commit.is_head {
        labels.push(CommitRefLabel {
            name: "HEAD".to_string(),
            kind: CommitRefLabelKind::Head,
        });
    }
    labels.extend(
        commit
            .branch_names
            .iter()
            .cloned()
            .map(|name| CommitRefLabel {
                name,
                kind: CommitRefLabelKind::Branch,
            }),
    );

    div()
        .flex()
        .items_center()
        .gap_1()
        .w(px(COMMIT_REF_LABELS_WIDTH))
        .overflow_hidden()
        .flex_shrink_0()
        .debug_selector(move || format!("commit-ref-labels-{row_index}"))
        .children(
            labels
                .into_iter()
                .map(|label| render_commit_ref_label(row_index, label))
                .collect::<Vec<_>>(),
        )
}

const COMMIT_REF_LABELS_WIDTH: f32 = 156.;
const COMMIT_REF_LABEL_MAX_WIDTH: f32 = COMMIT_REF_LABELS_WIDTH - 8.;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitRefLabel {
    name: String,
    kind: CommitRefLabelKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitRefLabelKind {
    Head,
    Branch,
}

fn render_commit_ref_label(row_index: usize, label: CommitRefLabel) -> gpui::Div {
    let selector = format!(
        "commit-ref-label-{row_index}-{}",
        debug_ref_label_fragment(&label.name)
    );
    let (border_color, background, text_color) = match label.kind {
        CommitRefLabelKind::Head => (rgb(0x0ea5e9), rgb(0x102536), rgb(0x7dd3fc)),
        CommitRefLabelKind::Branch => (rgb(0x3f6212), rgb(0x17230f), rgb(0xa3e635)),
    };

    div()
        .px_1()
        .py_0p5()
        .border_1()
        .border_color(border_color)
        .bg(background)
        .text_color(text_color)
        .text_size(px(10.))
        .font_family("monospace")
        .max_w(px(COMMIT_REF_LABEL_MAX_WIDTH))
        .truncate()
        .debug_selector(move || selector.clone())
        .child(label.name)
}

fn render_commit_graph_gutter(
    row_index: usize,
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    max_lanes: usize,
) -> impl IntoElement {
    let lane_count = max_lanes.max(1);
    let debug_selector = format!("commit-graph-gutter-{row_index}");

    div()
        .flex()
        .items_center()
        .w(px(commit_graph_gutter_width(lane_count)))
        .font_family("monospace")
        .id(("commit-graph-gutter", row_index))
        .debug_selector(move || debug_selector.clone())
        .children(
            (0..lane_count)
                .map(|lane| render_commit_graph_lane(row_index, lane, row, previous_row))
                .collect::<Vec<_>>(),
        )
}

fn render_commit_graph_history_overlay(
    rows: &[graph::GraphRow],
    max_lanes: usize,
) -> impl IntoElement {
    let lane_count = max_lanes.max(1);
    let height = rows.len() as f32 * COMMIT_ROW_HEIGHT;

    div()
        .absolute()
        .left(px(COMMIT_ROW_HORIZONTAL_PADDING))
        .top_0()
        .w(px(commit_graph_gutter_width(lane_count)))
        .h(px(height))
        .debug_selector(|| "commit-graph-overlay".to_string())
        .child(
            div().relative().w_full().h(px(height)).children(
                commit_graph_overlay_row_indices(rows.len())
                    .into_iter()
                    .map(|row_index| {
                        div()
                            .absolute()
                            .left_0()
                            .top(px(row_index as f32 * COMMIT_ROW_HEIGHT))
                            .child(render_commit_graph_gutter(
                                row_index,
                                &rows[row_index],
                                row_index
                                    .checked_sub(1)
                                    .and_then(|previous_row| rows.get(previous_row)),
                                lane_count,
                            ))
                    })
                    .collect::<Vec<_>>(),
            ),
        )
}

fn commit_graph_overlay_row_indices(row_count: usize) -> Vec<usize> {
    (0..row_count).rev().collect()
}

fn render_commit_graph_gutter_spacer(max_lanes: usize) -> impl IntoElement {
    div()
        .w(px(commit_graph_gutter_width(max_lanes.max(1))))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .flex_shrink_0()
}

fn commit_graph_gutter_width(lane_count: usize) -> f32 {
    (lane_count as f32 * COMMIT_GRAPH_LANE_WIDTH).max(COMMIT_GRAPH_LANE_WIDTH * 2.)
}

const COMMIT_GRAPH_LANE_WIDTH: f32 = 22.;
const COMMIT_GRAPH_LANE_HEIGHT: f32 = COMMIT_ROW_HEIGHT;
const COMMIT_GRAPH_MIDDLE_HEIGHT: f32 = 10.;
const COMMIT_GRAPH_VERTICAL_HEIGHT: f32 =
    (COMMIT_GRAPH_LANE_HEIGHT - COMMIT_GRAPH_MIDDLE_HEIGHT) / 2.;
const COMMIT_GRAPH_LINE_WIDTH: f32 = 2.;
const COMMIT_GRAPH_DOT_SIZE: f32 = 8.;
const COMMIT_GRAPH_BEND_RADIUS: f32 = 8.;
const COMMIT_GRAPH_BEND_CUBIC_CONTROL: f32 = 0.552_284_8;

fn commit_graph_line_x() -> f32 {
    (COMMIT_GRAPH_LANE_WIDTH - COMMIT_GRAPH_LINE_WIDTH) / 2.
}

fn commit_graph_right_line_x() -> f32 {
    commit_graph_line_x() + COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_right_line_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH - commit_graph_right_line_x()
}

fn commit_graph_middle_line_y() -> f32 {
    (commit_graph_middle_height() - COMMIT_GRAPH_LINE_WIDTH) / 2.
}

fn commit_graph_middle_line_bottom_y() -> f32 {
    commit_graph_middle_line_y() + COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_bend_radius() -> f32 {
    COMMIT_GRAPH_BEND_RADIUS
}

fn commit_graph_middle_height() -> f32 {
    COMMIT_GRAPH_MIDDLE_HEIGHT
}

fn commit_graph_line_width() -> f32 {
    COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_bend_overlay_height() -> f32 {
    COMMIT_GRAPH_LANE_HEIGHT
}

fn commit_graph_bend_overlay_top() -> f32 {
    -COMMIT_GRAPH_VERTICAL_HEIGHT
}

fn commit_graph_bend_overlay_x() -> f32 {
    -COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_bend_overlay_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH + COMMIT_GRAPH_LINE_WIDTH * 2.
}

fn commit_graph_bend_overlay_lane_offset_x() -> f32 {
    COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_commit_bend_overlay_x() -> f32 {
    -COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_commit_bend_overlay_width() -> f32 {
    commit_graph_commit_bend_overlay_dot_center_x() + COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_commit_bend_overlay_dot_center_x() -> f32 {
    -commit_graph_commit_bend_overlay_x()
        + commit_graph_dot_side_line_width()
        + COMMIT_GRAPH_DOT_SIZE / 2.
}

fn commit_graph_merge_target_commit_bend_overlay_x() -> f32 {
    0.
}

fn commit_graph_merge_target_commit_bend_overlay_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH
}

fn commit_graph_merge_target_commit_bend_overlay_dot_center_x() -> f32 {
    -commit_graph_merge_target_commit_bend_overlay_x()
        + commit_graph_dot_side_line_width()
        + COMMIT_GRAPH_DOT_SIZE / 2.
}

fn commit_graph_merge_in_commit_bend_end_y() -> f32 {
    -commit_graph_bend_overlay_top()
        + commit_graph_dot_bottom_gap_y()
        + commit_graph_line_width() / 2.
}

fn commit_graph_merge_in_commit_line_y() -> f32 {
    commit_graph_merge_in_commit_bend_end_y() + commit_graph_bend_radius()
}

fn commit_graph_merge_in_commit_line_y_in_middle() -> f32 {
    commit_graph_merge_in_commit_line_y() - COMMIT_GRAPH_VERTICAL_HEIGHT
}

fn commit_graph_dot_gap_height() -> f32 {
    (COMMIT_GRAPH_MIDDLE_HEIGHT - COMMIT_GRAPH_DOT_SIZE) / 2.
}

fn commit_graph_dot_bottom_gap_y() -> f32 {
    commit_graph_dot_gap_height() + COMMIT_GRAPH_DOT_SIZE
}

fn commit_graph_dot_side_line_width() -> f32 {
    (COMMIT_GRAPH_LANE_WIDTH - COMMIT_GRAPH_DOT_SIZE) / 2.
}

#[derive(Debug, Clone, Copy)]
struct CommitGraphPoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Copy)]
struct CommitGraphCubicBend {
    start: CommitGraphPoint,
    first_control: CommitGraphPoint,
    second_control: CommitGraphPoint,
    end: CommitGraphPoint,
}

#[derive(Debug, Clone, Copy)]
struct CommitGraphRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy)]
struct CommitGraphBranchOffSourceBend {
    curve: CommitGraphCubicBend,
    horizontal_end: Option<CommitGraphPoint>,
}

fn commit_graph_lower_merge_in_horizontal_top_in_middle() -> f32 {
    commit_graph_merge_in_commit_line_y_in_middle() - commit_graph_line_width() / 2.
}

fn commit_graph_lower_connector_vertical_shift() -> f32 {
    COMMIT_GRAPH_LANE_HEIGHT
        - COMMIT_GRAPH_VERTICAL_HEIGHT
        - commit_graph_merge_in_commit_line_y_in_middle()
}

fn commit_graph_shifted_lower_merge_in_horizontal_top_in_middle() -> f32 {
    commit_graph_lower_merge_in_horizontal_top_in_middle()
        + commit_graph_lower_connector_vertical_shift()
}

fn commit_graph_shifted_bend_overlay_height() -> f32 {
    commit_graph_bend_overlay_height() + commit_graph_lower_connector_vertical_shift()
}

fn commit_graph_branch_off_source_bend_geometry(
    _spans_occupied_lanes: bool,
) -> CommitGraphBranchOffSourceBend {
    let lane_offset_x = commit_graph_bend_overlay_lane_offset_x();
    let center_x = lane_offset_x + commit_graph_line_x() + commit_graph_line_width() / 2.;
    let lower_line_y = commit_graph_merge_in_commit_line_y();
    let radius = commit_graph_bend_radius();
    let curve_end_x = center_x + radius;
    let horizontal_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let vertical_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let curve_end = CommitGraphPoint {
        x: curve_end_x,
        y: lower_line_y,
    };

    CommitGraphBranchOffSourceBend {
        curve: CommitGraphCubicBend {
            start: CommitGraphPoint {
                x: center_x,
                y: lower_line_y + radius,
            },
            first_control: CommitGraphPoint {
                x: center_x,
                y: lower_line_y + radius - vertical_control,
            },
            second_control: CommitGraphPoint {
                x: curve_end_x - horizontal_control,
                y: lower_line_y,
            },
            end: curve_end,
        },
        horizontal_end: Some(CommitGraphPoint {
            x: COMMIT_GRAPH_LANE_WIDTH - commit_graph_bend_overlay_x(),
            y: lower_line_y,
        }),
    }
}

fn commit_graph_merge_in_commit_bend_geometry() -> CommitGraphCubicBend {
    let end_x = commit_graph_commit_bend_overlay_dot_center_x();
    let end_y = commit_graph_merge_in_commit_bend_end_y();
    let radius = commit_graph_bend_radius();
    let start_x = end_x - radius;
    let lower_line_y = commit_graph_merge_in_commit_line_y();
    let horizontal_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let vertical_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;

    CommitGraphCubicBend {
        start: CommitGraphPoint {
            x: start_x,
            y: lower_line_y,
        },
        first_control: CommitGraphPoint {
            x: start_x + horizontal_control,
            y: lower_line_y,
        },
        second_control: CommitGraphPoint {
            x: end_x,
            y: end_y + vertical_control,
        },
        end: CommitGraphPoint { x: end_x, y: end_y },
    }
}

fn commit_graph_merge_target_commit_bend_geometry() -> CommitGraphCubicBend {
    let end_x = commit_graph_merge_target_commit_bend_overlay_dot_center_x();
    let end_y = commit_graph_merge_in_commit_bend_end_y();
    let radius = commit_graph_bend_radius();
    let start_x = end_x + radius;
    let lower_line_y = commit_graph_merge_in_commit_line_y();
    let horizontal_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;
    let vertical_control = radius * COMMIT_GRAPH_BEND_CUBIC_CONTROL;

    CommitGraphCubicBend {
        start: CommitGraphPoint {
            x: start_x,
            y: lower_line_y,
        },
        first_control: CommitGraphPoint {
            x: start_x - horizontal_control,
            y: lower_line_y,
        },
        second_control: CommitGraphPoint {
            x: end_x,
            y: end_y + vertical_control,
        },
        end: CommitGraphPoint { x: end_x, y: end_y },
    }
}

fn commit_graph_merge_in_commit_dot_connector_geometry() -> CommitGraphRect {
    let bend = commit_graph_merge_in_commit_bend_geometry();
    let width = commit_graph_line_width();
    CommitGraphRect {
        x: bend.end.x - width / 2.,
        y: -commit_graph_bend_overlay_top() + commit_graph_dot_bottom_gap_y(),
        width,
        height: width,
    }
}

fn commit_graph_shifted_merge_in_commit_dot_connector_geometry() -> CommitGraphRect {
    let bend = commit_graph_merge_in_commit_bend_geometry();
    let connector = commit_graph_merge_in_commit_dot_connector_geometry();

    CommitGraphRect {
        height: bend.end.y + commit_graph_lower_connector_vertical_shift() - connector.y
            + commit_graph_line_width() / 2.,
        ..connector
    }
}

fn render_commit_graph_lane(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
) -> gpui::Div {
    let has_incoming = row.incoming_lanes.contains(&lane);
    let has_outgoing = row.outgoing_lanes.contains(&lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let lane_selector = format!("commit-graph-lane-{row_index}-{lane}");

    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .debug_selector(move || lane_selector.clone())
        .child(render_commit_graph_vertical_segment(
            row_index,
            lane,
            row,
            previous_row,
            "top",
            has_incoming,
            lane_color,
        ))
        .child(render_commit_graph_middle_segment(
            row_index, lane, row, lane_color,
        ))
        .child(render_commit_graph_vertical_segment(
            row_index,
            lane,
            row,
            previous_row,
            "bottom",
            has_outgoing,
            lane_color,
        ))
}

fn commit_graph_lane_color(row: &graph::GraphRow, lane: usize) -> gpui::Rgba {
    const PALETTE: [u32; 6] = [0x60a5fa, 0xa3e635, 0xfbbf24, 0xf472b6, 0x2dd4bf, 0xc084fc];

    row.lane_colors
        .get(lane)
        .and_then(|color| *color)
        .map(|color| rgb(PALETTE[color % PALETTE.len()]))
        .unwrap_or_else(|| rgb(0x555555))
}

fn render_commit_graph_vertical_segment(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    position: &'static str,
    visible: bool,
    color: gpui::Rgba,
) -> gpui::Div {
    let selector = format!("commit-graph-vertical-{row_index}-{lane}-{position}");
    let (top, height) = commit_graph_vertical_segment_geometry(row, previous_row, lane, position);
    let segment = div()
        .absolute()
        .left(px(commit_graph_line_x()))
        .top(px(top))
        .w(px(COMMIT_GRAPH_LINE_WIDTH))
        .h(px(height))
        .when(visible, |segment| {
            segment.bg(color).debug_selector(move || selector.clone())
        });

    div()
        .relative()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_VERTICAL_HEIGHT))
        .child(segment)
}

fn commit_graph_vertical_segment_geometry(
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    lane: usize,
    position: &'static str,
) -> (f32, f32) {
    if position == "top" {
        if let Some(top) =
            commit_graph_top_vertical_inset_after_previous_row_branch_out(row, previous_row, lane)
        {
            return (top, COMMIT_GRAPH_VERTICAL_HEIGHT - top);
        }
    }

    if commit_graph_rounded_elbow_preserves_target_vertical(row, lane) {
        return (0., COMMIT_GRAPH_VERTICAL_HEIGHT);
    }

    let Some(tangent_y) = commit_graph_rounded_elbow_tangent_y(row, lane) else {
        return (0., COMMIT_GRAPH_VERTICAL_HEIGHT);
    };

    let middle_top = COMMIT_GRAPH_VERTICAL_HEIGHT;
    let middle_bottom = COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_middle_height();

    match position {
        "top" if tangent_y < middle_top => (
            0.,
            (tangent_y + commit_graph_line_width()).clamp(0., COMMIT_GRAPH_VERTICAL_HEIGHT),
        ),
        "bottom" if tangent_y > middle_bottom => {
            let top = (tangent_y - middle_bottom - commit_graph_line_width())
                .clamp(0., COMMIT_GRAPH_VERTICAL_HEIGHT);
            (top, COMMIT_GRAPH_VERTICAL_HEIGHT - top)
        }
        _ => (0., COMMIT_GRAPH_VERTICAL_HEIGHT),
    }
}

fn commit_graph_top_vertical_inset_after_previous_row_branch_out(
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    lane: usize,
) -> Option<f32> {
    if !row.incoming_lanes.contains(&lane) {
        return None;
    }

    let previous_row = previous_row?;
    let connector = commit_graph_target_connector_for_lane(previous_row, lane)?;
    if connector.kind != graph::GraphConnectorKind::BranchOut
        || !commit_graph_connector_uses_lower_branch_out_line(previous_row, connector)
        || commit_graph_rounded_elbow_turns_up(previous_row, lane, connector)
    {
        return None;
    }

    Some(commit_graph_bend_radius() - commit_graph_line_width())
}

fn commit_graph_rounded_elbow_preserves_target_vertical(
    row: &graph::GraphRow,
    lane: usize,
) -> bool {
    matches!(
        commit_graph_target_connector_for_lane(row, lane).map(|connector| connector.kind),
        Some(graph::GraphConnectorKind::BranchOut | graph::GraphConnectorKind::MergeIn)
    ) && row.incoming_lanes.contains(&lane)
        && row.outgoing_lanes.contains(&lane)
}

fn commit_graph_rounded_elbow_tangent_y(row: &graph::GraphRow, lane: usize) -> Option<f32> {
    let connector = commit_graph_target_connector_for_lane(row, lane)?;

    match connector.kind {
        graph::GraphConnectorKind::BranchOut | graph::GraphConnectorKind::MergeIn => {}
        graph::GraphConnectorKind::Straight => return None,
    }

    let middle_center_y = if commit_graph_uses_lower_branch_out_line(row, lane) {
        COMMIT_GRAPH_VERTICAL_HEIGHT
            + commit_graph_merge_in_commit_line_y_in_middle()
            + commit_graph_lower_connector_vertical_shift()
    } else {
        COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_middle_line_y() + commit_graph_line_width() / 2.
    };

    Some(
        if commit_graph_rounded_elbow_turns_up(row, lane, connector) {
            middle_center_y - commit_graph_bend_radius()
        } else {
            middle_center_y + commit_graph_bend_radius()
        },
    )
}

fn render_commit_graph_middle_segment(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    color: gpui::Rgba,
) -> gpui::Div {
    let is_commit = lane == row.lane;
    let has_connector = row.connector_lanes.contains(&lane);
    let has_middle_vertical =
        row.incoming_lanes.contains(&lane) || row.outgoing_lanes.contains(&lane);
    let connector_selector = format!("commit-graph-connector-{row_index}-{lane}");
    let middle_vertical_selector = format!("commit-graph-middle-vertical-{row_index}-{lane}");
    let dot_selector = format!("commit-graph-dot-{row_index}");
    let dot_top_gap_selector = format!("commit-graph-dot-top-gap-{row_index}-{lane}");
    let dot_bottom_gap_selector = format!("commit-graph-dot-bottom-gap-{row_index}-{lane}");
    let non_commit_connector_selector = connector_selector.clone();
    let commit_connector_selector = connector_selector;

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .when(has_connector && !is_commit, |middle| {
            middle.child(render_commit_graph_non_commit_connector(
                row_index,
                lane,
                row,
                non_commit_connector_selector.clone(),
            ))
        })
        .when(
            has_middle_vertical && !has_connector && !is_commit,
            |middle| {
                middle.child(
                    div()
                        .w(px(COMMIT_GRAPH_LINE_WIDTH))
                        .h(px(commit_graph_middle_height()))
                        .bg(color)
                        .debug_selector(move || middle_vertical_selector.clone()),
                )
            },
        )
        .when(is_commit, |middle| {
            let lane_span = row.connector_lanes.iter().copied();
            let min_lane = lane_span.clone().min().unwrap_or(lane);
            let max_lane = lane_span.max().unwrap_or(lane);
            let has_left_connector = has_connector && lane > min_lane;
            let right_target_connector = commit_graph_target_connector_from_side(row, lane, true);
            let has_right_connector =
                (has_connector && lane < max_lane) || right_target_connector.is_some();
            let left_connector = commit_graph_connector_on_side(row, lane, false);
            let right_connectors = commit_graph_connectors_on_side(row, lane, true);
            let right_connector =
                commit_graph_connector_on_side(row, lane, true).or(right_target_connector);
            let sibling_branch_extension = right_connector.filter(|connector| {
                commit_graph_connector_is_sibling_branch_extension(row, *connector)
            });
            let rounded_left_connector =
                commit_graph_commit_side_rounded_connector(row, lane, false);
            let rounded_right_connector =
                commit_graph_commit_side_rounded_connector(row, lane, true);
            let right_connector_is_rounded = right_connector
                .zip(rounded_right_connector)
                .is_some_and(|(right_connector, rounded_right_connector)| {
                    right_connector == rounded_right_connector
                });
            let left_connector_color = left_connector
                .map(|connector| commit_graph_connector_color(row, connector))
                .unwrap_or(color);
            let right_connector_color = right_connector
                .map(|connector| commit_graph_connector_color(row, connector))
                .unwrap_or(color);
            let right_connector_selector = right_target_connector
                .filter(|connector| connector.kind == graph::GraphConnectorKind::MergeIn)
                .map(|_| format!("commit-graph-merge-in-horizontal-{row_index}-{lane}"))
                .unwrap_or(commit_connector_selector);
            let commit_side_layer_order = commit_graph_commit_side_layer_order(
                rounded_left_connector,
                rounded_right_connector,
                sibling_branch_extension,
            );
            let draw_sibling_branch_extension_below_priority = commit_side_layer_order
                .first()
                .is_some_and(|layer| *layer == CommitGraphCommitSideLayer::SiblingBranchExtension);

            middle.child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(commit_graph_middle_height()))
                    .when(has_middle_vertical, |commit| {
                        commit
                            .when(row.incoming_lanes.contains(&lane), |commit| {
                                commit.child(
                                    div()
                                        .absolute()
                                        .left(px(commit_graph_line_x()))
                                        .top_0()
                                        .w(px(COMMIT_GRAPH_LINE_WIDTH))
                                        .h(px(commit_graph_dot_gap_height()))
                                        .bg(color)
                                        .debug_selector(move || dot_top_gap_selector.clone()),
                                )
                            })
                            .when(row.outgoing_lanes.contains(&lane), |commit| {
                                commit.child(
                                    div()
                                        .absolute()
                                        .left(px(commit_graph_line_x()))
                                        .top(px(commit_graph_dot_bottom_gap_y()))
                                        .w(px(COMMIT_GRAPH_LINE_WIDTH))
                                        .h(px(commit_graph_dot_gap_height()))
                                        .bg(color)
                                        .debug_selector(move || dot_bottom_gap_selector.clone()),
                                )
                            })
                    })
                    .child(
                        div()
                            .w(px(commit_graph_dot_side_line_width()))
                            .h(px(COMMIT_GRAPH_LINE_WIDTH))
                            .when(
                                has_left_connector && rounded_left_connector.is_none(),
                                |line| line.bg(left_connector_color),
                            ),
                    )
                    .when(
                        draw_sibling_branch_extension_below_priority,
                        |commit| {
                            let connector = sibling_branch_extension
                                .expect("sibling branch extension layer requires connector");
                            commit.child(render_commit_graph_shared_branch_horizontal(
                                row_index,
                                lane,
                                commit_graph_connector_color(row, connector),
                            ))
                        },
                    )
                    .when_some(rounded_left_connector, |commit, connector| {
                        let selector = match connector.kind {
                            graph::GraphConnectorKind::MergeIn => {
                                format!(
                                    "commit-graph-rounded-merge-in-commit-elbow-{row_index}-{lane}"
                                )
                            }
                            graph::GraphConnectorKind::BranchOut
                            | graph::GraphConnectorKind::Straight => {
                                format!("commit-graph-rounded-commit-elbow-{row_index}-{lane}")
                            }
                        };

                        commit.child(render_commit_graph_rounded_merge_in_commit_bend(
                            selector,
                            left_connector_color,
                        ))
                    })
                    .when_some(rounded_right_connector, |commit, connector| {
                        let selector = match connector.kind {
                            graph::GraphConnectorKind::BranchOut if right_connectors.len() > 1 => {
                                format!(
                                    "commit-graph-rounded-merge-target-commit-elbow-{row_index}-{lane}-{}",
                                    connector.to_lane
                                )
                            }
                            graph::GraphConnectorKind::BranchOut => {
                                format!(
                                    "commit-graph-rounded-merge-target-commit-elbow-{row_index}-{lane}"
                                )
                            }
                            graph::GraphConnectorKind::MergeIn
                            | graph::GraphConnectorKind::Straight => {
                                format!("commit-graph-rounded-commit-elbow-{row_index}-{lane}")
                            }
                        };

                        commit.child(render_commit_graph_rounded_merge_target_commit_bend(
                            selector,
                            commit_graph_connector_color(row, connector),
                        ))
                    })
                    .child(
                        div()
                            .w(px(COMMIT_GRAPH_DOT_SIZE))
                            .h(px(COMMIT_GRAPH_DOT_SIZE))
                            .rounded_full()
                            .bg(color)
                            .debug_selector(move || dot_selector.clone()),
                    )
                    .child(
                        div()
                            .w(px(commit_graph_dot_side_line_width()))
                            .h(px(COMMIT_GRAPH_LINE_WIDTH))
                            .when(
                                has_right_connector
                                    && !right_connector_is_rounded
                                    && sibling_branch_extension.is_none(),
                                |line| {
                                    line.bg(right_connector_color)
                                        .debug_selector(move || right_connector_selector.clone())
                                },
                            ),
                    ),
            )
        })
}

fn commit_graph_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    commit_graph_target_connector_for_lane(row, lane)
        .or_else(|| commit_graph_source_connector_for_lane(row, lane))
        .or_else(|| commit_graph_spanning_connector_for_lane(row, lane))
}

fn commit_graph_target_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    row.connectors
        .iter()
        .copied()
        .find(|connector| connector.to_lane == lane)
}

fn commit_graph_target_connector_from_side(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    row.connectors.iter().copied().find(|connector| {
        connector.to_lane == lane
            && ((right && connector.from_lane > lane) || (!right && connector.from_lane < lane))
    })
}

fn commit_graph_source_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    row.connectors.iter().copied().find(|connector| {
        connector.from_lane == lane && connector.kind != graph::GraphConnectorKind::Straight
    })
}

fn commit_graph_spanning_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    row.connectors.iter().copied().find(|connector| {
        if connector.kind == graph::GraphConnectorKind::Straight {
            return false;
        }

        let min_lane = connector.from_lane.min(connector.to_lane);
        let max_lane = connector.from_lane.max(connector.to_lane);
        lane > min_lane && lane < max_lane
    })
}

fn commit_graph_spanning_connector_requires_center_fill(
    row: &graph::GraphRow,
    lane: usize,
) -> bool {
    commit_graph_target_connector_for_lane(row, lane).is_none()
        && commit_graph_spanning_connector_for_lane(row, lane).is_some()
        && !row.incoming_lanes.contains(&lane)
        && !row.outgoing_lanes.contains(&lane)
}

fn commit_graph_connector_on_side(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    commit_graph_connectors_on_side(row, lane, right)
        .into_iter()
        .next()
}

fn commit_graph_connectors_on_side(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Vec<graph::GraphConnector> {
    let mut connectors = row
        .connectors
        .iter()
        .copied()
        .filter(|connector| {
            (right && connector.to_lane > lane) || (!right && connector.to_lane < lane)
        })
        .collect::<Vec<_>>();
    connectors.sort_by_key(|connector| connector.to_lane.abs_diff(lane));
    connectors
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitGraphCommitSideLayer {
    SiblingBranchExtension,
    RoundedLeftConnector,
    RoundedRightConnector,
}

fn commit_graph_commit_side_layer_order(
    rounded_left_connector: Option<graph::GraphConnector>,
    rounded_right_connector: Option<graph::GraphConnector>,
    sibling_branch_extension: Option<graph::GraphConnector>,
) -> Vec<CommitGraphCommitSideLayer> {
    let mut layers = Vec::new();

    if sibling_branch_extension.is_some() {
        layers.push(CommitGraphCommitSideLayer::SiblingBranchExtension);
    }
    if rounded_left_connector.is_some() {
        layers.push(CommitGraphCommitSideLayer::RoundedLeftConnector);
    }
    if rounded_right_connector.is_some() {
        layers.push(CommitGraphCommitSideLayer::RoundedRightConnector);
    }

    layers
}

fn commit_graph_commit_side_rounded_connector(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    if right {
        return commit_graph_connectors_on_side(row, lane, true)
            .into_iter()
            .filter(|connector| {
                !commit_graph_connector_is_sibling_branch_extension(row, *connector)
            })
            .find(|connector| commit_graph_connector_uses_lower_branch_out_line(row, *connector));
    }

    if row.outgoing_lanes.contains(&lane) || !row.incoming_lanes.contains(&lane) {
        return None;
    }

    row.connectors
        .iter()
        .copied()
        .filter(|connector| {
            connector.to_lane < lane && connector.kind == graph::GraphConnectorKind::MergeIn
        })
        .min_by_key(|connector| connector.to_lane.abs_diff(lane))
}

fn commit_graph_connector_is_sibling_branch_extension(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> bool {
    connector.kind == graph::GraphConnectorKind::BranchOut
        && connector.from_lane == row.lane
        && !row.parent_lanes.contains(&connector.to_lane)
}

fn commit_graph_rounded_elbow_turns_up(
    row: &graph::GraphRow,
    lane: usize,
    connector: graph::GraphConnector,
) -> bool {
    row.incoming_lanes.contains(&lane)
        && (!row.outgoing_lanes.contains(&lane)
            || (connector.kind == graph::GraphConnectorKind::BranchOut
                && commit_graph_connector_is_sibling_branch_extension(row, connector)))
}

fn commit_graph_uses_lower_branch_out_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    commit_graph_connector_uses_lower_branch_out_line(row, connector)
}

fn commit_graph_connector_uses_lower_branch_out_line(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> bool {
    connector.kind == graph::GraphConnectorKind::BranchOut
        && (row.outgoing_lanes.contains(&connector.to_lane)
            || commit_graph_connector_is_sibling_branch_extension(row, connector))
}

fn commit_graph_uses_lower_merge_in_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    connector.kind == graph::GraphConnectorKind::MergeIn
        && row.incoming_lanes.contains(&connector.to_lane)
        && row.outgoing_lanes.contains(&connector.to_lane)
        && row.incoming_lanes.contains(&connector.from_lane)
        && !row.outgoing_lanes.contains(&connector.from_lane)
}

fn commit_graph_connector_color(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> gpui::Rgba {
    commit_graph_lane_color(row, commit_graph_connector_color_lane(connector))
}

fn commit_graph_connector_color_lane(connector: graph::GraphConnector) -> usize {
    match connector.kind {
        graph::GraphConnectorKind::BranchOut => connector.to_lane,
        graph::GraphConnectorKind::MergeIn => connector.from_lane,
        graph::GraphConnectorKind::Straight => connector.to_lane,
    }
}

fn render_commit_graph_rounded_elbow(
    selector: String,
    kind: graph::GraphConnectorKind,
    turns_up: bool,
    horizontal_top_y: f32,
    connector_color: gpui::Rgba,
) -> gpui::Div {
    let overlay_height = commit_graph_bend_overlay_height()
        + (horizontal_top_y - commit_graph_lower_merge_in_horizontal_top_in_middle()).max(0.);

    div()
        .absolute()
        .left(px(commit_graph_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_bend_overlay_width()))
        .h(px(overlay_height))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let lane_offset_x = commit_graph_bend_overlay_lane_offset_x();
                    let center_x = bounds.origin.x
                        + px(lane_offset_x + commit_graph_line_x() + line_width / 2.);
                    let center_y = bounds.origin.y
                        + px(COMMIT_GRAPH_VERTICAL_HEIGHT + horizontal_top_y + line_width / 2.);
                    let left_x = bounds.origin.x;
                    let right_x = bounds.origin.x + px(commit_graph_bend_overlay_width());
                    let radius = px(commit_graph_bend_radius());
                    let control = px(commit_graph_bend_radius() * COMMIT_GRAPH_BEND_CUBIC_CONTROL);

                    let mut connector = PathBuilder::stroke(px(line_width));
                    match kind {
                        graph::GraphConnectorKind::BranchOut => {
                            connector.move_to(point(left_x, center_y));
                            connector.line_to(point(center_x - radius, center_y));
                            if turns_up {
                                connector.cubic_bezier_to(
                                    point(center_x, center_y - radius),
                                    point(center_x - radius + control, center_y),
                                    point(center_x, center_y - radius + control),
                                );
                            } else {
                                connector.cubic_bezier_to(
                                    point(center_x, center_y + radius),
                                    point(center_x - radius + control, center_y),
                                    point(center_x, center_y + radius - control),
                                );
                            }
                        }
                        graph::GraphConnectorKind::MergeIn => {
                            if turns_up {
                                connector.move_to(point(center_x, center_y - radius));
                                connector.cubic_bezier_to(
                                    point(center_x + radius, center_y),
                                    point(center_x, center_y - radius + control),
                                    point(center_x + radius - control, center_y),
                                );
                            } else {
                                connector.move_to(point(center_x, center_y + radius));
                                connector.cubic_bezier_to(
                                    point(center_x + radius, center_y),
                                    point(center_x, center_y + radius - control),
                                    point(center_x + radius - control, center_y),
                                );
                            }
                            connector.line_to(point(right_x, center_y));
                        }
                        graph::GraphConnectorKind::Straight => {}
                    }

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top_0()
            .w(px(commit_graph_bend_overlay_width()))
            .h(px(overlay_height)),
        )
}

fn render_commit_graph_rounded_branch_off_source_bend(
    selector: String,
    connector_color: gpui::Rgba,
    spans_occupied_lanes: bool,
    vertical_offset: f32,
) -> gpui::Div {
    div()
        .absolute()
        .left(px(commit_graph_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_bend_overlay_width()))
        .h(px(commit_graph_bend_overlay_height() + vertical_offset))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let bend = commit_graph_branch_off_source_bend_geometry(spans_occupied_lanes);

                    let mut connector = PathBuilder::stroke(px(line_width));
                    connector.move_to(point(
                        bounds.origin.x + px(bend.curve.start.x),
                        bounds.origin.y + px(bend.curve.start.y),
                    ));
                    connector.cubic_bezier_to(
                        point(
                            bounds.origin.x + px(bend.curve.end.x),
                            bounds.origin.y + px(bend.curve.end.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.curve.first_control.x),
                            bounds.origin.y + px(bend.curve.first_control.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.curve.second_control.x),
                            bounds.origin.y + px(bend.curve.second_control.y),
                        ),
                    );
                    if let Some(horizontal_end) = bend.horizontal_end {
                        connector.line_to(point(
                            bounds.origin.x + px(horizontal_end.x),
                            bounds.origin.y + px(horizontal_end.y),
                        ));
                    }

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top(px(vertical_offset))
            .w(px(commit_graph_bend_overlay_width()))
            .h(px(commit_graph_bend_overlay_height())),
        )
}

fn render_commit_graph_shared_branch_horizontal(
    row_index: usize,
    lane: usize,
    color: gpui::Rgba,
) -> gpui::Div {
    let horizontal_selector = format!("commit-graph-shared-branch-horizontal-{row_index}-{lane}");
    let horizontal_left =
        commit_graph_commit_bend_overlay_x() + commit_graph_merge_in_commit_bend_geometry().start.x;

    div()
        .absolute()
        .left(px(horizontal_left))
        .top(px(
            commit_graph_shifted_lower_merge_in_horizontal_top_in_middle(),
        ))
        .w(px(COMMIT_GRAPH_LANE_WIDTH - horizontal_left))
        .h(px(COMMIT_GRAPH_LINE_WIDTH))
        .bg(color)
        .debug_selector(move || horizontal_selector.clone())
}

fn render_commit_graph_rounded_merge_in_commit_bend(
    selector: String,
    connector_color: gpui::Rgba,
) -> gpui::Div {
    let dot_connector_selector = format!("{selector}-dot-connector");
    let vertical_offset = commit_graph_lower_connector_vertical_shift();

    div()
        .absolute()
        .left(px(commit_graph_commit_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_commit_bend_overlay_width()))
        .h(px(commit_graph_shifted_bend_overlay_height()))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let bend = commit_graph_merge_in_commit_bend_geometry();
                    let horizontal_start_x = -commit_graph_commit_bend_overlay_x();

                    let mut connector = PathBuilder::stroke(px(line_width));
                    connector.move_to(point(
                        bounds.origin.x + px(horizontal_start_x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.line_to(point(
                        bounds.origin.x + px(bend.start.x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.cubic_bezier_to(
                        point(
                            bounds.origin.x + px(bend.end.x),
                            bounds.origin.y + px(bend.end.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.first_control.x),
                            bounds.origin.y + px(bend.first_control.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.second_control.x),
                            bounds.origin.y + px(bend.second_control.y),
                        ),
                    );

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top(px(vertical_offset))
            .w(px(commit_graph_commit_bend_overlay_width()))
            .h(px(commit_graph_bend_overlay_height())),
        )
        .child({
            let connector = commit_graph_shifted_merge_in_commit_dot_connector_geometry();
            div()
                .absolute()
                .left(px(connector.x))
                .top(px(connector.y))
                .w(px(connector.width))
                .h(px(connector.height))
                .bg(connector_color)
                .debug_selector(move || dot_connector_selector.clone())
        })
}

fn render_commit_graph_rounded_merge_target_commit_bend(
    selector: String,
    connector_color: gpui::Rgba,
) -> gpui::Div {
    let vertical_offset = commit_graph_lower_connector_vertical_shift();

    div()
        .absolute()
        .left(px(commit_graph_merge_target_commit_bend_overlay_x()))
        .top(px(commit_graph_bend_overlay_top()))
        .w(px(commit_graph_merge_target_commit_bend_overlay_width()))
        .h(px(commit_graph_shifted_bend_overlay_height()))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let line_width = commit_graph_line_width();
                    let bend = commit_graph_merge_target_commit_bend_geometry();
                    let horizontal_start_x = commit_graph_merge_target_commit_bend_overlay_width();

                    let mut connector = PathBuilder::stroke(px(line_width));
                    connector.move_to(point(
                        bounds.origin.x + px(horizontal_start_x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.line_to(point(
                        bounds.origin.x + px(bend.start.x),
                        bounds.origin.y + px(bend.start.y),
                    ));
                    connector.cubic_bezier_to(
                        point(
                            bounds.origin.x + px(bend.end.x),
                            bounds.origin.y + px(bend.end.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.first_control.x),
                            bounds.origin.y + px(bend.first_control.y),
                        ),
                        point(
                            bounds.origin.x + px(bend.second_control.x),
                            bounds.origin.y + px(bend.second_control.y),
                        ),
                    );

                    if let Ok(path) = connector.build() {
                        window.paint_path(path, connector_color);
                    }
                },
            )
            .absolute()
            .left_0()
            .top(px(vertical_offset))
            .w(px(commit_graph_merge_target_commit_bend_overlay_width()))
            .h(px(commit_graph_bend_overlay_height())),
        )
}

fn render_commit_graph_non_commit_connector(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    connector_selector: String,
) -> gpui::Div {
    let target_connector = commit_graph_target_connector_for_lane(row, lane);
    let source_connector = commit_graph_source_connector_for_lane(row, lane);
    let connector = commit_graph_connector_for_lane(row, lane);
    let has_incoming = row.incoming_lanes.contains(&lane);
    let has_outgoing = row.outgoing_lanes.contains(&lane);
    let preserve_target_vertical = commit_graph_rounded_elbow_preserves_target_vertical(row, lane);
    let uses_lower_merge_in_line = commit_graph_uses_lower_merge_in_line(row, lane);
    let uses_lower_branch_out_line = commit_graph_uses_lower_branch_out_line(row, lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let color = connector
        .map(|connector| commit_graph_connector_color(row, connector))
        .unwrap_or(lane_color);
    let (left_visible, right_visible) = match (target_connector, source_connector) {
        (Some(connector), _) => match connector.kind {
            graph::GraphConnectorKind::BranchOut => (true, false),
            graph::GraphConnectorKind::MergeIn => (false, true),
            graph::GraphConnectorKind::Straight => (true, true),
        },
        (None, Some(connector))
            if connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane =>
        {
            (true, false)
        }
        _ => (true, true),
    };
    let kind_selector = target_connector.and_then(|connector| match connector.kind {
        graph::GraphConnectorKind::BranchOut => {
            Some(format!("commit-graph-branch-out-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::MergeIn => {
            Some(format!("commit-graph-merge-in-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::Straight => None,
    });
    let elbow_selector = target_connector.and_then(|connector| match connector.kind {
        graph::GraphConnectorKind::BranchOut => {
            Some(format!("commit-graph-branch-out-elbow-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::MergeIn => {
            Some(format!("commit-graph-merge-in-elbow-{row_index}-{lane}"))
        }
        graph::GraphConnectorKind::Straight => None,
    });
    let rounded_elbow = target_connector.and_then(|connector| match connector.kind {
        graph::GraphConnectorKind::BranchOut => Some((
            format!("commit-graph-rounded-branch-out-elbow-{row_index}-{lane}"),
            connector.kind,
        )),
        graph::GraphConnectorKind::MergeIn if !uses_lower_merge_in_line => Some((
            format!("commit-graph-rounded-merge-in-elbow-{row_index}-{lane}"),
            connector.kind,
        )),
        graph::GraphConnectorKind::MergeIn => None,
        graph::GraphConnectorKind::Straight => None,
    });
    let lower_merge_in_source_bend = target_connector.and_then(|connector| {
        (connector.kind == graph::GraphConnectorKind::MergeIn && uses_lower_merge_in_line).then(
            || {
                (
                    format!("commit-graph-rounded-branch-off-source-elbow-{row_index}-{lane}"),
                    connector.from_lane > lane + 1,
                )
            },
        )
    });
    let source_merge_in_bend = source_connector.and_then(|connector| {
        (connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane).then(
            || {
                (
                    format!("commit-graph-rounded-merge-in-source-elbow-{row_index}-{lane}"),
                    graph::GraphConnectorKind::BranchOut,
                )
            },
        )
    });
    let spanning_through_target_connector = target_connector
        .and_then(|_| commit_graph_spanning_connector_for_lane(row, lane))
        .filter(|connector| commit_graph_connector_uses_lower_branch_out_line(row, *connector));
    let left_horizontal_is_rounded = rounded_elbow
        .as_ref()
        .is_some_and(|(_, kind)| *kind == graph::GraphConnectorKind::BranchOut)
        || source_merge_in_bend.is_some();
    let right_horizontal_is_rounded = rounded_elbow
        .as_ref()
        .is_some_and(|(_, kind)| *kind == graph::GraphConnectorKind::MergeIn)
        || lower_merge_in_source_bend.is_some();
    let has_rounded_elbow = left_horizontal_is_rounded || right_horizontal_is_rounded;
    let horizontal_top_y = if uses_lower_merge_in_line || uses_lower_branch_out_line {
        commit_graph_shifted_lower_merge_in_horizontal_top_in_middle()
    } else {
        commit_graph_middle_line_y()
    };
    let elbow_top = if has_incoming { 0. } else { horizontal_top_y };
    let elbow_bottom = if uses_lower_branch_out_line && has_outgoing {
        horizontal_top_y + commit_graph_line_width()
    } else if has_outgoing {
        commit_graph_middle_height()
    } else {
        commit_graph_middle_line_bottom_y()
    };
    let elbow_height = elbow_bottom - elbow_top;
    let middle_vertical_selector = format!("commit-graph-middle-vertical-{row_index}-{lane}");
    let has_middle_vertical = has_incoming || has_outgoing;
    let center_fill_selector =
        format!("commit-graph-spanning-horizontal-center-{row_index}-{lane}");
    let spanning_left_selector =
        format!("commit-graph-spanning-horizontal-left-{row_index}-{lane}");
    let spanning_right_selector =
        format!("commit-graph-spanning-horizontal-right-{row_index}-{lane}");
    let spanning_through_target_selector =
        format!("commit-graph-spanning-horizontal-through-target-{row_index}-{lane}");
    let source_merge_in_right_selector = source_connector.and_then(|connector| {
        (connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane)
            .then(|| format!("commit-graph-merge-in-source-horizontal-right-{row_index}-{lane}"))
    });
    let incoming_vertical_bridge = target_connector.and_then(|connector| {
        if connector.kind != graph::GraphConnectorKind::BranchOut
            || !commit_graph_rounded_elbow_turns_up(row, lane, connector)
        {
            return None;
        }

        let tangent_y = commit_graph_rounded_elbow_tangent_y(row, lane)?;
        (tangent_y > COMMIT_GRAPH_VERTICAL_HEIGHT).then(|| {
            (
                format!("commit-graph-rounded-branch-out-vertical-bridge-{row_index}-{lane}"),
                tangent_y - COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_line_width(),
            )
        })
    });
    let fill_spanning_center = commit_graph_spanning_connector_requires_center_fill(row, lane);

    let mut connector_shape = div()
        .relative()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(horizontal_top_y))
                .w(px(commit_graph_line_x()))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .when(left_visible, |line| {
                    line.when(!left_horizontal_is_rounded, |line| line.bg(color))
                        .when_some(
                            target_connector.and_then(|connector| {
                                (connector.kind == graph::GraphConnectorKind::BranchOut).then(
                                    || {
                                        format!(
                                            "commit-graph-branch-out-horizontal-{row_index}-{lane}"
                                        )
                                    },
                                )
                            }),
                            |line, selector| line.debug_selector(move || selector.clone()),
                        )
                        .when(
                            target_connector.is_none()
                                && commit_graph_spanning_connector_for_lane(row, lane).is_some(),
                            |line| line.debug_selector(move || spanning_left_selector.clone()),
                        )
                }),
        )
        .child(
            div()
                .absolute()
                .left(px(commit_graph_right_line_x()))
                .top(px(horizontal_top_y))
                .w(px(commit_graph_right_line_width()))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .when(right_visible, |line| {
                    line.when(!right_horizontal_is_rounded, |line| line.bg(color))
                        .when_some(
                            target_connector.and_then(|connector| {
                                (connector.kind == graph::GraphConnectorKind::MergeIn).then(|| {
                                    format!("commit-graph-merge-in-horizontal-{row_index}-{lane}")
                                })
                            }),
                            |line, selector| line.debug_selector(move || selector.clone()),
                        )
                        .when(
                            target_connector.is_none()
                                && commit_graph_spanning_connector_for_lane(row, lane).is_some(),
                            |line| line.debug_selector(move || spanning_right_selector.clone()),
                        )
                        .when_some(source_merge_in_right_selector, |line, selector| {
                            line.debug_selector(move || selector.clone())
                        })
                }),
        );

    if fill_spanning_center {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(horizontal_top_y))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .bg(color)
                .debug_selector(move || center_fill_selector.clone()),
        );
    }

    if let Some(spanning_connector) = spanning_through_target_connector {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(
                    commit_graph_shifted_lower_merge_in_horizontal_top_in_middle(),
                ))
                .w(px(COMMIT_GRAPH_LANE_WIDTH))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .bg(commit_graph_connector_color(row, spanning_connector))
                .debug_selector(move || spanning_through_target_selector.clone()),
        );
    }

    if let Some(kind_selector) = kind_selector {
        connector_shape = connector_shape.debug_selector(move || kind_selector.clone());
    }

    if let Some(elbow_selector) = elbow_selector {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(elbow_top))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(elbow_height))
                .when(!has_rounded_elbow, |elbow| {
                    elbow.bg(if has_middle_vertical {
                        lane_color
                    } else {
                        color
                    })
                })
                .debug_selector(move || elbow_selector.clone()),
        );
    }

    if has_middle_vertical {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(elbow_top))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(elbow_height))
                .when(!has_rounded_elbow || preserve_target_vertical, |vertical| {
                    vertical.bg(lane_color)
                })
                .debug_selector(move || middle_vertical_selector.clone()),
        );
    }

    if let Some((incoming_vertical_bridge_selector, incoming_vertical_bridge_height)) =
        incoming_vertical_bridge
    {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top_0()
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(incoming_vertical_bridge_height))
                .bg(lane_color)
                .debug_selector(move || incoming_vertical_bridge_selector.clone()),
        );
    }

    if let Some((source_bend_selector, source_bend_spans_occupied_lanes)) =
        lower_merge_in_source_bend
    {
        connector_shape =
            connector_shape.child(render_commit_graph_rounded_branch_off_source_bend(
                source_bend_selector,
                color,
                source_bend_spans_occupied_lanes,
                commit_graph_lower_connector_vertical_shift(),
            ));
    }

    if let Some((source_bend_selector, source_bend_kind)) = source_merge_in_bend {
        connector_shape = connector_shape.child(render_commit_graph_rounded_elbow(
            source_bend_selector,
            source_bend_kind,
            has_incoming && !has_outgoing,
            horizontal_top_y,
            color,
        ));
    }

    if let Some((rounded_elbow_selector, rounded_elbow_kind)) = rounded_elbow {
        connector_shape = connector_shape.child(render_commit_graph_rounded_elbow(
            rounded_elbow_selector,
            rounded_elbow_kind,
            target_connector
                .map(|connector| commit_graph_rounded_elbow_turns_up(row, lane, connector))
                .unwrap_or(false),
            horizontal_top_y,
            color,
        ));
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .debug_selector(move || connector_selector.clone())
        .child(connector_shape)
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

fn render_file_content(content: repo::FileContentBody, scroll: &FileDiffScroll) -> AnyElement {
    match content {
        repo::FileContentBody::Text(text) => {
            let cells = read_only_file_cells(&text);

            render_file_diff_side(
                "Contents",
                "file-read-only-content",
                cells,
                scroll.handle_for(repo::DiffSide::New),
            )
            .into_any_element()
        }
        repo::FileContentBody::Binary => {
            render_file_diff_content(repo::FileDiffContent::Binary, scroll)
        }
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

fn read_only_file_cells(text: &str) -> Vec<DiffLineCell> {
    content_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| DiffLineCell {
            line_number: Some(index + 1),
            text: line,
            status: DiffLineStatus::Unchanged,
        })
        .collect()
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
        repo::ChangeKind::Added => rgb(0xb8f77a),
        repo::ChangeKind::Modified => rgb(0x7da4ff),
        repo::ChangeKind::Deleted => rgb(0xff5f78),
        repo::ChangeKind::Renamed => rgb(0xf3d36b),
    }
}

fn change_kind_text(kind: repo::ChangeKind) -> gpui::Rgba {
    match kind {
        repo::ChangeKind::Added => rgb(0xb8f77a),
        repo::ChangeKind::Modified => rgb(0x7da4ff),
        repo::ChangeKind::Deleted => rgb(0xff5f78),
        repo::ChangeKind::Renamed => rgb(0xf3d36b),
    }
}

fn render_file_tree_folder_icon(
    path_fragment: &str,
    collapsed: bool,
    color: gpui::Rgba,
) -> gpui::Div {
    let state = if collapsed { "closed" } else { "open" };
    let selector = format!("file-tree-folder-icon-{state}-{path_fragment}");

    let icon = div()
        .relative()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .flex_none()
        .debug_selector(move || selector.clone());

    if collapsed {
        icon.child(render_file_tree_icon_part(
            format!("file-tree-folder-icon-closed-body-{path_fragment}"),
            1.,
            6.,
            14.,
            8.,
            color,
        ))
        .child(render_file_tree_icon_part(
            format!("file-tree-folder-icon-closed-tab-{path_fragment}"),
            2.,
            3.,
            7.,
            4.,
            color,
        ))
    } else {
        icon.child(render_file_tree_open_folder_outline(
            format!("file-tree-folder-icon-open-outline-{path_fragment}"),
            color,
        ))
    }
}

fn render_file_tree_icon_part(
    selector: String,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    color: gpui::Rgba,
) -> gpui::Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .rounded(px(1.5))
        .border_1()
        .border_color(color)
        .debug_selector(move || selector.clone())
}

fn render_file_tree_open_folder_outline(selector: String, color: gpui::Rgba) -> gpui::Div {
    div()
        .absolute()
        .left_0()
        .top_0()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let point_at = |x, y| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                    let mut outline = PathBuilder::stroke(px(1.35));

                    outline.move_to(point_at(2.5, 8.));
                    outline.line_to(point_at(2.5, 5.));
                    outline.line_to(point_at(6.5, 5.));
                    outline.line_to(point_at(8., 7.));
                    outline.line_to(point_at(13., 7.));
                    outline.line_to(point_at(13.8, 9.));

                    outline.move_to(point_at(1.5, 9.));
                    outline.line_to(point_at(14.5, 9.));
                    outline.line_to(point_at(13., 14.));
                    outline.line_to(point_at(2.5, 14.));
                    outline.line_to(point_at(1.5, 9.));

                    if let Ok(path) = outline.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .w_full()
            .h_full(),
        )
}

fn render_file_tree_file_icon(selector: String, color: gpui::Rgba) -> gpui::Div {
    let folded_corner_selector = format!("{selector}-folded-corner");

    div()
        .relative()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .flex_none()
        .debug_selector(move || selector.clone())
        .child(
            div()
                .absolute()
                .left(px(3.))
                .top(px(2.))
                .w(px(10.))
                .h(px(12.))
                .rounded(px(1.5))
                .border_1()
                .border_color(color),
        )
        .child(
            div()
                .absolute()
                .left(px(9.))
                .top(px(2.))
                .w(px(4.))
                .h(px(4.))
                .border_1()
                .border_color(color)
                .debug_selector(move || folded_corner_selector.clone()),
        )
}

fn render_file_tree_indent_guides(depth: usize, path_fragment: &str) -> gpui::Div {
    let guides = (0..depth)
        .map(|level| {
            let selector = format!("file-tree-indent-guide-{path_fragment}-{level}");
            div()
                .absolute()
                .left(px(
                    (level + 1) as f32 * FILE_TREE_INDENT_WIDTH - FILE_TREE_INDENT_GUIDE_WIDTH / 2.
                ))
                .top_0()
                .w(px(FILE_TREE_INDENT_GUIDE_WIDTH))
                .h(px(FILE_TREE_ROW_HEIGHT))
                .bg(rgb(0x2b383f))
                .debug_selector(move || selector.clone())
        })
        .collect::<Vec<_>>();

    div()
        .relative()
        .flex_none()
        .w(px(file_tree_indent_guides_width(depth)))
        .min_h(px(FILE_TREE_ROW_HEIGHT))
        .children(guides)
}

fn file_tree_indent_guides_width(depth: usize) -> f32 {
    depth as f32 * FILE_TREE_INDENT_WIDTH
        + if depth > 0 {
            FILE_TREE_GUIDE_TO_ITEM_GAP
        } else {
            0.
        }
}

fn render_file_tree_file_name(
    selector: String,
    display_name: &str,
    deleted: bool,
    deleted_strike_selector: String,
) -> gpui::Div {
    div()
        .relative()
        .flex()
        .items_center()
        .text_color(rgb(0xe6eef0))
        .text_size(px(FILE_TREE_TEXT_SIZE))
        .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
        .font_family(FILE_TREE_FONT_FAMILY)
        .whitespace_nowrap()
        .debug_selector(move || selector.clone())
        .child(display_name.to_string())
        .when(deleted, |label| {
            label.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(10.))
                    .h(px(1.))
                    .bg(rgb(0xe6eef0))
                    .debug_selector(move || deleted_strike_selector.clone()),
            )
        })
}

fn render_change_status_icon(
    kind: repo::ChangeKind,
    selector: String,
    icon_selector: String,
) -> gpui::Div {
    let color = change_kind_text(kind);
    let glyph: AnyElement = match kind {
        repo::ChangeKind::Added => Icon::new(LucideIcon::SquarePlus)
            .text_color(color)
            .size(px(FILE_TREE_STATUS_ICON_SIZE))
            .into_any_element(),
        repo::ChangeKind::Deleted => Icon::new(LucideIcon::SquareMinus)
            .text_color(color)
            .size(px(FILE_TREE_STATUS_ICON_SIZE))
            .into_any_element(),
        repo::ChangeKind::Modified => Icon::new(LucideIcon::SquareDot)
            .text_color(color)
            .size(px(FILE_TREE_STATUS_ICON_SIZE))
            .into_any_element(),
        repo::ChangeKind::Renamed => div()
            .text_size(px(FILE_TREE_BADGE_TEXT_SIZE))
            .font_family(FILE_TREE_FONT_FAMILY)
            .text_color(color)
            .child("R")
            .into_any_element(),
    };

    let mut marker = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(FILE_TREE_STATUS_ICON_SIZE))
        .h(px(FILE_TREE_STATUS_ICON_SIZE))
        .flex_none()
        .debug_selector(move || icon_selector.clone());
    // The Lucide square-* glyphs draw their own outline, so only the rename
    // marker still needs a hand-drawn bordered box around its letter.
    if matches!(kind, repo::ChangeKind::Renamed) {
        marker = marker
            .border_1()
            .rounded(px(2.))
            .border_color(change_kind_border(kind));
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .flex_none()
        .debug_selector(move || selector.clone())
        .child(marker.child(glyph))
}

fn render_file_diff_stat(selector: String, stats: repo::LineStats) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap_2()
        .w(px(68.))
        .flex_none()
        .font_family(FILE_TREE_FONT_FAMILY)
        .text_size(px(FILE_TREE_DIFF_STAT_TEXT_SIZE))
        .debug_selector(move || selector.clone())
        .child(
            div()
                .text_color(rgb(0xb8f77a))
                .child(format!("+ {}", stats.added)),
        )
        .child(
            div()
                .text_color(rgb(0xff5f78))
                .child(format!("- {}", stats.removed)),
        )
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.mode {
            Mode::NoRepo => self.render_no_repo(cx).into_any_element(),
            Mode::RepoOpen { repo } => self.render_repo_open(repo, cx),
        };

        div()
            .relative()
            .flex()
            .flex_col()
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
            .on_action(cx.listener(|app, _: &QuitApplication, _window, cx| {
                app.quit_application(cx);
            }))
            .child(self.render_title_bar(cx))
            .child(div().flex().flex_1().min_h(px(0.)).w_full().child(body))
            .children(self.render_context_popover(cx))
            .children(self.render_repo_switcher(cx))
            .child(self.notifications.clone())
    }
}

fn insert_file_tree_entry(root: &mut FileTreeBranch, entry: FileListEntry) {
    let path = entry.path().to_string();
    let mut parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some(file_name) = parts.pop() else {
        return;
    };

    let mut branch = root;
    for folder in parts {
        branch = branch.folders.entry(folder.to_string()).or_default();
    }

    branch.files.push(FileTreeLeaf {
        name: file_name.to_string(),
        entry,
    });
    branch.files.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.entry.path().cmp(right.entry.path()))
    });
}

/// Collect every folder path that is an ancestor of a changed file. These are
/// the folders that lead to the diff, so they stay expanded by default even in
/// "all files" mode.
fn changed_file_ancestor_paths(entries: &[FileListEntry]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();

    for entry in entries {
        if !matches!(entry, FileListEntry::Changed(_)) {
            continue;
        }

        let mut parts = entry
            .path()
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        parts.pop();

        let mut prefix = String::new();
        for part in parts {
            if prefix.is_empty() {
                prefix = part.to_string();
            } else {
                prefix = format!("{prefix}/{part}");
            }
            paths.insert(prefix.clone());
        }
    }

    paths
}

/// Walk every folder in the tree (regardless of collapse state) recording its
/// path and whether it collapses by default. Mirrors the default-collapse logic
/// in [`append_file_tree_rows`] so bulk collapse/expand stays consistent.
fn collect_file_tree_folder_defaults(
    branch: &FileTreeBranch,
    prefix: &str,
    collapse_unchanged_by_default: bool,
    changed_ancestor_paths: &BTreeSet<String>,
    folders: &mut Vec<(String, bool)>,
) {
    for (name, child) in &branch.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let collapsed_by_default =
            collapse_unchanged_by_default && !changed_ancestor_paths.contains(&path);
        folders.push((path.clone(), collapsed_by_default));
        collect_file_tree_folder_defaults(
            child,
            &path,
            collapse_unchanged_by_default,
            changed_ancestor_paths,
            folders,
        );
    }
}

fn append_file_tree_rows(
    branch: &FileTreeBranch,
    depth: usize,
    prefix: &str,
    collapsed_paths: &BTreeSet<String>,
    collapse_unchanged_by_default: bool,
    changed_ancestor_paths: &BTreeSet<String>,
    rows: &mut Vec<FileTreeRow>,
) {
    for (name, child) in &branch.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        // A folder is collapsed by default when it leads to no changed files in
        // "all files" mode. `collapsed_paths` records the user's manual toggles
        // and flips the folder away from its default state.
        let collapsed_by_default =
            collapse_unchanged_by_default && !changed_ancestor_paths.contains(&path);
        let collapsed = collapsed_by_default ^ collapsed_paths.contains(&path);

        rows.push(FileTreeRow::Folder {
            name: name.clone(),
            path: path.clone(),
            depth,
            collapsed,
        });

        if !collapsed {
            append_file_tree_rows(
                child,
                depth + 1,
                &path,
                collapsed_paths,
                collapse_unchanged_by_default,
                changed_ancestor_paths,
                rows,
            );
        }
    }

    for leaf in &branch.files {
        rows.push(FileTreeRow::File {
            name: leaf.name.clone(),
            entry: leaf.entry.clone(),
            depth,
        });
    }
}

fn commit_ancestry_path(
    commits: &[repo::CommitInfo],
    first_sha: &str,
    second_sha: &str,
) -> Option<Vec<String>> {
    let commits_by_sha = commits
        .iter()
        .map(|commit| (commit.sha.as_str(), commit))
        .collect::<BTreeMap<_, _>>();

    commit_ancestry_path_from_descendant(
        &commits_by_sha,
        first_sha,
        second_sha,
        &mut BTreeSet::new(),
    )
    .or_else(|| {
        commit_ancestry_path_from_descendant(
            &commits_by_sha,
            second_sha,
            first_sha,
            &mut BTreeSet::new(),
        )
    })
}

fn commit_ancestry_path_from_descendant(
    commits_by_sha: &BTreeMap<&str, &repo::CommitInfo>,
    current_sha: &str,
    ancestor_sha: &str,
    visited_shas: &mut BTreeSet<String>,
) -> Option<Vec<String>> {
    if current_sha == ancestor_sha {
        return Some(vec![current_sha.to_string()]);
    }

    if !visited_shas.insert(current_sha.to_string()) {
        return None;
    }

    let commit = commits_by_sha.get(current_sha)?;
    for parent_sha in &commit.parent_shas {
        if let Some(mut path) = commit_ancestry_path_from_descendant(
            commits_by_sha,
            parent_sha,
            ancestor_sha,
            visited_shas,
        ) {
            path.insert(0, current_sha.to_string());
            return Some(path);
        }
    }

    None
}

fn debug_path_fragment(path: &str) -> String {
    path.replace('/', "-")
}

fn debug_ref_label_fragment(label: &str) -> String {
    label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn repository_title(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        commit_graph_connector_color_lane, commit_graph_connector_for_lane,
        commit_graph_line_width, commit_graph_merge_in_commit_line_y,
        commit_graph_spanning_connector_requires_center_fill, commit_row_separator_width,
        debug_ref_label_fragment, side_by_side_diff_rows, single_side_diff_rows, App,
        CloseChangeset, DiffLineStatus, FileListEntry, FileListMode, FileTreeRow, Mode,
        OpenChangeset, OpenFailed, ReviewScreen, Selection, FILE_TREE_FOLDER_ICON_SIZE,
        FILE_TREE_FONT_FAMILY, FILE_TREE_INDENT_WIDTH, FILE_TREE_ROW_HEIGHT,
        FILE_TREE_STATUS_ICON_SIZE, FILE_TREE_TEXT_SIZE,
    };
    use crate::graph::{self, GraphConnectorKind};
    use crate::repo::{ChangeKind, DiffSide, INITIAL_COMMIT_LIMIT};
    use crate::settings::{self, RecentRepository, Settings};
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::{font, px, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
    use std::{fs, path::PathBuf};

    /// Open a window holding a freshly constructed `App`, with the
    /// gpui-component theme installed. The theme global is required by themed
    /// widgets such as the changeset resizable split.
    fn add_app_window(cx: &mut TestAppContext) -> WindowHandle<App> {
        cx.update(gpui_component::init);
        cx.add_window(App::new)
    }

    /// Write a settings file at `path` whose only populated field is the recent
    /// repository list. Mirrors how the storage tests seed state on disk.
    fn seed_recent_repositories(
        path: &std::path::Path,
        recent_repositories: Vec<RecentRepository>,
    ) {
        settings::save(
            path,
            &Settings {
                recent_repositories,
            },
        )
        .expect("seed settings store");
    }

    /// Read back just the recent repository list from the settings file.
    fn load_recent_repositories(path: &std::path::Path) -> Vec<RecentRepository> {
        settings::load(path).recent_repositories
    }

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

    fn graph_commit(sha: &str, parent_shas: &[&str]) -> graph::GraphCommit {
        graph::GraphCommit {
            sha: sha.to_string(),
            authored_timestamp: 0,
            parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
        }
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

    fn init_repo_with_detached_head() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("detached.txt"), "base\n").expect("write file");
        let root_oid = commit_all(&repo, "Base", &[]);

        fs::write(dir.path().join("detached.txt"), "tip\n").expect("update file");
        let tip_oid = commit_all(&repo, "Tip", &[root_oid]);
        repo.set_head_detached(tip_oid).expect("detach HEAD");

        drop(repo);

        (dir, tip_oid.to_string())
    }

    fn init_repo_with_changed_and_context_files() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("changed.txt"), "before\n").expect("write changed file");
        fs::write(dir.path().join("context.txt"), "context\n").expect("write context file");
        let root_oid = commit_all(&repo, "Initial", &[]);

        fs::write(dir.path().join("changed.txt"), "after\n").expect("update changed file");
        let update_oid = commit_all(&repo, "Update changed file", &[root_oid]);

        drop(repo);

        (dir, update_oid.to_string())
    }

    fn init_repo_with_nested_changed_and_context_files() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::create_dir_all(dir.path().join("src")).expect("create src dir");
        fs::write(dir.path().join("src/changed.txt"), "before\n").expect("write changed file");
        fs::write(dir.path().join("src/context.txt"), "context\n").expect("write context file");
        let root_oid = commit_all(&repo, "Initial", &[]);

        fs::write(dir.path().join("src/changed.txt"), "after\n").expect("update changed file");
        let update_oid = commit_all(&repo, "Update changed file", &[root_oid]);

        drop(repo);

        (dir, update_oid.to_string())
    }

    fn init_repo_with_deeply_nested_long_paths() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let deep_dir = "deeply/nested/directory/structure/that/keeps/going";
        let long_names = [
            "a_very_long_changed_file_name_for_horizontal_overflow_one.txt",
            "a_very_long_changed_file_name_for_horizontal_overflow_two.txt",
            "a_very_long_changed_file_name_for_horizontal_overflow_three.txt",
            "a_very_long_changed_file_name_for_horizontal_overflow_four.txt",
            "a_very_long_changed_file_name_for_horizontal_overflow_five.txt",
            "a_very_long_changed_file_name_for_horizontal_overflow_six.txt",
        ];
        fs::create_dir_all(dir.path().join(deep_dir)).expect("create nested dirs");
        for name in long_names {
            fs::write(dir.path().join(deep_dir).join(name), "before\n").expect("write file");
        }
        let root_oid = commit_all(&repo, "Initial", &[]);

        for name in long_names {
            fs::write(dir.path().join(deep_dir).join(name), "after\n").expect("update file");
        }
        let update_oid = commit_all(&repo, "Update files", &[root_oid]);

        drop(repo);
        (dir, update_oid.to_string())
    }

    fn init_repo_with_nested_line_stat_changes() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::create_dir_all(dir.path().join("src")).expect("create src dir");
        fs::write(dir.path().join("src/notes.txt"), "keep\nold\n").expect("write notes file");
        let root_oid = commit_all(&repo, "Initial", &[]);

        fs::write(dir.path().join("src/notes.txt"), "keep\nnew\nextra\n")
            .expect("update notes file");
        let update_oid = commit_all(&repo, "Update notes file", &[root_oid]);

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

    fn init_repo_with_empty_rollup_range() -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("roundtrip.txt"), "base\n").expect("write base file");
        let base_oid = commit_all(&repo, "Base", &[]);

        fs::write(dir.path().join("roundtrip.txt"), "changed\n").expect("change file");
        let change_oid = commit_all(&repo, "Change file", &[base_oid]);

        fs::write(dir.path().join("roundtrip.txt"), "base\n").expect("revert file");
        let revert_oid = commit_all(&repo, "Revert file", &[change_oid]);

        drop(repo);

        (
            dir,
            vec![
                revert_oid.to_string(),
                change_oid.to_string(),
                base_oid.to_string(),
            ],
        )
    }

    fn init_repo_with_linear_history(count: usize) -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        let mut newest_first = Vec::with_capacity(count);
        let mut parents = Vec::new();

        for index in 0..count {
            fs::write(dir.path().join("history.txt"), format!("commit {index}\n"))
                .expect("write history file");
            let oid = commit_all(&repo, &format!("Commit {index}"), &parents);
            newest_first.insert(0, oid.to_string());
            parents = vec![oid];
        }

        drop(repo);
        (dir, newest_first)
    }

    fn init_repo_with_diverged_history() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("base.txt"), "base\n").expect("write base file");
        let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

        fs::write(dir.path().join("left.txt"), "left\n").expect("write left file");
        let left_oid =
            commit_all_to_ref_at_time(&repo, Some("refs/heads/left"), "Left", &[root_oid], 30);

        fs::remove_file(dir.path().join("left.txt")).expect("remove left file");
        fs::write(dir.path().join("right.txt"), "right\n").expect("write right file");
        let right_oid =
            commit_all_to_ref_at_time(&repo, Some("refs/heads/right"), "Right", &[root_oid], 20);

        fs::write(dir.path().join("left.txt"), "left\n").expect("restore left file");
        let merge_oid = commit_all_to_ref_at_time(&repo, None, "Merge", &[left_oid, right_oid], 40);
        repo.reference("refs/heads/master", merge_oid, true, "update test HEAD")
            .expect("point HEAD branch at merge");

        drop(repo);

        (dir, left_oid.to_string(), right_oid.to_string())
    }

    fn commit_info_for_graph(sha: &str, parents: &[&str]) -> crate::repo::CommitInfo {
        commit_info_for_graph_at(sha, 0, parents)
    }

    fn commit_info_for_graph_at(
        sha: &str,
        authored_timestamp: i64,
        parents: &[&str],
    ) -> crate::repo::CommitInfo {
        crate::repo::CommitInfo {
            sha: sha.to_string(),
            short_sha: sha.chars().take(7).collect(),
            summary: sha.to_string(),
            author: "Greviewer Tests".to_string(),
            authored_timestamp,
            authored_date: "1970-01-01".to_string(),
            parent_shas: parents.iter().map(|parent| parent.to_string()).collect(),
            branch_names: Vec::new(),
            parent_count: parents.len(),
            is_head: false,
        }
    }

    fn seed_repo_open_mode_with_commits(
        app: &mut App,
        path: PathBuf,
        commits: Vec<crate::repo::CommitInfo>,
    ) {
        let head = commits.first().map(|commit| crate::repo::HeadInfo {
            short_sha: commit.short_sha.clone(),
            summary: commit.summary.clone(),
        });

        app.mode = Mode::RepoOpen {
            repo: crate::repo::OpenRepository {
                path,
                head,
                commits,
                has_more_commits: false,
            },
        };
    }

    fn init_repo_with_merge_range() -> (tempfile::TempDir, String, String, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("base.txt"), "base\n").expect("write base file");
        let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

        fs::write(dir.path().join("side.txt"), "side\n").expect("write side file");
        let side_oid =
            commit_all_to_ref_at_time(&repo, Some("refs/heads/side"), "Side", &[root_oid], 20);

        fs::remove_file(dir.path().join("side.txt")).expect("remove side file");
        fs::write(dir.path().join("main.txt"), "main\n").expect("write main file");
        let main_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main", &[root_oid], 30);

        fs::write(dir.path().join("side.txt"), "side\n").expect("restore side file");
        let merge_oid =
            commit_all_to_ref_at_time(&repo, Some("HEAD"), "Merge", &[main_oid, side_oid], 40);

        drop(repo);

        (
            dir,
            merge_oid.to_string(),
            main_oid.to_string(),
            side_oid.to_string(),
            root_oid.to_string(),
        )
    }

    fn init_repo_with_deleted_file() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("obsolete.txt"), "obsolete\n").expect("write obsolete file");
        let root_oid = commit_all(&repo, "Add obsolete.txt", &[]);

        fs::remove_file(dir.path().join("obsolete.txt")).expect("delete obsolete file");
        let delete_oid = commit_all(&repo, "Delete obsolete.txt", &[root_oid]);

        drop(repo);

        (dir, delete_oid.to_string())
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

    fn init_repo_with_binary_file() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("binary.dat"), b"\xff\xfe\0data").expect("write binary file");
        let oid = commit_all(&repo, "Add binary file", &[]);

        drop(repo);

        (dir, oid.to_string())
    }

    fn init_repo_with_renamed_file() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(
            dir.path().join("old.txt"),
            "line one\nline two\nline three\nold line\nline five\n",
        )
        .expect("write old file");
        let root_oid = commit_all(&repo, "Add old.txt", &[]);

        fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).expect("rename file");
        fs::write(
            dir.path().join("new.txt"),
            "line one\nline two\nline three\nnew line\nline five\n",
        )
        .expect("update renamed file");
        let rename_oid = commit_all(&repo, "Rename old.txt", &[root_oid]);

        drop(repo);

        (dir, rename_oid.to_string())
    }

    fn test_debug_selector(selector: String) -> &'static str {
        Box::leak(selector.into_boxed_str())
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
        let sig =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        commit_all_to_ref_with_signature(repo, update_ref, message, parents, &sig)
    }

    fn commit_all_to_ref_at_time(
        repo: &Repository,
        update_ref: Option<&str>,
        message: &str,
        parents: &[git2::Oid],
        seconds: i64,
    ) -> git2::Oid {
        let time = git2::Time::new(seconds, 0);
        let sig = Signature::new("Greviewer Tests", "tests@greviewer.invalid", &time)
            .expect("create signature");
        commit_all_to_ref_with_signature(repo, update_ref, message, parents, &sig)
    }

    fn commit_all_to_ref_with_signature(
        repo: &Repository,
        update_ref: Option<&str>,
        message: &str,
        parents: &[git2::Oid],
        sig: &Signature<'_>,
    ) -> git2::Oid {
        let mut index = repo.index().expect("open index");
        index.clear().expect("clear index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let parent_commits = parents
            .iter()
            .map(|oid| repo.find_commit(*oid).expect("find parent commit"))
            .collect::<Vec<_>>();
        let parent_refs = parent_commits.iter().collect::<Vec<_>>();

        repo.commit(update_ref, sig, sig, message, &tree, &parent_refs)
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
        let _window = add_app_window(cx);
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

        let window = add_app_window(cx);

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
    async fn opening_a_real_repo_sets_the_window_title_to_the_repo_name(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let expected_title = path
            .file_name()
            .expect("repo directory name")
            .to_string_lossy()
            .to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert_eq!(visual.window_title(), Some(expected_title));
    }

    #[gpui::test]
    async fn opening_another_real_repo_replaces_the_window_title(cx: &mut TestAppContext) {
        let (first_dir, _) = init_repo_with_one_commit();
        let (second_dir, _) = init_repo_with_one_commit();
        let first_path = first_dir.path().to_path_buf();
        let second_path = second_dir.path().to_path_buf();
        let second_title = second_path
            .file_name()
            .expect("repo directory name")
            .to_string_lossy()
            .to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(first_path, window, cx);
                app.open_repository_at(second_path, window, cx);
            })
            .expect("open repos");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert_eq!(visual.window_title(), Some(second_title));
    }

    #[gpui::test]
    async fn opening_a_non_repo_preserves_the_existing_window_title(cx: &mut TestAppContext) {
        let (repo_dir, _) = init_repo_with_one_commit();
        let repo_path = repo_dir.path().to_path_buf();
        let expected_title = repo_path
            .file_name()
            .expect("repo directory name")
            .to_string_lossy()
            .to_string();
        let non_repo_dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_path, window, cx);
                app.open_repository_at(non_repo_dir.path().to_path_buf(), window, cx);
            })
            .expect("open repo then non-repo");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert_eq!(visual.window_title(), Some(expected_title));
    }

    #[gpui::test]
    async fn opening_a_non_repo_without_prior_repo_leaves_the_window_title_empty(
        cx: &mut TestAppContext,
    ) {
        let non_repo_dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(non_repo_dir.path().to_path_buf(), window, cx);
            })
            .expect("open non-repo");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert_eq!(visual.window_title(), None);
    }

    #[gpui::test]
    async fn opening_an_unborn_repo_renders_empty_commit_history(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().to_path_buf();
        Repository::init(&path).expect("init repo");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open unborn repo");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.mode {
                Mode::RepoOpen { repo } => {
                    assert!(repo.head.is_none());
                    assert!(repo.commits.is_empty());
                }
                Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
            })
            .expect("read unborn repo mode");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("commit-history-empty")
            .expect("empty commit history debug bounds");
        assert!(
            visual.debug_bounds("open-changeset").is_none(),
            "unborn repos should not offer opening a changeset"
        );
    }

    #[gpui::test]
    async fn opening_repositories_records_recent_paths_and_moves_reopened_repo_to_top(
        cx: &mut TestAppContext,
    ) {
        let (first_dir, _) = init_repo_with_one_commit();
        let first_path = first_dir
            .path()
            .canonicalize()
            .expect("canonical first path");
        let (second_dir, _) = init_repo_with_one_commit();
        let second_path = second_dir
            .path()
            .canonicalize()
            .expect("canonical second path");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(first_path.clone(), window, cx);
                app.open_repository_at(second_path.clone(), window, cx);
                assert_eq!(
                    app.settings.recent_repositories,
                    vec![
                        RecentRepository::available(second_path.clone()),
                        RecentRepository::available(first_path.clone()),
                    ],
                );

                app.open_repository_at(first_path.clone(), window, cx);
                assert_eq!(
                    app.settings.recent_repositories,
                    vec![
                        RecentRepository::available(first_path),
                        RecentRepository::available(second_path),
                    ],
                );
            })
            .expect("open repositories");
    }

    #[gpui::test]
    async fn persisted_recent_repositories_load_on_startup(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let dir = tempfile::tempdir().expect("create tempdir");
        let store_path = dir.path().join("settings.json");
        let recent_repositories = vec![
            RecentRepository::unavailable(dir.path().join("repo-one")),
            RecentRepository::unavailable(dir.path().join("repo-two")),
        ];
        seed_recent_repositories(&store_path, recent_repositories.clone());

        let window = cx.add_window(|window, cx| {
            App::new_with_settings_store_path(window, cx, store_path.clone())
        });

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.settings.recent_repositories, recent_repositories);
            })
            .expect("read loaded recent repositories");
    }

    #[gpui::test]
    async fn startup_reopens_last_available_repository(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().canonicalize().expect("canonical repo path");
        let state_dir = tempfile::tempdir().expect("create tempdir");
        let store_path = state_dir.path().join("settings.json");
        seed_recent_repositories(&store_path, vec![RecentRepository::available(path.clone())]);

        let window = cx.add_window(|window, cx| {
            App::new_with_settings_store_path(window, cx, store_path.clone())
        });

        window
            .read_with(cx, |app, _cx| match &app.mode {
                Mode::RepoOpen { repo } => {
                    assert_eq!(repo.path, path);
                    let head = repo.head.as_ref().expect("head present");
                    assert_eq!(head.summary, "Add hello.txt");
                }
                Mode::NoRepo => panic!("expected RepoOpen on startup, got NoRepo"),
            })
            .expect("read startup-opened repository");
    }

    #[gpui::test]
    async fn startup_marks_unopenable_last_repository_unavailable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let dir = tempfile::tempdir().expect("create tempdir");
        let missing_path = dir.path().join("missing-repo");
        let store_path = dir.path().join("settings.json");
        seed_recent_repositories(
            &store_path,
            vec![RecentRepository::available(missing_path.clone())],
        );

        let window = cx.add_window(|window, cx| {
            App::new_with_settings_store_path(window, cx, store_path.clone())
        });

        window
            .read_with(cx, |app, cx| {
                assert!(
                    matches!(app.mode, Mode::NoRepo),
                    "expected NoRepo when the last repository cannot be opened",
                );
                assert_eq!(
                    app.settings.recent_repositories,
                    vec![RecentRepository::unavailable(missing_path.clone())],
                );
                assert_eq!(
                    app.notification_count(cx),
                    0,
                    "startup fallback must not raise an error notification",
                );
            })
            .expect("read startup fallback state");

        assert_eq!(
            load_recent_repositories(&store_path),
            vec![RecentRepository::unavailable(missing_path)],
        );
    }

    #[gpui::test]
    async fn opening_repository_persists_recent_repositories_to_disk(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().canonicalize().expect("canonical repo path");
        let state_dir = tempfile::tempdir().expect("create tempdir");
        let store_path = state_dir.path().join("settings.json");
        let window = cx.add_window(|window, cx| {
            App::new_with_settings_store_path(window, cx, store_path.clone())
        });

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
            })
            .expect("open repository");

        assert_eq!(
            load_recent_repositories(&store_path),
            vec![RecentRepository::available(path)],
        );
    }

    #[gpui::test]
    async fn clicking_recent_repository_opens_it(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().canonicalize().expect("canonical repo path");
        let missing_dir = tempfile::tempdir().expect("create tempdir");
        let missing_path = missing_dir.path().join("missing-repo");
        // Put an unavailable entry first so startup auto-open does not fire,
        // leaving the recent list visible and the available row clickable.
        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repositories(
                window,
                cx,
                vec![
                    RecentRepository::unavailable(missing_path),
                    RecentRepository::available(path.clone()),
                ],
            )
        });

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("recent-repository-row-1")
            .expect("recent repository row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| match &app.mode {
                Mode::RepoOpen { repo } => {
                    let head = repo.head.as_ref().expect("head present");
                    assert_eq!(head.summary, "Add hello.txt");
                }
                Mode::NoRepo => panic!("expected RepoOpen, got NoRepo"),
            })
            .expect("read opened recent repository");
    }

    #[gpui::test]
    async fn clicking_unavailable_recent_repository_marks_it_unavailable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);

        let dir = tempfile::tempdir().expect("create tempdir");
        let unavailable_path = dir.path().join("already-unavailable");
        let missing_path = dir.path().join("missing-repo");
        // Put an unavailable entry first so startup auto-open does not fire,
        // leaving the recent list visible and the available row clickable.
        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repositories(
                window,
                cx,
                vec![
                    RecentRepository::unavailable(unavailable_path.clone()),
                    RecentRepository::available(missing_path.clone()),
                ],
            )
        });

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("recent-repository-row-1")
            .expect("recent repository row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, cx| {
                assert!(matches!(app.mode, Mode::NoRepo));
                assert_eq!(
                    app.settings.recent_repositories,
                    vec![
                        RecentRepository::unavailable(unavailable_path.clone()),
                        RecentRepository::unavailable(missing_path.clone()),
                    ],
                );
                assert_eq!(app.notification_count(cx), 1);
            })
            .expect("read unavailable recent repository");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("unavailable-recent-repository-row-1")
            .expect("unavailable recent repository row debug bounds");
    }

    #[gpui::test]
    async fn failed_recent_repository_activation_persists_unavailable_state(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);

        let dir = tempfile::tempdir().expect("create tempdir");
        let missing_path = dir.path().join("missing-repo");
        let store_path = dir.path().join("settings.json");
        // Unavailable sentinel first so construction-time auto-open is skipped;
        // the activation call below is what marks missing_path unavailable.
        seed_recent_repositories(
            &store_path,
            vec![
                RecentRepository::unavailable(dir.path().join("sentinel-repo")),
                RecentRepository::available(missing_path.clone()),
            ],
        );
        let window = cx.add_window(|window, cx| {
            App::new_with_settings_store_path(window, cx, store_path.clone())
        });

        window
            .update(cx, |app, window, cx| {
                app.open_recent_repository(missing_path.clone(), window, cx);
            })
            .expect("activate missing recent repository");

        assert_eq!(
            load_recent_repositories(&store_path),
            vec![
                RecentRepository::unavailable(dir.path().join("sentinel-repo")),
                RecentRepository::unavailable(missing_path),
            ],
        );
    }

    #[gpui::test]
    async fn unavailable_recent_repository_can_be_removed(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);

        let dir = tempfile::tempdir().expect("create tempdir");
        let missing_path = dir.path().join("missing-repo");
        let store_path = dir.path().join("settings.json");
        // Seed as unavailable so the remove button is visible immediately
        // without relying on a click-to-fail path (which is tested separately).
        seed_recent_repositories(
            &store_path,
            vec![RecentRepository::unavailable(missing_path.clone())],
        );
        let window = cx.add_window(|window, cx| {
            App::new_with_settings_store_path(window, cx, store_path.clone())
        });

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let remove_bounds = visual
            .debug_bounds("unavailable-recent-repository-remove-0")
            .expect("unavailable recent repository remove debug bounds");
        visual.simulate_click(remove_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, cx| {
                assert!(app.settings.recent_repositories.is_empty());
                assert_eq!(
                    app.notification_count(cx),
                    0,
                    "removing an unavailable entry must not raise a notification",
                );
            })
            .expect("read removed recent repository");
        assert_eq!(
            load_recent_repositories(&store_path),
            Vec::<RecentRepository>::new(),
        );
    }

    #[gpui::test]
    async fn selecting_commits_toggles_single_selection(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
    async fn selecting_merge_to_second_parent_uses_that_ancestry_path_and_rolls_up_merge_files(
        cx: &mut TestAppContext,
    ) {
        let (dir, merge_sha, main_sha, side_sha, root_sha) = init_repo_with_merge_range();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let (merge_index, main_index, side_index) = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let merge_index = repo
                    .commits
                    .iter()
                    .position(|commit| commit.sha == merge_sha)
                    .expect("merge commit row");
                let main_index = repo
                    .commits
                    .iter()
                    .position(|commit| commit.sha == main_sha)
                    .expect("main commit row");
                let side_index = repo
                    .commits
                    .iter()
                    .position(|commit| commit.sha == side_sha)
                    .expect("side commit row");

                (merge_index, main_index, side_index)
            })
            .expect("read commit row indexes");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let merge_bounds = visual
            .debug_bounds(test_debug_selector(format!("commit-row-{merge_index}")))
            .expect("merge commit row debug bounds");
        visual.simulate_click(merge_bounds.center(), Modifiers::none());

        let side_bounds = visual
            .debug_bounds(test_debug_selector(format!("commit-row-{side_index}")))
            .expect("side commit row debug bounds");
        visual.simulate_click(side_bounds.center(), Modifiers::shift());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Range {
                        start_sha: merge_sha.clone(),
                        end_sha: side_sha.clone(),
                        shas: vec![merge_sha.clone(), side_sha.clone()],
                    },
                );
            })
            .expect("read merge range selection");

        assert!(
            visual
                .debug_bounds(test_debug_selector(format!(
                    "selected-commit-row-{main_index}"
                )))
                .is_none(),
            "first-parent commit should not be selected when the range follows the second parent"
        );

        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    let files = changeset
                        .files
                        .iter()
                        .map(|file| (file.path.as_str(), file.kind))
                        .collect::<Vec<_>>();

                    assert_eq!(changeset.commit_sha, merge_sha);
                    assert_eq!(changeset.base_sha, Some(root_sha));
                    assert_eq!(
                        files,
                        vec![
                            ("main.txt", ChangeKind::Added),
                            ("side.txt", ChangeKind::Added),
                        ],
                    );
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read merge changeset");
    }

    #[test]
    fn commit_graph_horizontal_connectors_use_branch_lane_color() {
        let rows = graph::layout_graph(&[
            graph_commit("merge", &["left", "right"]),
            graph_commit("left", &["base"]),
            graph_commit("right", &["base"]),
            graph_commit("base", &[]),
        ]);

        let branch_out = rows[0]
            .connectors
            .iter()
            .copied()
            .find(|connector| connector.kind == GraphConnectorKind::BranchOut)
            .expect("branch-out connector");
        assert_eq!(commit_graph_connector_color_lane(branch_out), 1);

        let merge_in = rows[2]
            .connectors
            .iter()
            .copied()
            .find(|connector| connector.kind == GraphConnectorKind::MergeIn)
            .expect("merge-in connector");
        assert_eq!(commit_graph_connector_color_lane(merge_in), 1);
    }

    #[test]
    fn commit_graph_connectors_span_intermediate_lanes() {
        let connector = graph::GraphConnector {
            from_lane: 0,
            to_lane: 2,
            kind: GraphConnectorKind::BranchOut,
        };
        let row = graph::GraphRow {
            sha: "wide-branch".to_string(),
            lane: 0,
            lane_count: 3,
            active_lanes: vec![0],
            incoming_lanes: Vec::new(),
            outgoing_lanes: vec![0, 2],
            parent_lanes: vec![0, 2],
            connector_lanes: vec![0, 1, 2],
            connectors: vec![connector],
            lane_colors: vec![Some(0), None, Some(1)],
        };

        assert_eq!(commit_graph_connector_for_lane(&row, 1), Some(connector));
        assert_eq!(commit_graph_connector_color_lane(connector), 2);
    }

    #[test]
    fn commit_graph_empty_spanning_connectors_fill_the_center_gap() {
        let connector = graph::GraphConnector {
            from_lane: 0,
            to_lane: 2,
            kind: GraphConnectorKind::BranchOut,
        };
        let row = graph::GraphRow {
            sha: "wide-branch".to_string(),
            lane: 0,
            lane_count: 3,
            active_lanes: vec![0],
            incoming_lanes: Vec::new(),
            outgoing_lanes: vec![0, 2],
            parent_lanes: vec![0, 2],
            connector_lanes: vec![0, 1, 2],
            connectors: vec![connector],
            lane_colors: vec![Some(0), None, Some(1)],
        };

        assert!(commit_graph_spanning_connector_requires_center_fill(
            &row, 1
        ));
    }

    #[test]
    fn commit_rows_do_not_draw_separators_between_graph_segments() {
        assert_eq!(commit_row_separator_width(), 0.);
    }

    #[test]
    fn commit_graph_bend_radius_is_large_enough_for_smooth_elbows() {
        let bend_radius = super::commit_graph_bend_radius();
        let middle_height = super::commit_graph_middle_height();
        let overlay_height = super::commit_graph_bend_overlay_height();
        let overlay_top = super::commit_graph_bend_overlay_top();
        let line_width = super::commit_graph_line_width();

        assert_eq!(bend_radius, 8.);
        assert_eq!(middle_height, 10.);
        assert!(
            overlay_top < 0.,
            "rounded bends should draw outside the compact middle band instead of stretching it",
        );
        assert!(
            overlay_height >= bend_radius * 2. + line_width,
            "rounded bend overlay should fit the full rounded bend radius",
        );
    }

    #[test]
    fn commit_side_branch_bend_turns_from_horizontal_into_vertical() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();

        assert!(
            bend.first_control.x > bend.start.x,
            "component 3 should start right-first from the horizontal segment",
        );
        assert_eq!(
            bend.first_control.y, bend.start.y,
            "component 3 should have a horizontal tangent at the start",
        );
        assert_eq!(
            bend.second_control.x, bend.end.x,
            "component 3 should end on the branch lane vertical",
        );
        assert!(
            bend.second_control.y > bend.end.y,
            "component 3 should curve upward into the branch lane vertical",
        );
    }

    #[test]
    fn commit_side_branch_bend_keeps_original_shape_before_row_boundary_shift() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();
        let radius = super::commit_graph_bend_radius();
        let control = radius * super::COMMIT_GRAPH_BEND_CUBIC_CONTROL;
        let bend_end_x_in_commit = super::commit_graph_commit_bend_overlay_x() + bend.end.x;
        let dot_center_x =
            super::commit_graph_dot_side_line_width() + super::COMMIT_GRAPH_DOT_SIZE / 2.;
        let bend_end_y_in_middle = super::commit_graph_bend_overlay_top() + bend.end.y;
        let dot_bottom_y = super::commit_graph_dot_bottom_gap_y();

        assert_eq!(
            bend_end_x_in_commit, dot_center_x,
            "component 3 should end on the commit dot's vertical centerline",
        );
        assert_eq!(
            bend_end_y_in_middle,
            dot_bottom_y + super::commit_graph_line_width() / 2.,
            "component 3's local shape should stay just below the commit dot before paint-time shifting",
        );
        assert_eq!(
            bend.end.x - bend.start.x,
            radius,
            "component 3 should use a circular horizontal radius",
        );
        assert_eq!(
            bend.start.y - bend.end.y,
            radius,
            "component 3 should use the same vertical radius as a circular quadrant",
        );
        assert_eq!(
            bend.first_control.x,
            bend.start.x + control,
            "component 3 first control should preserve circular quadrant geometry",
        );
        assert_eq!(
            bend.second_control.y,
            bend.end.y + control,
            "component 3 second control should preserve circular quadrant geometry",
        );
    }

    #[test]
    fn commit_side_branch_dot_connector_bridges_bend_endpoint() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();
        let connector = super::commit_graph_merge_in_commit_dot_connector_geometry();
        let dot_bottom_y =
            -super::commit_graph_bend_overlay_top() + super::commit_graph_dot_bottom_gap_y();

        assert_eq!(
            connector.x + connector.width / 2.,
            bend.end.x,
            "dot connector should be centered on component 3's vertical tangent",
        );
        assert_eq!(
            connector.width,
            super::commit_graph_line_width(),
            "dot connector should match the graph stroke width",
        );
        assert_eq!(
            connector.y, dot_bottom_y,
            "dot connector should start exactly at the commit dot bottom edge",
        );
        assert!(
            connector.y <= bend.end.y && connector.y + connector.height >= bend.end.y,
            "dot connector should cover the component 3 endpoint seam",
        );
    }

    #[test]
    fn shifted_commit_side_branch_dot_connector_bridges_translated_bend_endpoint() {
        let bend = super::commit_graph_merge_in_commit_bend_geometry();
        let connector = super::commit_graph_shifted_merge_in_commit_dot_connector_geometry();
        let original_connector = super::commit_graph_merge_in_commit_dot_connector_geometry();
        let shifted_bend_endpoint_y =
            bend.end.y + super::commit_graph_lower_connector_vertical_shift();

        assert_eq!(
            connector.y, original_connector.y,
            "moving the bend should not move the dot-side filler away from the commit dot",
        );
        assert_eq!(
            connector.height,
            original_connector.height + super::commit_graph_lower_connector_vertical_shift(),
            "dot-side filler should lengthen by the same amount as the bend moved",
        );
        assert!(
            connector.y <= shifted_bend_endpoint_y
                && connector.y + connector.height >= shifted_bend_endpoint_y,
            "dot-side filler should cover the translated component 3 endpoint",
        );
    }

    #[test]
    fn commit_side_merge_target_bend_turns_from_horizontal_into_vertical() {
        let bend = super::commit_graph_merge_target_commit_bend_geometry();

        assert!(
            bend.first_control.x < bend.start.x,
            "merge target component 3 should start left-first from the horizontal segment",
        );
        assert_eq!(
            bend.first_control.y, bend.start.y,
            "merge target component 3 should have a horizontal tangent at the start",
        );
        assert_eq!(
            bend.second_control.x, bend.end.x,
            "merge target component 3 should end on the target commit vertical",
        );
        assert!(
            bend.second_control.y > bend.end.y,
            "merge target component 3 should curve upward into the target commit vertical",
        );
    }

    #[test]
    fn commit_side_merge_target_bend_keeps_original_shape_before_row_boundary_shift() {
        let bend = super::commit_graph_merge_target_commit_bend_geometry();
        let radius = super::commit_graph_bend_radius();
        let control = radius * super::COMMIT_GRAPH_BEND_CUBIC_CONTROL;
        let bend_end_x_in_commit =
            super::commit_graph_merge_target_commit_bend_overlay_x() + bend.end.x;
        let dot_center_x =
            super::commit_graph_dot_side_line_width() + super::COMMIT_GRAPH_DOT_SIZE / 2.;
        let bend_end_y_in_middle = super::commit_graph_bend_overlay_top() + bend.end.y;
        let dot_bottom_y = super::commit_graph_dot_bottom_gap_y();

        assert_eq!(
            bend_end_x_in_commit, dot_center_x,
            "merge target component 3 should end on the commit dot's vertical centerline",
        );
        assert_eq!(
            bend_end_y_in_middle,
            dot_bottom_y + super::commit_graph_line_width() / 2.,
            "merge target component 3's local shape should stay just below the commit dot before paint-time shifting",
        );
        assert_eq!(
            bend.start.x - bend.end.x,
            radius,
            "merge target component 3 should use a circular horizontal radius",
        );
        assert_eq!(
            bend.start.y - bend.end.y,
            radius,
            "merge target component 3 should use the same vertical radius as a circular quadrant",
        );
        assert_eq!(
            bend.first_control.x,
            bend.start.x - control,
            "merge target component 3 first control should preserve circular quadrant geometry",
        );
        assert_eq!(
            bend.second_control.y,
            bend.end.y + control,
            "merge target component 3 second control should preserve circular quadrant geometry",
        );
    }

    #[test]
    fn branch_off_horizontal_component_uses_tangent_bounds_and_baseline() {
        let adjacent = super::commit_graph_branch_off_source_bend_geometry(false);
        assert!(
            adjacent.horizontal_end.is_some(),
            "adjacent branch-off bends need a short horizontal component between circular bends",
        );

        let spanning = super::commit_graph_branch_off_source_bend_geometry(true);
        let horizontal_end = spanning
            .horizontal_end
            .expect("spanning branch-off should draw component 2");
        assert_eq!(
            horizontal_end.y, spanning.curve.end.y,
            "component 2 should share the source bend tangent baseline",
        );
        assert!(
            horizontal_end.x > spanning.curve.end.x,
            "component 2 should begin after component 1 reaches its tangent",
        );
        assert_eq!(
            super::commit_graph_lower_merge_in_horizontal_top_in_middle()
                + super::commit_graph_line_width() / 2.,
            super::commit_graph_merge_in_commit_line_y_in_middle(),
            "component 2 filled segments should be centered on the bend baseline",
        );
    }

    #[test]
    fn lane_change_horizontal_baseline_is_centered_on_the_row_boundary() {
        let row_boundary_center_y_in_middle =
            super::COMMIT_GRAPH_LANE_HEIGHT - super::COMMIT_GRAPH_VERTICAL_HEIGHT;

        assert_eq!(
            super::commit_graph_shifted_lower_merge_in_horizontal_top_in_middle()
                + super::commit_graph_line_width() / 2.,
            row_boundary_center_y_in_middle,
            "horizontal lane-change strokes should be centered on the border between graph rows",
        );
    }

    #[test]
    fn commit_graph_overlay_paints_lower_rows_first() {
        assert_eq!(
            super::commit_graph_overlay_row_indices(4),
            vec![3, 2, 1, 0],
            "lower graph rows should paint first so row-boundary branch turns cover the next row's vertical continuation",
        );
    }

    #[test]
    fn adjacent_branch_off_horizontal_component_connects_circular_bends() {
        let source_bend = super::commit_graph_branch_off_source_bend_geometry(false);
        let commit_bend = super::commit_graph_merge_in_commit_bend_geometry();
        let radius = super::commit_graph_bend_radius();

        let source_horizontal_end = source_bend
            .horizontal_end
            .expect("adjacent branch-offs should draw a short horizontal component");
        let source_horizontal_end_x =
            super::commit_graph_bend_overlay_x() + source_horizontal_end.x;
        let commit_horizontal_start_x = super::COMMIT_GRAPH_LANE_WIDTH;
        let commit_curve_start_x = super::COMMIT_GRAPH_LANE_WIDTH
            + super::commit_graph_commit_bend_overlay_x()
            + commit_bend.start.x;

        assert_eq!(
            source_bend.curve.end.x - source_bend.curve.start.x,
            radius,
            "component 1 should remain a circular quadrant",
        );
        assert_eq!(
            source_bend.curve.start.y - source_bend.curve.end.y,
            radius,
            "component 1 should use the same vertical radius as a circular quadrant",
        );
        assert_eq!(
            source_horizontal_end_x, commit_horizontal_start_x,
            "component 2 should reach the adjacent commit lane boundary",
        );
        assert!(
            commit_curve_start_x > commit_horizontal_start_x,
            "component 2 should continue inside the commit lane until component 3 starts",
        );
        assert_eq!(
            source_bend.curve.end.y, commit_bend.start.y,
            "adjacent branch-off bends should share the same lower baseline",
        );
    }

    #[test]
    fn sibling_branch_extension_turns_up_into_active_outer_lane() {
        let rows = graph::layout_graph(&[
            graph::GraphCommit {
                sha: "merge-lfs".into(),
                authored_timestamp: 50,
                parent_shas: vec!["merge-docs".into(), "lfs-tip".into()],
            },
            graph::GraphCommit {
                sha: "lfs-tip".into(),
                authored_timestamp: 40,
                parent_shas: vec!["trunk-base".into()],
            },
            graph::GraphCommit {
                sha: "merge-docs".into(),
                authored_timestamp: 30,
                parent_shas: vec!["trunk-base".into(), "docs-tip".into()],
            },
            graph::GraphCommit {
                sha: "docs-tip".into(),
                authored_timestamp: 20,
                parent_shas: vec!["trunk-base".into()],
            },
            graph::GraphCommit {
                sha: "trunk-base".into(),
                authored_timestamp: 10,
                parent_shas: Vec::new(),
            },
        ]);
        let row = &rows[3];
        let horizontal_center_y = super::COMMIT_GRAPH_VERTICAL_HEIGHT
            + super::commit_graph_lower_merge_in_horizontal_top_in_middle()
            + super::commit_graph_lower_connector_vertical_shift()
            + super::commit_graph_line_width() / 2.;

        assert_eq!(
            row.sha, "docs-tip",
            "test fixture should inspect the earlier sibling side-branch row",
        );
        assert_eq!(
            super::commit_graph_rounded_elbow_tangent_y(row, 2),
            Some(horizontal_center_y - super::commit_graph_bend_radius()),
            "shared sibling branch extensions should curve upward into the active outer lane",
        );
    }

    #[test]
    fn sibling_branch_extension_renders_below_priority_branch_bend() {
        let layer_order = super::commit_graph_commit_side_layer_order(
            Some(graph::GraphConnector {
                from_lane: 1,
                to_lane: 0,
                kind: graph::GraphConnectorKind::MergeIn,
            }),
            None,
            Some(graph::GraphConnector {
                from_lane: 1,
                to_lane: 2,
                kind: graph::GraphConnectorKind::BranchOut,
            }),
        );

        assert_eq!(
            layer_order,
            vec![
                super::CommitGraphCommitSideLayer::SiblingBranchExtension,
                super::CommitGraphCommitSideLayer::RoundedLeftConnector,
            ],
            "shared sibling branch extensions should render below the active side branch bend",
        );
    }

    #[gpui::test]
    async fn commit_graph_renders_merge_lanes(cx: &mut TestAppContext) {
        let (dir, _left_sha, _right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                assert_eq!(repo.commits[0].parent_shas.len(), 2);
            })
            .expect("read merge commit");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("commit-graph-gutter-0")
            .expect("merge commit graph gutter debug bounds");
        visual
            .debug_bounds("commit-graph-dot-0")
            .expect("merge commit graph dot debug bounds");
        let merge_commit_dot = visual
            .debug_bounds("commit-graph-dot-0")
            .expect("merge commit graph dot debug bounds");
        let merge_commit_bottom_gap = visual
            .debug_bounds("commit-graph-dot-bottom-gap-0-0")
            .expect("merge commit bottom dot gap debug bounds");
        visual
            .debug_bounds("commit-graph-lane-0-1")
            .expect("merge commit second parent lane debug bounds");
        let branch_out_connector_bounds = visual
            .debug_bounds("commit-graph-connector-0-1")
            .expect("merge commit second parent connector debug bounds");
        visual
            .debug_bounds("commit-graph-branch-out-0-1")
            .expect("merge commit branch-out connector debug bounds");
        let branch_out_elbow_bounds = visual
            .debug_bounds("commit-graph-branch-out-elbow-0-1")
            .expect("merge commit branch-out elbow debug bounds");
        let rounded_branch_out_elbow_bounds = visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-0-1")
            .expect("merge commit rounded branch-out elbow debug bounds");
        let merge_target_commit_bend_bounds = visual
            .debug_bounds("commit-graph-rounded-merge-target-commit-elbow-0-0")
            .expect("merge target rounded commit bend debug bounds");
        assert!(
            rounded_branch_out_elbow_bounds.origin.y < branch_out_connector_bounds.origin.y,
            "rounded branch-out elbow should draw outside the compact middle band above the connector",
        );
        assert!(
            rounded_branch_out_elbow_bounds.origin.y
                + rounded_branch_out_elbow_bounds.size.height
                > branch_out_connector_bounds.origin.y + branch_out_connector_bounds.size.height,
            "rounded branch-out elbow should draw outside the compact middle band below the connector",
        );
        let branch_out_middle_vertical_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-0-1")
            .expect("merge commit branch-out middle vertical debug bounds");
        let branch_out_horizontal_bounds = visual
            .debug_bounds("commit-graph-branch-out-horizontal-0-1")
            .expect("merge commit branch-out horizontal debug bounds");
        let merge_commit_row = visual
            .debug_bounds("commit-row-0")
            .expect("merge commit row debug bounds");
        assert_eq!(
            branch_out_horizontal_bounds.origin.y + px(commit_graph_line_width() / 2.),
            merge_target_commit_bend_bounds.origin.y
                + px(super::commit_graph_lower_connector_vertical_shift()
                    + commit_graph_merge_in_commit_line_y()),
            "branch-out horizontal should meet the merge target bend on the lower baseline",
        );
        assert_eq!(
            branch_out_horizontal_bounds.origin.y + px(commit_graph_line_width() / 2.),
            merge_commit_row.origin.y + merge_commit_row.size.height,
            "branch-out horizontal should be centered on the border below the merge row",
        );
        assert_eq!(
            branch_out_middle_vertical_bounds.origin.y, branch_out_horizontal_bounds.origin.y,
            "branch-out middle vertical should not protrude above the horizontal turn",
        );
        let branch_out_vertical_bounds = visual
            .debug_bounds("commit-graph-vertical-0-1-bottom")
            .expect("merge commit second parent outgoing vertical debug bounds");
        assert_eq!(
            branch_out_elbow_bounds.origin.x, branch_out_vertical_bounds.origin.x,
            "branch-out elbow should align with the outgoing lane",
        );
        assert!(
            branch_out_vertical_bounds.origin.y
                >= merge_commit_row.origin.y + merge_commit_row.size.height
                    - px(commit_graph_line_width()),
            "branch-out outgoing vertical should not pull the branch turn above the row border",
        );
        let merge_commit_bottom_bounds = visual
            .debug_bounds("commit-graph-vertical-0-0-bottom")
            .expect("merge commit trunk outgoing vertical debug bounds");
        assert_eq!(
            merge_commit_dot.origin.y + merge_commit_dot.size.height,
            merge_commit_bottom_gap.origin.y,
            "trunk dot gap fill should start at the commit dot edge",
        );
        assert_eq!(
            merge_commit_bottom_gap.origin.y + merge_commit_bottom_gap.size.height,
            merge_commit_bottom_bounds.origin.y,
            "trunk dot gap fill should connect to the outgoing trunk vertical",
        );
        assert!(
            merge_target_commit_bend_bounds.origin.y + merge_target_commit_bend_bounds.size.height
                > merge_commit_dot.origin.y + merge_commit_dot.size.height,
            "merge target commit bend should have room below the trunk commit dot",
        );
        visual
            .debug_bounds("commit-graph-vertical-0-1-bottom")
            .expect("merge commit second parent outgoing vertical debug bounds");
        let continued_lane_top_bounds = visual
            .debug_bounds("commit-graph-vertical-1-1-top")
            .expect("continued second lane incoming vertical debug bounds");
        let continued_lane_row = visual
            .debug_bounds("commit-row-1")
            .expect("continued second lane row debug bounds");
        let continued_lane_middle_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-1-1")
            .expect("continued second lane middle vertical debug bounds");
        let continued_lane_bottom_bounds = visual
            .debug_bounds("commit-graph-vertical-1-1-bottom")
            .expect("continued second lane outgoing vertical debug bounds");
        assert_eq!(
            continued_lane_top_bounds.origin.y,
            continued_lane_row.origin.y
                + px(super::commit_graph_bend_radius() - super::commit_graph_line_width()),
            "continued vertical should start at the previous row's branch-out curve tangent, not at the row border",
        );
        assert_eq!(
            continued_lane_middle_bounds.origin.x, continued_lane_top_bounds.origin.x,
            "continued lane middle vertical should align with the incoming vertical",
        );
        assert_eq!(
            continued_lane_top_bounds.origin.y + continued_lane_top_bounds.size.height,
            continued_lane_middle_bounds.origin.y,
            "continued lane middle vertical should connect to the incoming vertical",
        );
        assert_eq!(
            continued_lane_middle_bounds.origin.y + continued_lane_middle_bounds.size.height,
            continued_lane_bottom_bounds.origin.y,
            "continued lane middle vertical should connect to the outgoing vertical",
        );
        let merge_in_elbow_bounds = visual
            .debug_bounds("commit-graph-merge-in-elbow-2-0")
            .expect("right branch merge-in elbow debug bounds");
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-merge-in-elbow-2-0")
                .is_none(),
            "merge-in target trunk should stay vertical; the final rounded bend belongs at the branch commit",
        );
        let merge_in_vertical_bounds = visual
            .debug_bounds("commit-graph-vertical-2-0-bottom")
            .expect("right branch merge-in outgoing vertical debug bounds");
        let merge_in_top_vertical_bounds = visual
            .debug_bounds("commit-graph-vertical-2-0-top")
            .expect("right branch merge-in incoming trunk vertical debug bounds");
        let merge_in_middle_vertical_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-2-0")
            .expect("right branch merge-in middle trunk vertical debug bounds");
        let merge_in_horizontal_bounds = visual
            .debug_bounds("commit-graph-merge-in-horizontal-2-0")
            .expect("right branch merge-in horizontal debug bounds");
        let right_branch_commit_row = visual
            .debug_bounds("commit-row-2")
            .expect("right branch commit row debug bounds");
        let branch_off_source_bend_bounds = visual
            .debug_bounds("commit-graph-rounded-branch-off-source-elbow-2-0")
            .expect("right branch source-side rounded branch-off bend debug bounds");
        let merge_in_commit_bend_bounds = visual
            .debug_bounds("commit-graph-rounded-merge-in-commit-elbow-2-1")
            .expect("right branch commit rounded merge-in bend debug bounds");
        let right_branch_commit_dot_bounds = visual
            .debug_bounds("commit-graph-dot-2")
            .expect("right branch commit dot debug bounds");
        assert_eq!(
            merge_in_horizontal_bounds.origin.y + px(commit_graph_line_width() / 2.),
            merge_in_commit_bend_bounds.origin.y
                + px(
                    super::commit_graph_lower_connector_vertical_shift()
                        + commit_graph_merge_in_commit_line_y()
                ),
            "merge-in horizontal should start right on the lower baseline before bending up into the branch commit",
        );
        assert_eq!(
            merge_in_horizontal_bounds.origin.y + px(commit_graph_line_width() / 2.),
            right_branch_commit_row.origin.y + right_branch_commit_row.size.height,
            "merge-in horizontal should be centered on the border below the branch commit row",
        );
        assert_eq!(
            merge_in_horizontal_bounds.origin.x,
            merge_in_middle_vertical_bounds.origin.x + merge_in_middle_vertical_bounds.size.width,
            "merge-in horizontal should start at the trunk lane",
        );
        assert!(
            branch_off_source_bend_bounds.origin.x < merge_in_commit_bend_bounds.origin.x,
            "source-side branch-off bend should be left of the commit-side bend",
        );
        assert!(
            branch_off_source_bend_bounds.origin.y < merge_in_horizontal_bounds.origin.y,
            "source-side branch-off bend should have room above the horizontal run for the quarter-turn",
        );
        assert!(
            branch_off_source_bend_bounds.origin.y + branch_off_source_bend_bounds.size.height
                > merge_in_horizontal_bounds.origin.y,
            "source-side branch-off bend should overlap the horizontal run after starting vertically",
        );
        assert!(
            merge_in_commit_bend_bounds.origin.x < right_branch_commit_dot_bounds.origin.x,
            "merge-in commit-side bend should start before the branch commit dot",
        );
        assert!(
            merge_in_commit_bend_bounds.origin.y + merge_in_commit_bend_bounds.size.height
                > right_branch_commit_dot_bounds.origin.y
                    + right_branch_commit_dot_bounds.size.height,
            "merge-in commit-side bend should have room below the branch commit dot",
        );
        assert_eq!(
            merge_in_elbow_bounds.origin.x, merge_in_vertical_bounds.origin.x,
            "merge-in elbow should align with the parent lane",
        );
        assert_eq!(
            merge_in_top_vertical_bounds.origin.y + merge_in_top_vertical_bounds.size.height,
            merge_in_middle_vertical_bounds.origin.y,
            "merge-in target trunk should stay connected above the rounded branch join",
        );
        assert_eq!(
            merge_in_middle_vertical_bounds.origin.y + merge_in_middle_vertical_bounds.size.height,
            merge_in_vertical_bounds.origin.y,
            "merge-in target trunk should stay connected below the rounded branch join",
        );
    }

    #[gpui::test]
    async fn commit_graph_keeps_side_parent_lane_active_when_trunk_merge_shares_parent(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph_at("merge-lfs", 50, &["merge-docs", "lfs-tip"]),
                        commit_info_for_graph_at("lfs-tip", 40, &["trunk-base"]),
                        commit_info_for_graph_at("merge-docs", 30, &["trunk-base", "docs-tip"]),
                        commit_info_for_graph_at("docs-tip", 20, &["trunk-base"]),
                        commit_info_for_graph_at("trunk-base", 10, &[]),
                    ],
                );
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let graph_commits = repo
                    .commits
                    .iter()
                    .map(|commit| graph::GraphCommit {
                        sha: commit.sha.clone(),
                        authored_timestamp: commit.authored_timestamp,
                        parent_shas: commit.parent_shas.clone(),
                    })
                    .collect::<Vec<_>>();
                let rows = graph::layout_graph(&graph_commits);

                assert_eq!(rows[0].parent_lanes, vec![0, 2]);
                assert_eq!(rows[0].connector_lanes, vec![0, 1, 2]);

                assert_eq!(rows[1].lane, 2);
                assert_eq!(rows[1].parent_lanes, vec![2]);
                assert_eq!(rows[1].connector_lanes, vec![2]);
                assert_eq!(rows[1].outgoing_lanes, vec![0, 2]);

                assert_eq!(rows[2].lane, 0);
                assert_eq!(rows[2].incoming_lanes, vec![0, 2]);
                assert_eq!(rows[2].outgoing_lanes, vec![0, 1, 2]);
                assert_eq!(rows[2].parent_lanes, vec![0, 1]);
                assert_eq!(rows[2].connector_lanes, vec![0, 1]);
                assert!(
                    rows[2].connectors.iter().any(|connector| {
                        connector.from_lane == 0
                            && connector.to_lane == 0
                            && connector.kind == GraphConnectorKind::Straight
                    }),
                    "the trunk merge should keep its first-parent edge on the trunk lane",
                );
                assert!(
                    !rows[2].connectors.iter().any(|connector| {
                        connector.from_lane == 0
                            && connector.to_lane == 2
                            && connector.kind == GraphConnectorKind::BranchOut
                    }),
                    "the lfs side edge should not branch from the docs merge row",
                );
                assert!(
                    rows[2].connectors.iter().any(|connector| {
                        connector.from_lane == 0
                            && connector.to_lane == 1
                            && connector.kind == GraphConnectorKind::BranchOut
                    }),
                    "the docs side branch should branch independently from the merge row",
                );
                assert_eq!(rows[3].connector_lanes, vec![0, 1, 2]);
                assert!(
                    rows[3].connectors.iter().any(|connector| {
                        connector.from_lane == 1
                            && connector.to_lane == 2
                            && connector.kind == GraphConnectorKind::BranchOut
                    }),
                    "the lfs side edge should branch from the docs side branch row",
                );
                assert_eq!(
                    rows[3].outgoing_lanes,
                    vec![0],
                    "the lfs side edge should stop after joining the shared sibling branch",
                );
                assert_eq!(rows[4].connector_lanes, Vec::<usize>::new());
                assert!(
                    rows[4].connectors.is_empty(),
                    "the shared parent row should not redraw the sibling branch merge",
                );
            })
            .expect("inspect graph layout");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let side_top = visual
            .debug_bounds("commit-graph-vertical-2-2-top")
            .expect("side lane top vertical through merge row");
        let side_middle = visual
            .debug_bounds("commit-graph-middle-vertical-2-2")
            .expect("side lane middle vertical through merge row");
        let side_bottom = visual
            .debug_bounds("commit-graph-vertical-2-2-bottom")
            .expect("side lane bottom vertical through merge row");

        assert!(
            visual
                .debug_bounds("commit-graph-rounded-spanning-branch-end-elbow-2-2")
                .is_none(),
            "the side lane should pass through the merge row instead of joining the docs branch",
        );
        visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-0-2")
            .expect("lfs side branch should open directly into lane 2");
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-branch-out-elbow-1-2")
                .is_none(),
            "lfs side branch should not hop from lane 1 to lane 2 at its commit row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-spanning-horizontal-through-target-2-1")
                .is_none(),
            "lfs side branch should not draw a horizontal connector on the docs merge row",
        );
        visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-2-1")
            .expect("docs side branch should occupy the first side lane");
        visual
            .debug_bounds("commit-graph-shared-branch-horizontal-3-1")
            .expect("lfs side branch should continue the docs branch line from lane 1");
        let shared_branch_horizontal = visual
            .debug_bounds("commit-graph-shared-branch-horizontal-3-1")
            .expect("lfs side branch should continue the docs branch line from lane 1");
        let docs_tip_row = visual
            .debug_bounds("commit-row-3")
            .expect("docs side branch row debug bounds");
        let branch_out_elbow = visual
            .debug_bounds("commit-graph-rounded-branch-out-elbow-3-2")
            .expect("lfs side branch should curve up from the docs side branch row into lane 2");
        let docs_lane = visual
            .debug_bounds("commit-graph-lane-3-1")
            .expect("docs side branch lane");
        let lfs_lane = visual
            .debug_bounds("commit-graph-lane-3-2")
            .expect("lfs side branch lane");
        let lfs_top_vertical = visual
            .debug_bounds("commit-graph-vertical-3-2-top")
            .expect("lfs side branch incoming vertical");
        let lfs_vertical_bridge = visual
            .debug_bounds("commit-graph-rounded-branch-out-vertical-bridge-3-2")
            .expect("lfs side branch curve should bridge to the incoming vertical");
        let expected_shared_start_x = super::commit_graph_commit_bend_overlay_x()
            + super::commit_graph_merge_in_commit_bend_geometry().start.x;
        let expected_branch_out_tangent_y = super::COMMIT_GRAPH_VERTICAL_HEIGHT
            + super::commit_graph_lower_merge_in_horizontal_top_in_middle()
            + super::commit_graph_lower_connector_vertical_shift()
            + super::commit_graph_line_width() / 2.
            - super::commit_graph_bend_radius();

        assert!(
            shared_branch_horizontal.origin.x + shared_branch_horizontal.size.width
                >= branch_out_elbow.origin.x,
            "shared sibling branch horizontal should overlap the lane-2 curve",
        );
        assert_eq!(
            shared_branch_horizontal.origin.x,
            docs_lane.origin.x + px(expected_shared_start_x),
            "shared sibling branch horizontal should start where the docs branch reaches the lower baseline",
        );
        assert_eq!(
            shared_branch_horizontal.origin.y + px(super::commit_graph_line_width() / 2.),
            docs_tip_row.origin.y + docs_tip_row.size.height,
            "shared sibling branch horizontal should be centered on the border below its source row",
        );
        assert_eq!(
            lfs_vertical_bridge.origin.x, lfs_top_vertical.origin.x,
            "lfs vertical bridge should align with the incoming vertical",
        );
        assert_eq!(
            lfs_vertical_bridge.origin.y,
            lfs_top_vertical.origin.y + lfs_top_vertical.size.height,
            "lfs vertical bridge should start where the incoming vertical ends",
        );
        assert!(
            lfs_vertical_bridge.origin.y + lfs_vertical_bridge.size.height
                >= lfs_lane.origin.y
                    + px(expected_branch_out_tangent_y + super::commit_graph_line_width()),
            "lfs vertical bridge should overlap the upward curve tangent without a seam",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-shared-branch-vertical-3-1")
                .is_none(),
            "lfs side branch should start horizontally, not vertically from the docs commit",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-merge-target-commit-elbow-3-1")
                .is_none(),
            "lfs branch extension should not bend through the docs commit dot",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-4-0")
                .is_none(),
            "shared sibling branch should not redraw a separate merge-in at the common parent row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-spanning-horizontal-left-4-1")
                .is_none(),
            "shared sibling branch should not cross an empty lane at the common parent row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-spanning-horizontal-right-4-1")
                .is_none(),
            "shared sibling branch should not cross an empty lane at the common parent row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-merge-in-source-elbow-4-2")
                .is_none(),
            "shared sibling branch should not draw a second lower bend back toward the trunk",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-source-horizontal-right-4-2")
                .is_none(),
            "lfs merge source should not draw a dangling horizontal to the right",
        );
        assert_eq!(
            side_top.origin.y + side_top.size.height,
            side_middle.origin.y,
            "side lane top segment should connect to the middle segment",
        );
        assert!(
            side_bottom.origin.y <= side_middle.origin.y + side_middle.size.height,
            "side lane middle segment should not leave a gap before the bottom segment",
        );
    }

    #[gpui::test]
    async fn commit_graph_keeps_lower_merge_in_horizontal_aligned_across_occupied_lanes(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph(
                            "merge-tip",
                            &["main-a", "ending-side", "stable-side"],
                        ),
                        commit_info_for_graph("main-a", &["main-base"]),
                        commit_info_for_graph("stable-side", &["main-base"]),
                        commit_info_for_graph("ending-side", &["main-base"]),
                        commit_info_for_graph("main-base", &[]),
                    ],
                );
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let graph_commits = repo
                    .commits
                    .iter()
                    .map(|commit| graph::GraphCommit {
                        sha: commit.sha.clone(),
                        authored_timestamp: commit.authored_timestamp,
                        parent_shas: commit.parent_shas.clone(),
                    })
                    .collect::<Vec<_>>();
                let rows = graph::layout_graph(&graph_commits);
                assert_eq!(rows[2].lane, 2);
                assert_eq!(rows[2].incoming_lanes, vec![0, 1, 2]);
                assert_eq!(rows[2].connector_lanes, vec![0, 1, 2]);
            })
            .expect("inspect graph layout");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let target_horizontal = visual
            .debug_bounds("commit-graph-merge-in-horizontal-2-0")
            .expect("target trunk merge-in horizontal debug bounds");
        let spanning_horizontal = visual
            .debug_bounds("commit-graph-spanning-horizontal-right-2-1")
            .expect("occupied intermediate merge-in horizontal debug bounds");
        let commit_bend = visual
            .debug_bounds("commit-graph-rounded-merge-in-commit-elbow-2-2")
            .expect("commit-side merge-in bend debug bounds");

        assert_eq!(
            target_horizontal.origin.y, spanning_horizontal.origin.y,
            "merge-in horizontal should stay aligned while passing an occupied lane",
        );
        assert_eq!(
            spanning_horizontal.origin.y + px(commit_graph_line_width() / 2.),
            commit_bend.origin.y
                + px(super::commit_graph_lower_connector_vertical_shift()
                    + commit_graph_merge_in_commit_line_y()),
            "spanning horizontal should meet the commit-side bend on the lower baseline",
        );
    }

    #[gpui::test]
    async fn commit_graph_vertical_segments_connect_between_rows(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let first_row_bottom = visual
            .debug_bounds("commit-graph-vertical-0-0-bottom")
            .expect("first row outgoing vertical debug bounds");
        let second_row_top = visual
            .debug_bounds("commit-graph-vertical-1-0-top")
            .expect("second row incoming vertical debug bounds");
        let first_row = visual
            .debug_bounds("commit-row-0")
            .expect("first commit row debug bounds");
        let second_row = visual
            .debug_bounds("commit-row-1")
            .expect("second commit row debug bounds");
        let first_dot = visual
            .debug_bounds("commit-graph-dot-0")
            .expect("first commit dot debug bounds");
        let second_dot = visual
            .debug_bounds("commit-graph-dot-1")
            .expect("second commit dot debug bounds");
        let first_bottom_gap = visual
            .debug_bounds("commit-graph-dot-bottom-gap-0-0")
            .expect("first commit bottom dot gap debug bounds");
        let second_top_gap = visual
            .debug_bounds("commit-graph-dot-top-gap-1-0")
            .expect("second commit top dot gap debug bounds");

        assert_eq!(
            first_row_bottom.origin.y + first_row_bottom.size.height,
            second_row_top.origin.y,
            "commit graph vertical segments should connect across adjacent rows; first row: {first_row:?}, second row: {second_row:?}, first bottom: {first_row_bottom:?}, second top: {second_row_top:?}",
        );
        assert_eq!(
            first_dot.origin.y + first_dot.size.height,
            first_bottom_gap.origin.y,
            "bottom dot gap fill should start at the dot edge",
        );
        assert_eq!(
            first_bottom_gap.origin.y + first_bottom_gap.size.height,
            first_row_bottom.origin.y,
            "bottom dot gap fill should connect to the outgoing vertical",
        );
        assert_eq!(
            second_row_top.origin.y + second_row_top.size.height,
            second_top_gap.origin.y,
            "top dot gap fill should connect to the incoming vertical",
        );
        assert_eq!(
            second_top_gap.origin.y + second_top_gap.size.height,
            second_dot.origin.y,
            "top dot gap fill should end at the dot edge",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-commit-vertical-0-0")
                .is_none()
                && visual
                    .debug_bounds("commit-graph-commit-vertical-1-0")
                    .is_none(),
            "commit dots should not get full-height through-lines that protrude beyond the dot",
        );
    }

    #[gpui::test]
    async fn commit_rows_render_as_single_line_columns_in_requested_order(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                app.mode = Mode::RepoOpen {
                    repo: crate::repo::OpenRepository {
                        path: dir.path().to_path_buf(),
                        head: Some(crate::repo::HeadInfo {
                            short_sha: "abcdef0".to_string(),
                            summary: "Compact row".to_string(),
                        }),
                        commits: vec![crate::repo::CommitInfo {
                            sha: "abcdef0123456789abcdef0123456789abcdef01".to_string(),
                            short_sha: "abcdef0".to_string(),
                            summary: "Collapse graph row into columns".to_string(),
                            author: "Greviewer Tests".to_string(),
                            authored_timestamp: 0,
                            authored_date: "1970-01-01".to_string(),
                            parent_shas: Vec::new(),
                            branch_names: vec!["main".to_string()],
                            parent_count: 0,
                            is_head: true,
                        }],
                        has_more_commits: false,
                    },
                };
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("commit-row-0")
            .expect("commit row debug bounds");
        let graph = visual
            .debug_bounds("commit-graph-gutter-0")
            .expect("graph gutter debug bounds");
        let hash = visual
            .debug_bounds("commit-hash-0")
            .expect("commit hash debug bounds");
        let summary = visual
            .debug_bounds("commit-summary-0")
            .expect("commit summary debug bounds");
        let author = visual
            .debug_bounds("commit-author-0")
            .expect("commit author debug bounds");
        let time = visual
            .debug_bounds("commit-time-0")
            .expect("commit time debug bounds");
        let labels = visual
            .debug_bounds("commit-ref-labels-0")
            .expect("commit labels debug bounds");

        assert!(
            row.size.height <= px(44.),
            "commit row should be compact enough for a single-line layout: {row:?}"
        );
        assert!(graph.origin.x < hash.origin.x, "graph should be first");
        assert!(hash.origin.x < summary.origin.x, "hash should follow graph");
        assert!(
            summary.origin.x < author.origin.x,
            "summary should precede author"
        );
        assert!(
            author.origin.x < time.origin.x,
            "author should precede time"
        );
        assert!(
            time.origin.x < labels.origin.x,
            "time should precede labels"
        );
    }

    #[gpui::test]
    async fn commit_rows_render_head_and_branch_labels(cx: &mut TestAppContext) {
        let (dir, _left_sha, _right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let (head_row, master_row, left_row, right_row) = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let row_for_branch = |branch_name: &str| {
                    repo.commits
                        .iter()
                        .position(|commit| {
                            commit.branch_names.iter().any(|name| name == branch_name)
                        })
                        .expect("branch row")
                };

                (
                    repo.commits
                        .iter()
                        .position(|commit| commit.is_head)
                        .expect("head row"),
                    row_for_branch("master"),
                    row_for_branch("left"),
                    row_for_branch("right"),
                )
            })
            .expect("read branch label rows");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let label_selector = |row: usize, label: &str| {
            Box::leak(format!("commit-ref-label-{row}-{label}").into_boxed_str()) as &'static str
        };
        visual
            .debug_bounds(label_selector(head_row, "head"))
            .expect("head label on merge commit");
        visual
            .debug_bounds(label_selector(master_row, "master"))
            .expect("master label on merge commit");
        visual
            .debug_bounds(label_selector(left_row, "left"))
            .expect("left branch label on left commit");
        visual
            .debug_bounds(label_selector(right_row, "right"))
            .expect("right branch label on right commit");
    }

    #[gpui::test]
    async fn detached_head_repositories_render_without_a_head_marker(cx: &mut TestAppContext) {
        let (dir, tip_sha) = init_repo_with_detached_head();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open detached HEAD repo");

        cx.run_until_parked();

        let master_row = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };

                assert_eq!(repo.commits.len(), 2);
                assert_eq!(repo.commits[0].sha, tip_sha);
                assert!(
                    repo.commits.iter().all(|commit| !commit.is_head),
                    "detached HEAD should not mark a checked-out branch tip"
                );

                repo.commits
                    .iter()
                    .position(|commit| commit.branch_names.iter().any(|name| name == "master"))
                    .expect("master branch row")
            })
            .expect("read detached HEAD repo");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("commit-row-0")
            .expect("tip commit row debug bounds");
        visual
            .debug_bounds(test_debug_selector(format!(
                "commit-ref-label-{master_row}-master"
            )))
            .expect("master branch label debug bounds");
        assert!(
            visual.debug_bounds("commit-ref-label-0-head").is_none(),
            "detached HEAD should not render a HEAD label"
        );
    }

    #[gpui::test]
    async fn long_branch_labels_do_not_cover_the_commit_graph(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let branch_name = "not-merged-branch-with-a-name-that-would-cover-the-graph".to_string();
        let label_selector = Box::leak(
            format!(
                "commit-ref-label-0-{}",
                debug_ref_label_fragment(&branch_name)
            )
            .into_boxed_str(),
        ) as &'static str;
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                app.mode = Mode::RepoOpen {
                    repo: crate::repo::OpenRepository {
                        path: dir.path().to_path_buf(),
                        head: Some(crate::repo::HeadInfo {
                            short_sha: "abcdef0".to_string(),
                            summary: "Long branch label".to_string(),
                        }),
                        commits: vec![crate::repo::CommitInfo {
                            sha: "abcdef0123456789abcdef0123456789abcdef01".to_string(),
                            short_sha: "abcdef0".to_string(),
                            summary: "Long branch label".to_string(),
                            author: "Greviewer Tests".to_string(),
                            authored_timestamp: 0,
                            authored_date: "1970-01-01".to_string(),
                            parent_shas: Vec::new(),
                            branch_names: vec![branch_name],
                            parent_count: 0,
                            is_head: false,
                        }],
                        has_more_commits: false,
                    },
                };
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let label_bounds = visual
            .debug_bounds(label_selector)
            .expect("long branch label debug bounds");
        let graph_bounds = visual
            .debug_bounds("commit-graph-gutter-0")
            .expect("commit graph gutter debug bounds");

        assert!(
            label_bounds.origin.x >= graph_bounds.origin.x + graph_bounds.size.width,
            "branch label should not cover the graph gutter; label: {label_bounds:?}, graph: {graph_bounds:?}"
        );
    }

    #[gpui::test]
    async fn scrolling_commit_history_loads_older_commits(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, shas) = init_repo_with_linear_history(INITIAL_COMMIT_LIMIT + 2);
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                assert_eq!(repo.commits.len(), INITIAL_COMMIT_LIMIT);
                assert!(repo.has_more_commits);
            })
            .expect("read initial commit page");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));
        let first_row_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("first commit row debug bounds");
        let before_scroll = window
            .read_with(cx, |app, _cx| app.commit_history_scroll.offset())
            .expect("read commit history offset before wheel");
        visual.simulate_event(ScrollWheelEvent {
            position: first_row_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
            ..Default::default()
        });
        cx.run_until_parked();
        let (after_scroll, max_scroll) = window
            .read_with(cx, |app, _cx| {
                (
                    app.commit_history_scroll.offset(),
                    app.commit_history_scroll.max_offset(),
                )
            })
            .expect("read commit history offset after wheel");
        assert!(
            max_scroll.height > px(0.),
            "long commit history should exceed the visible graph area"
        );
        assert!(
            after_scroll.y < before_scroll.y,
            "wheel scroll should move the commit history upward; before: {before_scroll:?}, after: {after_scroll:?}, max: {max_scroll:?}"
        );

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                assert_eq!(repo.commits.len(), INITIAL_COMMIT_LIMIT + 2);
                assert!(!repo.has_more_commits);
                assert_eq!(
                    repo.commits.last().expect("oldest loaded commit").sha,
                    shas[INITIAL_COMMIT_LIMIT + 1]
                );
            })
            .expect("read loaded commit page");

        let oldest_row_selector =
            Box::leak(format!("commit-row-{}", INITIAL_COMMIT_LIMIT + 1).into_boxed_str())
                as &'static str;
        visual
            .debug_bounds(oldest_row_selector)
            .expect("oldest loaded commit row debug bounds");
    }

    /// Open a deeply-nested-long-paths repo, select its single commit, open the
    /// changeset view, and resize the window to 360×200. Returns the window
    /// handle (for `read_with` access to app state) and the visual context (for
    /// `debug_bounds` / simulate calls).
    ///
    /// Both `file_tree_scrolls_both_axes` and `file_tree_rows_are_uniform_width`
    /// share this setup; extract here to keep each test focused on its own
    /// assertions.
    fn open_deeply_nested_changeset_at_360x200(
        cx: &mut TestAppContext,
    ) -> (WindowHandle<App>, VisualTestContext) {
        use gpui::{px, size};

        let (dir, sha) = init_repo_with_deeply_nested_long_paths();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(sha.clone(), cx);
            })
            .expect("open repo and select commit");
        cx.run_until_parked();

        // Open the changeset view (transitions review_screen to Changeset mode,
        // which renders the file tree with changed-files-scroll).
        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open-changeset button must be visible after selecting a commit");
        visual.simulate_click(open_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        visual.simulate_resize(size(px(360.), px(200.)));
        cx.run_until_parked();

        (window, visual)
    }

    #[gpui::test]
    async fn file_tree_scrolls_both_axes(cx: &mut TestAppContext) {
        use gpui::px;

        let (window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // Query bounds to ensure a layout pass has occurred, then read scroll state.
        visual
            .debug_bounds("changed-files-scroll")
            .expect("changed-files-scroll must be rendered in changeset view");
        cx.run_until_parked();

        let max_offset = window
            .read_with(cx, |app, _cx| app.file_tree_scroll.max_offset())
            .expect("read file tree max offset");

        assert!(
            max_offset.height > px(0.),
            "nested changeset should overflow the file tree vertically; max: {max_offset:?}"
        );
        assert!(
            max_offset.width > px(0.),
            "long nested paths should overflow the file tree horizontally; max: {max_offset:?}"
        );
    }

    #[gpui::test]
    async fn file_tree_rows_are_uniform_width(cx: &mut TestAppContext) {
        use gpui::px;

        let (_window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // The folder row "deeply" (index 0) has short content. The fixture's
        // deep_dir has 7 components, so folder rows occupy indices 0-6; the first
        // changed-file row is at index 7. With w_full() on every row and a
        // flex_none inner wrapper, both rows must expand to the widest row's width
        // — which exceeds the 360 px viewport. We verify rows extend beyond the
        // viewport width, proving they fill the scrolled content width rather than
        // clipping to the viewport. (debug_bounds returns layout bounds, not
        // viewport-clipped bounds, so a value > 360 px is genuine.)
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-deeply")
            .expect("top-level folder row must be rendered");
        // NOTE: "changed-file-row-7" is index 7 because deep_dir
        // ("deeply/nested/directory/structure/that/keeps/going") has exactly 7
        // path components, placing folder rows at indices 0–6 and the first file
        // row at index 7. If deep_dir in init_repo_with_deeply_nested_long_paths
        // is ever changed to a path with a different component count, this index
        // must be updated to match.
        let file_bounds = visual
            .debug_bounds("changed-file-row-7")
            .expect("first changed-file row (index 7, after 7 folder levels) must be rendered");

        assert!(
            folder_bounds.size.width > px(360.),
            "folder row (short content) should expand to scrolled content width, not clip to \
             viewport; got {:?}",
            folder_bounds.size.width,
        );
        assert!(
            file_bounds.size.width > px(360.),
            "changed-file row (long content) should extend beyond viewport; got {:?}",
            file_bounds.size.width,
        );
        // Both rows must be the same width: the short folder row is pulled up to
        // the wrapper's full width by w_full(), matching the long file row.
        assert!(
            (folder_bounds.size.width - file_bounds.size.width).abs() <= px(1.),
            "folder and changed-file rows must have equal width (uniform backgrounds); \
             folder={:?}, file={:?}",
            folder_bounds.size.width,
            file_bounds.size.width,
        );
    }

    #[gpui::test]
    async fn file_tree_scrollbar_reveals_on_panel_hover(cx: &mut TestAppContext) {
        use gpui::{point, px, Modifiers};

        // Reuse the Task 1 setup helper (deeply-nested changeset, 360×200 window).
        let (_window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // Not hovered: no scrollbar overlay is rendered.
        assert!(
            visual.debug_bounds("file-tree-scrollbar").is_none(),
            "scrollbar overlay should be hidden until the panel is hovered"
        );

        // Move the cursor into the file-tree panel.
        let panel = visual
            .debug_bounds("changed-files")
            .expect("file tree panel bounds");
        visual.simulate_mouse_move(panel.center(), None, Modifiers::default());
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("file-tree-scrollbar").is_some(),
            "scrollbar overlay should appear while the cursor is over the panel"
        );

        // Move the cursor outside the panel; the overlay should disappear.
        visual.simulate_mouse_move(point(px(-10.), px(-10.)), None, Modifiers::default());
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("file-tree-scrollbar").is_none(),
            "scrollbar overlay should disappear when the cursor leaves the panel"
        );
    }

    #[gpui::test]
    async fn opening_changeset_requires_a_selection(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
    async fn closing_changeset_clears_the_context_popover(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex.clone(), cx);
                app.open_changeset(window, cx);
                app.context_popover_open = true;
                app.close_changeset(cx);
            })
            .expect("close changeset");

        window
            .read_with(cx, |app, _cx| {
                assert!(!app.context_popover_open);
                assert!(matches!(app.review_screen, ReviewScreen::Graph));
            })
            .expect("read state");
    }

    #[gpui::test]
    async fn opening_changeset_clears_the_context_popover(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex.clone(), cx);
                app.open_changeset(window, cx);
                // Simulate the popover being left open, then re-open the
                // changeset: opening must dismiss the popover.
                app.context_popover_open = true;
                app.open_changeset(window, cx);
            })
            .expect("reopen changeset");

        window
            .read_with(cx, |app, _cx| {
                assert!(!app.context_popover_open);
                assert!(matches!(app.review_screen, ReviewScreen::Changeset { .. }));
            })
            .expect("read state");
    }

    #[gpui::test]
    async fn opening_range_changeset_renders_rollup_changed_files(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_three_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
    async fn opening_empty_rollup_changeset_shows_empty_state(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_empty_rollup_range();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let revert_bounds = visual
            .debug_bounds("commit-row-0")
            .expect("revert commit row debug bounds");
        visual.simulate_click(revert_bounds.center(), Modifiers::none());

        let change_bounds = visual
            .debug_bounds("commit-row-1")
            .expect("change commit row debug bounds");
        visual.simulate_click(change_bounds.center(), Modifiers::shift());

        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { sha, changeset } => {
                    assert_eq!(sha, &shas[0]);
                    assert_eq!(changeset.commit_sha, shas[0]);
                    assert_eq!(changeset.base_sha, Some(shas[2].clone()));
                    assert!(changeset.files.is_empty());
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read empty rollup changeset");

        visual
            .debug_bounds("changed-files-empty")
            .expect("empty changeset state debug bounds");
    }

    #[gpui::test]
    async fn toggling_all_files_shows_unchanged_files_and_opens_read_only_content(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        let unchanged_bounds = visual
            .debug_bounds("unchanged-file-row-1")
            .expect("unchanged file row debug bounds");
        visual.simulate_click(unchanged_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-read-only-content")
            .expect("read-only file content debug bounds");

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selected_changed_file_path,
                    Some("context.txt".to_string()),
                );
            })
            .expect("read selected context file");
    }

    #[gpui::test]
    async fn all_files_mode_aligns_unchanged_and_changed_file_icons_at_the_same_depth(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        let changed_icon_bounds = visual
            .debug_bounds("changed-file-kind-src-changed.txt")
            .expect("changed file icon debug bounds");
        let unchanged_icon_bounds = visual
            .debug_bounds("unchanged-file-icon-src-context.txt")
            .expect("unchanged file icon debug bounds");

        assert_eq!(
            unchanged_icon_bounds.origin.x, changed_icon_bounds.origin.x,
            "unchanged files should use the same icon slot as changed files at the same depth"
        );
    }

    #[gpui::test]
    async fn selecting_changed_file_in_all_files_mode_still_renders_side_by_side_diff(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        let changed_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(changed_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
    }

    #[gpui::test]
    async fn file_list_renders_nested_paths_as_tree_folders(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("file-tree-folder-src")
            .expect("src folder debug bounds");
        visual
            .debug_bounds("changed-file-row-1")
            .expect("nested changed file row debug bounds");
    }

    #[gpui::test]
    async fn file_tree_rows_render_icons_status_icons_and_diff_stats(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_nested_line_stat_changes();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "src/notes.txt");
                    assert_eq!(changeset.files[0].line_stats.added, 2);
                    assert_eq!(changeset.files[0].line_stats.removed, 1);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read changeset line stats");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder_icon_bounds = visual
            .debug_bounds("file-tree-folder-icon-open-src")
            .expect("folder icon debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-outline-src")
            .expect("open folder outline debug bounds");
        let guide_bounds = visual
            .debug_bounds("file-tree-indent-guide-src-notes.txt-0")
            .expect("nested file indent guide debug bounds");
        let changed_kind_bounds = visual
            .debug_bounds("changed-file-kind-src-notes.txt")
            .expect("changed file kind marker debug bounds");
        assert_eq!(
            guide_bounds.origin.x + guide_bounds.size.width / 2.,
            folder_icon_bounds.origin.x + folder_icon_bounds.size.width / 2.,
            "nested guide should be centered under its parent folder icon"
        );
        assert!(
            changed_kind_bounds.origin.x - (guide_bounds.origin.x + guide_bounds.size.width)
                >= px(11.),
            "nested file item should have breathing room after the guide"
        );
        visual
            .debug_bounds("changed-file-status-icon-src-notes.txt")
            .expect("changed file status icon debug bounds");
        let status_icon_bounds = visual
            .debug_bounds("changed-file-status-icon-src-notes.txt")
            .expect("changed file status icon debug bounds");
        assert_eq!(
            status_icon_bounds.size.width,
            px(FILE_TREE_STATUS_ICON_SIZE)
        );
        assert_eq!(
            status_icon_bounds.size.height,
            px(FILE_TREE_STATUS_ICON_SIZE)
        );
        visual
            .debug_bounds("changed-file-diff-stat-src-notes.txt")
            .expect("changed file diff stat debug bounds");
    }

    #[test]
    fn file_tree_indent_width_is_compact() {
        assert_eq!(FILE_TREE_INDENT_WIDTH, 16.);
    }

    #[test]
    fn file_tree_density_matches_zed_reference_scale() {
        assert_eq!(FILE_TREE_ROW_HEIGHT, 24.);
        assert_eq!(FILE_TREE_TEXT_SIZE, 14.);
        assert_eq!(FILE_TREE_FOLDER_ICON_SIZE, 16.);
        assert_eq!(FILE_TREE_STATUS_ICON_SIZE, 14.);
    }

    #[test]
    fn file_tree_font_family_uses_installed_berkeley_mono_family() {
        assert_eq!(FILE_TREE_FONT_FAMILY, "BerkeleyMono Nerd Font");
    }

    #[gpui::test]
    async fn file_tree_font_family_resolves_without_fallback(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let requested_font = font(FILE_TREE_FONT_FAMILY);
            let font_id = cx.text_system().resolve_font(&requested_font);
            let resolved_font = cx
                .text_system()
                .get_font_for_id(font_id)
                .expect("resolved font should be cached");

            assert_eq!(resolved_font.family.as_ref(), FILE_TREE_FONT_FAMILY);
        });
    }

    #[gpui::test]
    async fn deleted_file_tree_rows_render_deletion_marker_and_struck_name(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_deleted_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open deleted file changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("changed-file-status-icon-obsolete.txt")
            .expect("deleted file status icon debug bounds");
        visual
            .debug_bounds("changed-file-deleted-strike-obsolete.txt")
            .expect("deleted file strike debug bounds");
    }

    #[gpui::test]
    async fn collapsing_file_tree_folder_persists_across_file_list_mode_toggle(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-src")
            .expect("src folder debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-src")
            .expect("open folder icon debug bounds");
        visual.simulate_click(folder_bounds.center(), Modifiers::none());

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("file-tree-folder-icon-closed-src")
            .expect("closed folder icon debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-closed-body-src")
            .expect("closed folder body debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-closed-tab-src")
            .expect("closed folder tab debug bounds");

        window
            .read_with(cx, |app, _cx| {
                assert!(app.collapsed_file_tree_paths.contains("src"));
            })
            .expect("read collapsed folder state");

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let ReviewScreen::Changeset { changeset, .. } = &app.review_screen else {
                    panic!("expected changeset screen");
                };
                let rows = app
                    .file_list_entries(repo, changeset)
                    .map(|entries| app.file_tree_rows(entries))
                    .expect("file tree rows");

                assert_eq!(rows.len(), 1, "collapsed folder should hide children");
                assert!(matches!(
                    &rows[0],
                    FileTreeRow::Folder {
                        path,
                        collapsed: true,
                        ..
                    } if path == "src"
                ));
            })
            .expect("read collapsed tree rows");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let ReviewScreen::Changeset { changeset, .. } = &app.review_screen else {
                    panic!("expected changeset screen");
                };
                let rows = app
                    .file_list_entries(repo, changeset)
                    .map(|entries| app.file_tree_rows(entries))
                    .expect("file tree rows");

                assert_eq!(
                    app.file_list_mode,
                    FileListMode::All,
                    "test should be in all-files mode"
                );
                assert_eq!(
                    rows.len(),
                    1,
                    "collapsed folder should hide all file children after toggling mode"
                );
                assert!(matches!(
                    &rows[0],
                    FileTreeRow::Folder {
                        path,
                        collapsed: true,
                        ..
                    } if path == "src"
                ));
            })
            .expect("read collapsed all-files tree rows");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-src")
            .expect("src folder debug bounds after toggling mode");
        visual.simulate_click(folder_bounds.center(), Modifiers::none());

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("changed-file-row-1")
            .expect("changed file child after expanding folder");
        visual
            .debug_bounds("unchanged-file-row-2")
            .expect("unchanged file child after expanding folder");
    }

    fn changed_file_entry(path: &str) -> FileListEntry {
        FileListEntry::Changed(crate::repo::ChangedFile {
            path: path.to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            is_binary: false,
            line_stats: crate::repo::LineStats::default(),
        })
    }

    fn unchanged_file_entry(path: &str) -> FileListEntry {
        FileListEntry::Unchanged(crate::repo::RepositoryFile {
            path: path.to_string(),
        })
    }

    fn folder_collapsed(rows: &[FileTreeRow], folder_path: &str) -> bool {
        rows.iter()
            .find_map(|row| match row {
                FileTreeRow::Folder {
                    path, collapsed, ..
                } if path == folder_path => Some(*collapsed),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing folder row for {folder_path}"))
    }

    #[gpui::test]
    async fn all_files_mode_collapses_folders_without_changes_by_default(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, _cx| {
                app.file_list_mode = FileListMode::All;
                app.file_tree_rows(vec![
                    changed_file_entry("src/app/changed.rs"),
                    unchanged_file_entry("src/app/sibling/context.rs"),
                    unchanged_file_entry("docs/readme.md"),
                ])
            })
            .expect("compute all-files tree rows");

        assert!(
            !folder_collapsed(&rows, "src"),
            "folders on a changed path stay expanded"
        );
        assert!(
            !folder_collapsed(&rows, "src/app"),
            "folders on a changed path stay expanded at every depth"
        );
        assert!(
            folder_collapsed(&rows, "src/app/sibling"),
            "sibling folder without changes is collapsed by default"
        );
        assert!(
            folder_collapsed(&rows, "docs"),
            "unrelated folder without changes is collapsed by default"
        );
        assert!(
            rows.iter().any(|row| row.path() == "src/app/changed.rs"),
            "changed file stays visible"
        );
        assert!(
            !rows.iter().any(|row| row.path() == "docs/readme.md"),
            "collapsed folder hides its files"
        );
    }

    #[gpui::test]
    async fn toggle_file_list_mode_flips_between_changed_and_all(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                assert_eq!(app.file_list_mode, FileListMode::Changed);
                app.toggle_file_list_mode(cx);
                assert_eq!(app.file_list_mode, FileListMode::All);
                app.toggle_file_list_mode(cx);
                assert_eq!(app.file_list_mode, FileListMode::Changed);
            })
            .expect("toggle file list mode");
    }

    #[gpui::test]
    async fn collapse_all_collapses_every_folder(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, cx| {
                app.file_list_mode = FileListMode::Changed;
                let entries = vec![
                    changed_file_entry("src/app/a.rs"),
                    changed_file_entry("src/b.rs"),
                    changed_file_entry("docs/c.md"),
                ];
                let folders = app.file_tree_folder_defaults(&entries);
                app.apply_folder_collapse(&folders, true, cx);
                app.file_tree_rows(entries)
            })
            .expect("collapse-all tree rows");

        assert!(folder_collapsed(&rows, "src"), "top-level folder collapsed");
        assert!(
            folder_collapsed(&rows, "docs"),
            "top-level folder collapsed"
        );
        assert!(
            !rows.iter().any(|row| row.path() == "src/app"),
            "collapsing the parent hides nested folders"
        );
        assert!(
            !rows.iter().any(|row| row.path() == "src/b.rs"),
            "collapsing hides files"
        );
    }

    #[gpui::test]
    async fn expand_all_expands_every_folder(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, cx| {
                app.file_list_mode = FileListMode::All;
                let entries = vec![
                    changed_file_entry("src/app/changed.rs"),
                    unchanged_file_entry("docs/nested/readme.md"),
                ];
                let folders = app.file_tree_folder_defaults(&entries);
                app.apply_folder_collapse(&folders, false, cx);
                app.file_tree_rows(entries)
            })
            .expect("expand-all tree rows");

        assert!(
            !folder_collapsed(&rows, "docs"),
            "expand-all overrides the default collapse of unchanged folders"
        );
        assert!(
            !folder_collapsed(&rows, "docs/nested"),
            "expand-all reaches nested folders"
        );
        assert!(
            rows.iter().any(|row| row.path() == "docs/nested/readme.md"),
            "expanded folders reveal their files"
        );
    }

    #[gpui::test]
    async fn changed_mode_keeps_all_folders_expanded_by_default(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, _cx| {
                app.file_list_mode = FileListMode::Changed;
                app.file_tree_rows(vec![changed_file_entry("src/app/changed.rs")])
            })
            .expect("compute changed tree rows");

        assert!(
            !folder_collapsed(&rows, "src"),
            "changed mode leaves folders expanded by default"
        );
        assert!(!folder_collapsed(&rows, "src/app"));
    }

    #[gpui::test]
    async fn selecting_changed_file_records_path(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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

        // The close affordance now lives only on the CloseChangeset action; the
        // header button was removed pending its move into the window bar.
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
            .expect("read closed review state");
    }

    #[gpui::test]
    async fn clicking_a_commit_row_toggles_selection(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
    async fn binary_changed_files_show_no_text_indicator(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_binary_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open binary changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("changed-file-binary-indicator-binary.dat")
            .expect("binary changed file indicator debug bounds");

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-binary")
            .expect("binary file placeholder debug bounds");
    }

    #[gpui::test]
    async fn renamed_changed_files_surface_old_path_and_render_side_by_side_diff(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_renamed_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open renamed changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "new.txt");
                    assert_eq!(changeset.files[0].old_path.as_deref(), Some("old.txt"));
                    assert_eq!(changeset.files[0].kind, ChangeKind::Renamed);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read renamed changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("changed-file-kind-new.txt")
            .expect("renamed change kind debug bounds");
        visual
            .debug_bounds("changed-file-rename-source-new.txt")
            .expect("changed file rename source debug bounds");

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-detail-rename-source-new.txt")
            .expect("file detail rename source debug bounds");
        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-removed")
            .expect("old renamed content diff row debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("new renamed content diff row debug bounds");
    }

    #[gpui::test]
    async fn renamed_file_row_is_single_line(cx: &mut TestAppContext) {
        use gpui::px;

        let (dir, oid_hex) = init_repo_with_renamed_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open renamed changeset");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);

        // The rename source is still rendered (now inline on the name line).
        visual
            .debug_bounds("changed-file-rename-source-new.txt")
            .expect("inline rename source bounds");

        // A single-line row is about FILE_TREE_ROW_HEIGHT tall; a two-line row is
        // visibly taller. Allow a small tolerance for padding/border.
        let row = visual
            .debug_bounds("changed-file-row-0")
            .expect("renamed file row bounds");
        assert!(
            row.size.height <= px(FILE_TREE_ROW_HEIGHT + 6.),
            "renamed file row should be single-line; height was {:?}",
            row.size.height
        );
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_detail_shell(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
    async fn changeset_resizable_split_lays_file_tree_left_of_the_diff(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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

        let tree_bounds = visual
            .debug_bounds("changed-files-scroll")
            .expect("changed files scroll debug bounds");
        let detail_bounds = visual
            .debug_bounds("file-detail-shell")
            .expect("file detail shell debug bounds");

        assert!(
            tree_bounds.right() <= detail_bounds.left(),
            "file tree should sit left of the diff panel: tree right {:?}, detail left {:?}",
            tree_bounds.right(),
            detail_bounds.left(),
        );
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_text_diff_content(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
    async fn added_file_diff_renders_only_the_new_side(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open added file changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "hello.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Added);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read added file changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("added line row debug bounds");
        assert!(
            visual.debug_bounds("file-diff-side-old").is_none(),
            "added file diff should not render an empty old-side pane"
        );
    }

    #[gpui::test]
    async fn deleted_file_diff_renders_only_the_old_side(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_deleted_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open deleted file changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "obsolete.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Deleted);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read deleted file changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed line row debug bounds");
        assert!(
            visual.debug_bounds("file-diff-side-new").is_none(),
            "deleted file diff should not render an empty new-side pane"
        );
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_line_highlights(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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
        let window = add_app_window(cx);

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

        let window = add_app_window(cx);
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
