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
    EventEmitter, FocusHandle, InteractiveElement, IntoElement, Modifiers, ParentElement, Pixels,
    Render, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::notification::{Notification, NotificationList};
use similar::{DiffTag, TextDiff};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

use crate::{graph, repo};

actions!(app, [OpenRepository, OpenChangeset, CloseChangeset]);

const MAX_RECENT_REPOSITORIES: usize = 10;

pub struct App {
    pub mode: Mode,
    pub selection: Selection,
    pub review_screen: ReviewScreen,
    pub selected_changed_file_path: Option<String>,
    pub file_list_mode: FileListMode,
    pub recent_repositories: Vec<RecentRepository>,
    collapsed_file_tree_paths: BTreeSet<String>,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    recent_repository_store_path: Option<PathBuf>,
    file_diff_scroll: FileDiffScroll,
    commit_history_scroll: ScrollHandle,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileListMode {
    Changed,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentRepository {
    pub path: PathBuf,
    pub available: bool,
}

impl RecentRepository {
    pub fn available(path: PathBuf) -> Self {
        Self {
            path,
            available: true,
        }
    }

    pub fn unavailable(path: PathBuf) -> Self {
        Self {
            path,
            available: false,
        }
    }
}

fn load_recent_repositories(path: &Path) -> io::Result<Vec<RecentRepository>> {
    let content = fs::read_to_string(path)?;

    Ok(content
        .lines()
        .filter_map(parse_recent_repository_line)
        .take(MAX_RECENT_REPOSITORIES)
        .collect())
}

fn save_recent_repositories(
    path: &Path,
    recent_repositories: &[RecentRepository],
) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let mut content = String::new();
    for recent in recent_repositories.iter().take(MAX_RECENT_REPOSITORIES) {
        let availability = if recent.available { "1" } else { "0" };
        content.push_str(availability);
        content.push('\t');
        content.push_str(&encode_recent_repository_path(&recent.path));
        content.push('\n');
    }

    fs::write(path, content)
}

fn parse_recent_repository_line(line: &str) -> Option<RecentRepository> {
    let (availability, encoded_path) = line.split_once('\t')?;
    if encoded_path.is_empty() {
        return None;
    }

    let path = decode_recent_repository_path(encoded_path);
    match availability {
        "1" => Some(RecentRepository::available(path)),
        "0" => Some(RecentRepository::unavailable(path)),
        _ => None,
    }
}

fn encode_recent_repository_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}

fn decode_recent_repository_path(path: &str) -> PathBuf {
    PathBuf::from(
        path.replace("%0D", "\r")
            .replace("%0A", "\n")
            .replace("%09", "\t")
            .replace("%25", "%"),
    )
}

fn load_recent_repositories_or_default(path: Option<&Path>) -> Vec<RecentRepository> {
    path.and_then(|path| load_recent_repositories(path).ok())
        .unwrap_or_default()
}

#[cfg(test)]
fn default_recent_repository_store_path() -> Option<PathBuf> {
    None
}

#[cfg(all(not(test), target_os = "macos"))]
fn default_recent_repository_store_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Greviewer")
            .join("recent-repositories")
    })
}

#[cfg(all(not(test), not(target_os = "macos")))]
fn default_recent_repository_store_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config_home| config_home.join("greviewer").join("recent-repositories"))
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

impl App {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let recent_repository_store_path = default_recent_repository_store_path();
        let recent_repositories =
            load_recent_repositories_or_default(recent_repository_store_path.as_deref());

        Self::new_with_picker_recent_and_store_path(
            window,
            cx,
            Box::new(GpuiPathPicker),
            recent_repositories,
            recent_repository_store_path,
        )
    }

    pub fn new_with_picker(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
    ) -> Self {
        Self::new_with_picker_and_recent(window, cx, path_picker, Vec::new())
    }

    pub fn new_with_recent_repositories(
        window: &mut Window,
        cx: &mut Context<Self>,
        recent_repositories: Vec<RecentRepository>,
    ) -> Self {
        Self::new_with_picker_and_recent(window, cx, Box::new(GpuiPathPicker), recent_repositories)
    }

    #[cfg(test)]
    fn new_with_recent_repository_store_path(
        window: &mut Window,
        cx: &mut Context<Self>,
        recent_repository_store_path: PathBuf,
    ) -> Self {
        let recent_repositories =
            load_recent_repositories_or_default(Some(&recent_repository_store_path));

        Self::new_with_picker_recent_and_store_path(
            window,
            cx,
            Box::new(GpuiPathPicker),
            recent_repositories,
            Some(recent_repository_store_path),
        )
    }

    fn new_with_picker_and_recent(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
        recent_repositories: Vec<RecentRepository>,
    ) -> Self {
        Self::new_with_picker_recent_and_store_path(
            window,
            cx,
            path_picker,
            recent_repositories,
            None,
        )
    }

    fn new_with_picker_recent_and_store_path(
        window: &mut Window,
        cx: &mut Context<Self>,
        path_picker: Box<dyn PathPicker>,
        recent_repositories: Vec<RecentRepository>,
        recent_repository_store_path: Option<PathBuf>,
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
            file_list_mode: FileListMode::Changed,
            recent_repositories,
            collapsed_file_tree_paths: BTreeSet::new(),
            notifications,
            path_picker,
            recent_repository_store_path,
            file_diff_scroll: FileDiffScroll::new(),
            commit_history_scroll: ScrollHandle::new(),
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
            Ok(repo) => self.apply_open_repository(repo, cx),
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
            Ok(repo) => self.apply_open_repository(repo, cx),
            Err(err) => {
                self.mark_recent_repository_unavailable(&path);
                self.persist_recent_repositories();
                self.push_open_failed(err.to_string(), window, cx);
            }
        }
    }

    fn apply_open_repository(&mut self, repo: repo::OpenRepository, cx: &mut Context<Self>) {
        let recent_path = repo.path.clone();

        self.mode = Mode::RepoOpen { repo };
        self.selection = Selection::None;
        self.review_screen = ReviewScreen::Graph;
        self.selected_changed_file_path = None;
        self.file_list_mode = FileListMode::Changed;
        self.collapsed_file_tree_paths.clear();
        self.record_recent_repository(recent_path);
        self.persist_recent_repositories();
        self.file_diff_scroll.reset();
        self.commit_history_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn record_recent_repository(&mut self, path: PathBuf) {
        self.recent_repositories
            .retain(|recent| recent.path != path);
        self.recent_repositories
            .insert(0, RecentRepository::available(path));
        self.recent_repositories.truncate(MAX_RECENT_REPOSITORIES);
    }

    fn mark_recent_repository_unavailable(&mut self, path: &PathBuf) {
        if let Some(recent) = self
            .recent_repositories
            .iter_mut()
            .find(|recent| recent.path == *path)
        {
            recent.available = false;
        }
    }

    fn persist_recent_repositories(&self) {
        if let Some(path) = &self.recent_repository_store_path {
            let _ = save_recent_repositories(path, &self.recent_repositories);
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

    fn set_file_list_mode(&mut self, mode: FileListMode, cx: &mut Context<Self>) {
        if self.file_list_mode == mode {
            return;
        }

        self.file_list_mode = mode;
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
        let recent_repositories = if self.recent_repositories.is_empty() {
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
                        self.recent_repositories
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
        let path = recent.path.clone();
        let display_path = path.display().to_string();
        let debug_selector = if recent.available {
            format!("recent-repository-row-{index}")
        } else {
            format!("unavailable-recent-repository-row-{index}")
        };
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
                app.open_recent_repository(path.clone(), window, cx);
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
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(0x5a2a2a))
                        .bg(rgb(0x241818))
                        .text_color(rgb(0xfca5a5))
                        .text_size(px(11.))
                        .child("Unavailable"),
                )
            })
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
            let graph_commits = repo
                .commits
                .iter()
                .map(|commit| graph::GraphCommit {
                    sha: commit.sha.clone(),
                    parent_shas: commit.parent_shas.clone(),
                })
                .collect::<Vec<_>>();
            let graph_rows = graph::layout_graph(&graph_commits);
            let max_graph_lanes = graph_rows
                .iter()
                .map(|row| row.lane_count)
                .max()
                .unwrap_or(1);

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
                .children(
                    repo.commits
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
                    .child(self.render_file_list(entries, cx))
                    .child(self.render_file_detail(repo, changeset, selected_path))
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
            .child(header)
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
                .flex_1()
                .min_h_0()
                .id("changed-files-scroll")
                .debug_selector(|| "changed-files-scroll".to_string())
                .overflow_y_scroll()
                .children(
                    rows.iter()
                        .enumerate()
                        .map(|(index, row)| self.render_file_tree_row(index, row, cx))
                        .collect::<Vec<_>>(),
                )
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .w(px(340.))
            .h_full()
            .min_h_0()
            .id("changed-files")
            .border_1()
            .border_color(rgb(0x242424))
            .child(self.render_file_list_mode_toggle(cx))
            .child(list_content)
    }

    fn file_tree_rows(&self, entries: Vec<FileListEntry>) -> Vec<FileTreeRow> {
        let mut root = FileTreeBranch::default();

        for entry in entries {
            insert_file_tree_entry(&mut root, entry);
        }

        let mut rows = Vec::new();
        append_file_tree_rows(&root, 0, "", &self.collapsed_file_tree_paths, &mut rows);
        rows
    }

    fn render_file_list_mode_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_1()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(rgb(0x242424))
            .child(self.render_file_list_mode_button(
                FileListMode::Changed,
                "Changed",
                "file-list-mode-changed",
                cx,
            ))
            .child(self.render_file_list_mode_button(
                FileListMode::All,
                "All files",
                "file-list-mode-all",
                cx,
            ))
    }

    fn render_file_list_mode_button(
        &self,
        mode: FileListMode,
        label: &'static str,
        selector: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.file_list_mode == mode;
        let border_color = if active { rgb(0x3b82f6) } else { rgb(0x343434) };
        let background = if active { rgb(0x1d283a) } else { rgb(0x171717) };
        let text_color = if active { rgb(0xdbeafe) } else { rgb(0x999999) };

        div()
            .px_2()
            .py_1()
            .border_1()
            .border_color(border_color)
            .bg(background)
            .text_color(text_color)
            .text_size(px(12.))
            .cursor_pointer()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.set_file_list_mode(mode, cx);
            }))
            .child(label)
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
        let disclosure = if collapsed { ">" } else { "v" };

        div()
            .flex()
            .items_center()
            .w_full()
            .gap_3()
            .px_4()
            .py_2()
            .bg(rgb(0x171717))
            .border_b_1()
            .border_color(rgb(0x242424))
            .cursor_pointer()
            .id(("file-tree-folder", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.toggle_file_tree_folder(path.clone(), cx);
            }))
            .child(depth_spacer(depth))
            .child(
                div()
                    .w(px(16.))
                    .text_color(rgb(0x999999))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .child(disclosure),
            )
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(14.))
                    .font_family("monospace")
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
            .child(depth_spacer(depth))
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
                            .child(display_name.to_string()),
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
        let border_color = if selected {
            rgb(0x3b82f6)
        } else {
            rgb(0x242424)
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
            .gap_3()
            .px_4()
            .py_2()
            .bg(row_bg)
            .border_b_1()
            .border_color(border_color)
            .cursor_pointer()
            .id(("unchanged-file-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, _window, cx| {
                app.select_changed_file(path.clone(), cx);
            }))
            .child(depth_spacer(depth))
            .child(div().w(px(72.)))
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(0xe6e6e6))
                    .text_size(px(14.))
                    .font_family("monospace")
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
        graph_row: &graph::GraphRow,
        max_graph_lanes: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let secondary = format!("{} · {}", commit.author, commit.authored_date);
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
            .child(render_commit_ref_labels(index, commit))
            .child(render_commit_graph_gutter(
                index,
                graph_row,
                max_graph_lanes,
            ))
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

const COMMIT_ROW_HEIGHT: f32 = 64.;

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
    max_lanes: usize,
) -> impl IntoElement {
    let lane_count = max_lanes.max(1);
    let debug_selector = format!("commit-graph-gutter-{row_index}");

    div()
        .flex()
        .items_center()
        .w(px(
            (lane_count as f32 * COMMIT_GRAPH_LANE_WIDTH).max(COMMIT_GRAPH_LANE_WIDTH * 2.)
        ))
        .font_family("monospace")
        .id(("commit-graph-gutter", row_index))
        .debug_selector(move || debug_selector.clone())
        .children(
            (0..lane_count)
                .map(|lane| render_commit_graph_lane(row_index, lane, row))
                .collect::<Vec<_>>(),
        )
}

const COMMIT_GRAPH_LANE_WIDTH: f32 = 22.;
const COMMIT_GRAPH_LANE_HEIGHT: f32 = COMMIT_ROW_HEIGHT;
const COMMIT_GRAPH_MIDDLE_HEIGHT: f32 = 10.;
const COMMIT_GRAPH_VERTICAL_HEIGHT: f32 =
    (COMMIT_GRAPH_LANE_HEIGHT - COMMIT_GRAPH_MIDDLE_HEIGHT) / 2.;
const COMMIT_GRAPH_LINE_WIDTH: f32 = 2.;
const COMMIT_GRAPH_DOT_SIZE: f32 = 8.;

fn commit_graph_line_x() -> f32 {
    (COMMIT_GRAPH_LANE_WIDTH - COMMIT_GRAPH_LINE_WIDTH) / 2.
}

fn commit_graph_right_line_x() -> f32 {
    commit_graph_line_x() + COMMIT_GRAPH_LINE_WIDTH
}

fn commit_graph_right_line_width() -> f32 {
    COMMIT_GRAPH_LANE_WIDTH - commit_graph_right_line_x()
}

fn commit_graph_dot_side_line_width() -> f32 {
    (COMMIT_GRAPH_LANE_WIDTH - COMMIT_GRAPH_DOT_SIZE) / 2.
}

fn render_commit_graph_lane(row_index: usize, lane: usize, row: &graph::GraphRow) -> gpui::Div {
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
    position: &'static str,
    visible: bool,
    color: gpui::Rgba,
) -> gpui::Div {
    let selector = format!("commit-graph-vertical-{row_index}-{lane}-{position}");
    let segment = div()
        .w(px(COMMIT_GRAPH_LINE_WIDTH))
        .h(px(COMMIT_GRAPH_VERTICAL_HEIGHT))
        .when(visible, |segment| {
            segment.bg(color).debug_selector(move || selector.clone())
        });

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_VERTICAL_HEIGHT))
        .child(segment)
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
    let non_commit_connector_selector = connector_selector.clone();
    let commit_connector_selector = connector_selector;

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_MIDDLE_HEIGHT))
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
                        .h(px(COMMIT_GRAPH_MIDDLE_HEIGHT))
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
            let has_right_connector = has_connector && lane < max_lane;
            let left_connector_color = commit_graph_connector_on_side(row, lane, false)
                .map(|connector| commit_graph_connector_color(row, connector))
                .unwrap_or(color);
            let right_connector_color = commit_graph_connector_on_side(row, lane, true)
                .map(|connector| commit_graph_connector_color(row, connector))
                .unwrap_or(color);

            middle.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(COMMIT_GRAPH_MIDDLE_HEIGHT))
                    .child(
                        div()
                            .w(px(commit_graph_dot_side_line_width()))
                            .h(px(COMMIT_GRAPH_LINE_WIDTH))
                            .when(has_left_connector, |line| line.bg(left_connector_color)),
                    )
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
                            .when(has_right_connector, |line| {
                                line.bg(right_connector_color)
                                    .debug_selector(move || commit_connector_selector.clone())
                            }),
                    ),
            )
        })
}

fn commit_graph_connector_for_lane(
    row: &graph::GraphRow,
    lane: usize,
) -> Option<graph::GraphConnector> {
    commit_graph_target_connector_for_lane(row, lane)
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
    row.connectors
        .iter()
        .copied()
        .filter(|connector| {
            (right && connector.to_lane > lane) || (!right && connector.to_lane < lane)
        })
        .min_by_key(|connector| connector.to_lane.abs_diff(lane))
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

fn render_commit_graph_non_commit_connector(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    connector_selector: String,
) -> gpui::Div {
    let target_connector = commit_graph_target_connector_for_lane(row, lane);
    let connector = commit_graph_connector_for_lane(row, lane);
    let has_incoming = row.incoming_lanes.contains(&lane);
    let has_outgoing = row.outgoing_lanes.contains(&lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let color = connector
        .map(|connector| commit_graph_connector_color(row, connector))
        .unwrap_or(lane_color);
    let (left_visible, right_visible) = match target_connector.map(|connector| connector.kind) {
        Some(graph::GraphConnectorKind::BranchOut) => (true, false),
        Some(graph::GraphConnectorKind::MergeIn) => (false, true),
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
    let elbow_top = if has_incoming { 0. } else { 4. };
    let elbow_bottom = if has_outgoing { 10. } else { 6. };
    let elbow_height = elbow_bottom - elbow_top;
    let middle_vertical_selector = format!("commit-graph-middle-vertical-{row_index}-{lane}");
    let has_middle_vertical = has_incoming || has_outgoing;
    let center_fill_selector =
        format!("commit-graph-spanning-horizontal-center-{row_index}-{lane}");
    let fill_spanning_center = commit_graph_spanning_connector_requires_center_fill(row, lane);

    let mut connector_shape = div()
        .relative()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_MIDDLE_HEIGHT))
        .child(
            div()
                .absolute()
                .left(px(0.))
                .top(px(4.))
                .w(px(commit_graph_line_x()))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .when(left_visible, |line| {
                    line.bg(color).when_some(
                        target_connector.and_then(|connector| {
                            (connector.kind == graph::GraphConnectorKind::BranchOut).then(|| {
                                format!("commit-graph-branch-out-horizontal-{row_index}-{lane}")
                            })
                        }),
                        |line, selector| line.debug_selector(move || selector.clone()),
                    )
                }),
        )
        .child(
            div()
                .absolute()
                .left(px(commit_graph_right_line_x()))
                .top(px(4.))
                .w(px(commit_graph_right_line_width()))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .when(right_visible, |line| {
                    line.bg(color).when_some(
                        target_connector.and_then(|connector| {
                            (connector.kind == graph::GraphConnectorKind::MergeIn).then(|| {
                                format!("commit-graph-merge-in-horizontal-{row_index}-{lane}")
                            })
                        }),
                        |line, selector| line.debug_selector(move || selector.clone()),
                    )
                }),
        );

    if fill_spanning_center {
        connector_shape = connector_shape.child(
            div()
                .absolute()
                .left(px(commit_graph_line_x()))
                .top(px(4.))
                .w(px(COMMIT_GRAPH_LINE_WIDTH))
                .h(px(COMMIT_GRAPH_LINE_WIDTH))
                .bg(color)
                .debug_selector(move || center_fill_selector.clone()),
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
                .bg(if has_middle_vertical {
                    lane_color
                } else {
                    color
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
                .bg(lane_color)
                .debug_selector(move || middle_vertical_selector.clone()),
        );
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_MIDDLE_HEIGHT))
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
            Mode::NoRepo => self.render_no_repo(cx).into_any_element(),
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

fn append_file_tree_rows(
    branch: &FileTreeBranch,
    depth: usize,
    prefix: &str,
    collapsed_paths: &BTreeSet<String>,
    rows: &mut Vec<FileTreeRow>,
) {
    for (name, child) in &branch.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let collapsed = collapsed_paths.contains(&path);

        rows.push(FileTreeRow::Folder {
            name: name.clone(),
            path: path.clone(),
            depth,
            collapsed,
        });

        if !collapsed {
            append_file_tree_rows(child, depth + 1, &path, collapsed_paths, rows);
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

fn depth_spacer(depth: usize) -> gpui::Div {
    div().w(px(depth as f32 * 16.))
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

#[cfg(test)]
mod tests {
    use super::{
        commit_graph_connector_color_lane, commit_graph_connector_for_lane,
        commit_graph_spanning_connector_requires_center_fill, commit_row_separator_width,
        debug_ref_label_fragment, load_recent_repositories, save_recent_repositories,
        side_by_side_diff_rows, single_side_diff_rows, App, CloseChangeset, DiffLineStatus,
        FileListMode, FileTreeRow, Mode, OpenChangeset, OpenFailed, RecentRepository, ReviewScreen,
        Selection,
    };
    use crate::graph::{self, GraphConnectorKind};
    use crate::repo::{ChangeKind, DiffSide, INITIAL_COMMIT_LIMIT};
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

    fn graph_commit(sha: &str, parent_shas: &[&str]) -> graph::GraphCommit {
        graph::GraphCommit {
            sha: sha.to_string(),
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

    #[test]
    fn recent_repository_store_round_trips_paths_and_availability() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store_path = dir.path().join("state").join("recent-repositories");
        let recent_repositories = vec![
            RecentRepository::available(dir.path().join("repo-one")),
            RecentRepository::unavailable(dir.path().join("repo%\t\n\r-two")),
        ];

        save_recent_repositories(&store_path, &recent_repositories)
            .expect("save recent repositories");

        assert_eq!(
            load_recent_repositories(&store_path).expect("load recent repositories"),
            recent_repositories,
        );
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
        let window = cx.add_window(App::new);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(first_path.clone(), window, cx);
                app.open_repository_at(second_path.clone(), window, cx);
                assert_eq!(
                    app.recent_repositories,
                    vec![
                        RecentRepository::available(second_path.clone()),
                        RecentRepository::available(first_path.clone()),
                    ],
                );

                app.open_repository_at(first_path.clone(), window, cx);
                assert_eq!(
                    app.recent_repositories,
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
        let dir = tempfile::tempdir().expect("create tempdir");
        let store_path = dir.path().join("recent-repositories");
        let recent_repositories = vec![
            RecentRepository::available(dir.path().join("repo-one")),
            RecentRepository::unavailable(dir.path().join("repo-two")),
        ];
        save_recent_repositories(&store_path, &recent_repositories)
            .expect("seed recent repository store");

        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repository_store_path(window, cx, store_path.clone())
        });

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.recent_repositories, recent_repositories);
            })
            .expect("read loaded recent repositories");
    }

    #[gpui::test]
    async fn opening_repository_persists_recent_repositories_to_disk(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().canonicalize().expect("canonical repo path");
        let state_dir = tempfile::tempdir().expect("create tempdir");
        let store_path = state_dir.path().join("recent-repositories");
        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repository_store_path(window, cx, store_path.clone())
        });

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
            })
            .expect("open repository");

        assert_eq!(
            load_recent_repositories(&store_path).expect("load recent repository store"),
            vec![RecentRepository::available(path)],
        );
    }

    #[gpui::test]
    async fn clicking_recent_repository_opens_it(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().canonicalize().expect("canonical repo path");
        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repositories(
                window,
                cx,
                vec![RecentRepository::available(path.clone())],
            )
        });

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("recent-repository-row-0")
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
        let missing_path = dir.path().join("missing-repo");
        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repositories(
                window,
                cx,
                vec![RecentRepository::available(missing_path.clone())],
            )
        });

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("recent-repository-row-0")
            .expect("recent repository row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, cx| {
                assert!(matches!(app.mode, Mode::NoRepo));
                assert_eq!(
                    app.recent_repositories,
                    vec![RecentRepository::unavailable(missing_path.clone())],
                );
                assert_eq!(app.notification_count(cx), 1);
            })
            .expect("read unavailable recent repository");

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("unavailable-recent-repository-row-0")
            .expect("unavailable recent repository row debug bounds");
    }

    #[gpui::test]
    async fn failed_recent_repository_activation_persists_unavailable_state(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);

        let dir = tempfile::tempdir().expect("create tempdir");
        let missing_path = dir.path().join("missing-repo");
        let store_path = dir.path().join("recent-repositories");
        save_recent_repositories(
            &store_path,
            &[RecentRepository::available(missing_path.clone())],
        )
        .expect("seed recent repository store");
        let window = cx.add_window(|window, cx| {
            App::new_with_recent_repository_store_path(window, cx, store_path.clone())
        });

        window
            .update(cx, |app, window, cx| {
                app.open_recent_repository(missing_path.clone(), window, cx);
            })
            .expect("activate missing recent repository");

        assert_eq!(
            load_recent_repositories(&store_path).expect("load recent repository store"),
            vec![RecentRepository::unavailable(missing_path)],
        );
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
    async fn selecting_merge_to_second_parent_uses_that_ancestry_path_and_rolls_up_merge_files(
        cx: &mut TestAppContext,
    ) {
        let (dir, merge_sha, main_sha, side_sha, root_sha) = init_repo_with_merge_range();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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

    #[gpui::test]
    async fn commit_graph_renders_merge_lanes(cx: &mut TestAppContext) {
        let (dir, _left_sha, _right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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
        visual
            .debug_bounds("commit-graph-lane-0-1")
            .expect("merge commit second parent lane debug bounds");
        visual
            .debug_bounds("commit-graph-connector-0-1")
            .expect("merge commit second parent connector debug bounds");
        visual
            .debug_bounds("commit-graph-branch-out-0-1")
            .expect("merge commit branch-out connector debug bounds");
        let branch_out_elbow_bounds = visual
            .debug_bounds("commit-graph-branch-out-elbow-0-1")
            .expect("merge commit branch-out elbow debug bounds");
        let branch_out_middle_vertical_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-0-1")
            .expect("merge commit branch-out middle vertical debug bounds");
        let branch_out_horizontal_bounds = visual
            .debug_bounds("commit-graph-branch-out-horizontal-0-1")
            .expect("merge commit branch-out horizontal debug bounds");
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
        assert_eq!(
            branch_out_elbow_bounds.origin.y + branch_out_elbow_bounds.size.height,
            branch_out_vertical_bounds.origin.y,
            "branch-out elbow should connect to the outgoing lane",
        );
        visual
            .debug_bounds("commit-graph-vertical-0-1-bottom")
            .expect("merge commit second parent outgoing vertical debug bounds");
        visual
            .debug_bounds("commit-graph-vertical-1-1-top")
            .expect("continued second lane incoming vertical debug bounds");
        let continued_lane_top_bounds = visual
            .debug_bounds("commit-graph-vertical-1-1-top")
            .expect("continued second lane incoming vertical debug bounds");
        let continued_lane_middle_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-1-1")
            .expect("continued second lane middle vertical debug bounds");
        let continued_lane_bottom_bounds = visual
            .debug_bounds("commit-graph-vertical-1-1-bottom")
            .expect("continued second lane outgoing vertical debug bounds");
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
        let merge_in_vertical_bounds = visual
            .debug_bounds("commit-graph-vertical-2-0-bottom")
            .expect("right branch merge-in outgoing vertical debug bounds");
        let merge_in_middle_vertical_bounds = visual
            .debug_bounds("commit-graph-middle-vertical-2-0")
            .expect("right branch merge-in middle trunk vertical debug bounds");
        let merge_in_horizontal_bounds = visual
            .debug_bounds("commit-graph-merge-in-horizontal-2-0")
            .expect("right branch merge-in horizontal debug bounds");
        assert_eq!(
            merge_in_horizontal_bounds.origin.x,
            merge_in_middle_vertical_bounds.origin.x + merge_in_middle_vertical_bounds.size.width,
            "merge-in horizontal should start at the trunk lane",
        );
        assert_eq!(
            merge_in_elbow_bounds.origin.x, merge_in_vertical_bounds.origin.x,
            "merge-in elbow should align with the parent lane",
        );
        assert_eq!(
            merge_in_elbow_bounds.origin.y + merge_in_elbow_bounds.size.height,
            merge_in_vertical_bounds.origin.y,
            "merge-in elbow should connect to the parent lane",
        );
    }

    #[gpui::test]
    async fn commit_graph_vertical_segments_connect_between_rows(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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

        assert_eq!(
            first_row_bottom.origin.y + first_row_bottom.size.height,
            second_row_top.origin.y,
            "commit graph vertical segments should connect across adjacent rows; first row: {first_row:?}, second row: {second_row:?}, first bottom: {first_row_bottom:?}, second top: {second_row_top:?}",
        );
    }

    #[gpui::test]
    async fn commit_rows_render_head_and_branch_labels(cx: &mut TestAppContext) {
        let (dir, _left_sha, _right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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
        let window = cx.add_window(App::new);

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
            label_bounds.origin.x + label_bounds.size.width <= graph_bounds.origin.x,
            "branch label should end before the graph gutter starts; label: {label_bounds:?}, graph: {graph_bounds:?}"
        );
    }

    #[gpui::test]
    async fn scrolling_commit_history_loads_older_commits(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, shas) = init_repo_with_linear_history(INITIAL_COMMIT_LIMIT + 2);
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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
    async fn opening_empty_rollup_changeset_shows_empty_state(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_empty_rollup_range();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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
        let window = cx.add_window(App::new);

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
            .debug_bounds("file-list-mode-all")
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
    async fn selecting_changed_file_in_all_files_mode_still_renders_side_by_side_diff(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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
            .debug_bounds("file-list-mode-all")
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
        let window = cx.add_window(App::new);

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
    async fn collapsing_file_tree_folder_persists_across_file_list_mode_toggle(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = cx.add_window(App::new);

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
        visual.simulate_click(folder_bounds.center(), Modifiers::none());

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
            .debug_bounds("file-list-mode-all")
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
