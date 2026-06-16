//! Top-level application entity and root view.

mod branch_sidebar;
mod commit_graph;
mod diff_view;
mod file_tree;
pub mod menu;
pub mod path_picker;
#[cfg(test)]
mod test_support;
mod title_bar;

use self::branch_sidebar::*;
use self::commit_graph::*;
use self::diff_view::*;
use self::file_tree::*;
// Re-exported for `workspace::tab_bar`, which renders change-kind colors too.
pub(crate) use self::file_tree::change_kind_text;
pub use menu::{
    bind_app_keys, build_app_menus, open_repository_key_binding, quit_application_key_binding,
    MenuSnapshot, GREVIEWER_MENU_LABEL, OPEN_REPOSITORY_KEYSTROKE, OPEN_REPOSITORY_MENU_LABEL,
    QUIT_APPLICATION_KEYSTROKE,
};
pub use path_picker::{repository_prompt_options, GpuiPathPicker, PathPicker, PathPickerOutcome};

use gpui::prelude::FluentBuilder;
use gpui::{
    actions, canvas, div, pattern_slash, point, px, rgb, rgba, uniform_list, AnyElement,
    AppContext, Background, ClickEvent, Context, Entity, EventEmitter, FocusHandle, HighlightStyle,
    Hsla, InteractiveElement, IntoElement, Modifiers, ParentElement, PathBuilder, Pixels, Render,
    ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement, Styled, StyledText, TextStyle,
    UniformListScrollHandle, Window,
};
use gpui_component::notification::{Notification, NotificationList};
use gpui_component::resizable::{h_resizable, resizable_panel, ResizableState};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::tooltip::Tooltip;
use gpui_component::Icon;
use similar::{DiffTag, TextDiff};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::icons::LucideIcon;
use crate::settings::{self, RecentRepository, Settings, MAX_RECENT_REPOSITORIES};
use crate::workspace::FileDiffItem;
use crate::{diff_highlight, graph, repo};

actions!(
    app,
    [
        OpenRepository,
        OpenChangeset,
        CloseChangeset,
        QuitApplication,
        CloseActiveTab,
        ActivateNextTab,
        ActivatePreviousTab,
        SplitPaneLeft,
        SplitPaneRight,
        SplitPaneUp,
        SplitPaneDown,
        CloseActivePane
    ]
);

pub(crate) const FILE_TREE_FONT_FAMILY: &str = "BerkeleyMono Nerd Font";
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
const FILE_TREE_DIFF_STAT_WIDTH: f32 = 68.;
const FILE_TREE_STAT_GUTTER_WIDTH: f32 = 84.; // diff-stat width + horizontal cell padding
const BRANCH_SIDEBAR_DEFAULT_WIDTH: f32 = 240.;

pub struct App {
    pub mode: Mode,
    pub selection: Selection,
    pub review_screen: ReviewScreen,
    /// Open diff tabs. Source of truth for what the detail area shows.
    pub workspace: crate::workspace::Workspace,
    /// Last file row the user clicked. Drives the tree highlight only; tab
    /// activation deliberately does not move it (spec: tree is click-driven).
    pub file_tree_highlight_path: Option<String>,
    /// Scroll handles per pane (tab strip + diff sides), created on demand.
    /// RefCell because render paths take `&self`; ScrollHandle clones share
    /// their underlying state, so handing out clones is safe.
    pane_scrolls: RefCell<HashMap<crate::workspace::PaneId, PaneScrollState>>,
    /// Computed diff rows for the changed-file detail view, keyed by file path
    /// plus the commit/base shas they were derived from. `render_changed_file_detail`
    /// runs on every App render; without this cache it would re-read the file from
    /// git and recompute the line diff each time. Cleared whenever the changeset
    /// selection changes (see `open_changeset`/`apply_open_repository`), so entries
    /// never outlive the changeset they describe. `RefCell` because render paths
    /// take `&self`; the `Rc` lets a cache hit hand back the rows without cloning.
    diff_row_cache: RefCell<HashMap<DiffCacheKey, Rc<PreparedFileDiff>>>,
    /// While a tab drag hovers a pane's edge zone, the pane and the split
    /// direction its half-highlight previews. None when no edge is hovered.
    pub(crate) tab_drop_zone: Option<(crate::workspace::PaneId, crate::workspace::SplitDirection)>,
    pub file_list_mode: FileListMode,
    pub settings: Settings,
    collapsed_file_tree_paths: BTreeSet<String>,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    settings_store_path: Option<PathBuf>,
    commit_history_scroll: ScrollHandle,
    file_tree_scroll: ScrollHandle,
    /// Horizontal scroll handle for the path pane only; the stat gutter stays
    /// fixed while this scrolls.
    file_tree_hscroll: ScrollHandle,
    /// True while the cursor is anywhere over the file-tree panel; gates the
    /// hover-revealed scrollbar overlay.
    file_tree_hovered: bool,
    changeset_resizable: Entity<ResizableState>,
    graph_resizable: Entity<ResizableState>,
    branch_sidebar_scroll: ScrollHandle,
    /// True while the cursor is anywhere over the branch sidebar; gates its
    /// hover-revealed scrollbar overlay.
    branch_sidebar_hovered: bool,
    /// Branch keys (`heads/{name}` / `remotes/{name}`, see [`branch_key`])
    /// the user has toggled off in the sidebar. Session-only: cleared
    /// whenever a repository is opened. The checked-out branch is never in
    /// this set (its row renders no toggle).
    hidden_branches: BTreeSet<String>,
    /// Sidebar folder keys (e.g. `heads/features`) the user has collapsed.
    /// Session-only: cleared
    /// whenever a repository is opened. Folders default to expanded, so an
    /// empty set means every folder shows its contents.
    collapsed_branch_folders: BTreeSet<String>,
    /// Sidebar row index the cursor is currently over, if any. Gates the
    /// hover-revealed visibility toggle on visible branches.
    hovered_branch_row: Option<usize>,
    focus_handle: FocusHandle,
    /// Whether the title-bar context popover (the diff "switcher") is open.
    context_popover_open: bool,
    /// Whether the title-bar repo switcher (sibling-repository list) is open.
    repo_switcher_open: bool,
}

#[derive(Clone)]
pub(crate) struct FileDiffScroll {
    old: UniformListScrollHandle,
    new: UniformListScrollHandle,
    side_by_side: UniformListScrollHandle,
}

/// One pane's scroll handles: the tab strip plus the diff content sides.
#[derive(Clone)]
pub(crate) struct PaneScrollState {
    pub(crate) tab_bar: ScrollHandle,
    pub(crate) diff: FileDiffScroll,
}

impl PaneScrollState {
    fn new() -> Self {
        Self {
            tab_bar: ScrollHandle::new(),
            diff: FileDiffScroll::new(),
        }
    }
}

impl FileDiffScroll {
    fn new() -> Self {
        Self {
            old: UniformListScrollHandle::new(),
            new: UniformListScrollHandle::new(),
            side_by_side: UniformListScrollHandle::new(),
        }
    }

    fn handle_for(&self, side: repo::DiffSide) -> &UniformListScrollHandle {
        match side {
            repo::DiffSide::Old => &self.old,
            repo::DiffSide::New => &self.new,
        }
    }

    fn reset(&self) {
        let origin = point(px(0.), px(0.));
        self.old.0.borrow().base_handle.set_offset(origin);
        self.new.0.borrow().base_handle.set_offset(origin);
        self.side_by_side.0.borrow().base_handle.set_offset(origin);
    }

    #[cfg(test)]
    fn side_by_side_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.side_by_side.0.borrow().base_handle.offset()
    }

    #[cfg(test)]
    fn side_by_side_max_offset(&self) -> gpui::Size<gpui::Pixels> {
        self.side_by_side.0.borrow().base_handle.max_offset()
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

/// A folder's aggregate graph-visibility, derived from its descendant
/// branches' membership in `hidden_branches`. The HEAD branch cannot be
/// hidden and never counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderVisibility {
    /// No hideable descendant is hidden (or there are none).
    Visible,
    /// Every hideable descendant is hidden, and there is at least one.
    Hidden,
    /// Some hideable descendants are hidden, some visible.
    Mixed,
}

/// Render model for the graph branch sidebar: branches grouped under
/// collapsible folders derived from `/`-separated name segments. Branch rows
/// carry the full `Branch` — selection, hiding, and debug selectors all
/// key on the full name; only `display_name` is shortened.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BranchTreeRow {
    Section(BranchSectionRow),
    Folder(BranchFolderRow),
    Branch(BranchRow),
}

/// Non-interactive header introducing the Local or Remote half of the
/// sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchSectionRow {
    title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchFolderRow {
    /// Final path segment, e.g. "alice" for `team/alice`.
    name: String,
    /// Full prefix path, e.g. "team/alice". Keys collapse state.
    path: String,
    depth: usize,
    collapsed: bool,
    visibility: FolderVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchRow {
    branch: repo::Branch,
    /// Final name segment, e.g. "some-feature" for `features/some-feature`.
    display_name: String,
    depth: usize,
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
        let graph_resizable = cx.new(|_| ResizableState::default());
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
            workspace: crate::workspace::Workspace::new(),
            file_tree_highlight_path: None,
            pane_scrolls: RefCell::new(HashMap::new()),
            diff_row_cache: RefCell::new(HashMap::new()),
            tab_drop_zone: None,
            file_list_mode: FileListMode::Changed,
            settings,
            collapsed_file_tree_paths: BTreeSet::new(),
            notifications,
            path_picker,
            settings_store_path,
            commit_history_scroll: ScrollHandle::new(),
            file_tree_scroll: ScrollHandle::new(),
            file_tree_hscroll: ScrollHandle::new(),
            file_tree_hovered: false,
            changeset_resizable,
            graph_resizable,
            branch_sidebar_scroll: ScrollHandle::new(),
            branch_sidebar_hovered: false,
            hidden_branches: BTreeSet::new(),
            collapsed_branch_folders: BTreeSet::new(),
            hovered_branch_row: None,
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
        self.workspace = crate::workspace::Workspace::new();
        self.pane_scrolls.borrow_mut().clear();
        self.diff_row_cache.borrow_mut().clear();
        self.file_tree_highlight_path = None;
        self.file_list_mode = FileListMode::Changed;
        self.collapsed_file_tree_paths.clear();
        self.record_recent_repository(recent_path);
        self.persist_settings();
        self.commit_history_scroll.set_offset(point(px(0.), px(0.)));
        self.file_tree_scroll.set_offset(point(px(0.), px(0.)));
        self.file_tree_hscroll.set_offset(point(px(0.), px(0.)));
        self.branch_sidebar_scroll.set_offset(point(px(0.), px(0.)));
        self.branch_sidebar_hovered = false;
        self.hidden_branches.clear();
        self.collapsed_branch_folders.clear();
        self.hovered_branch_row = None;
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

    pub(crate) fn select_single_commit(&mut self, sha: String, cx: &mut Context<Self>) {
        self.selection = match &self.selection {
            Selection::Single { sha: selected_sha } if selected_sha == &sha => Selection::None,
            _ => Selection::Single { sha },
        };
        cx.notify();
    }

    /// Flip a branch's graph visibility. Hiding may make the selected
    /// commit(s) invisible, in which case the selection is cleared. The HEAD
    /// branch never reaches this path: its sidebar row renders no toggle.
    pub(crate) fn toggle_branch_visibility(&mut self, name: String, cx: &mut Context<Self>) {
        if self.hidden_branches.remove(&name) {
            cx.notify();
            return;
        }
        self.hidden_branches.insert(name);
        self.clear_selection_if_hidden();
        cx.notify();
    }

    /// Collapse or expand a sidebar branch folder. Purely visual: removes the
    /// folder's descendant rows from the sidebar without touching graph
    /// visibility.
    pub(crate) fn toggle_branch_folder(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.collapsed_branch_folders.insert(path.clone()) {
            self.collapsed_branch_folders.remove(&path);
        }
        cx.notify();
    }

    /// Flip graph visibility for every branch under a sidebar folder as one
    /// batched change: if any hideable descendant is visible, hide them all;
    /// otherwise show them all. The HEAD branch cannot be hidden and is
    /// skipped, so a folder containing it hides everything else inside.
    pub(crate) fn toggle_folder_visibility(&mut self, path: &str, cx: &mut Context<Self>) {
        let Mode::RepoOpen { repo } = &self.mode else {
            return;
        };
        let prefix = format!("{path}/");
        let descendants = repo
            .branches
            .iter()
            .filter(|branch| !branch.is_head)
            .map(|branch| branch_key(&branch.name, &branch.kind))
            .filter(|key| key.starts_with(&prefix))
            .collect::<Vec<_>>();
        if descendants.is_empty() {
            return;
        }

        let any_visible = descendants
            .iter()
            .any(|name| !self.hidden_branches.contains(name));
        if any_visible {
            self.hidden_branches.extend(descendants);
            self.clear_selection_if_hidden();
        } else {
            for name in &descendants {
                self.hidden_branches.remove(name);
            }
        }
        cx.notify();
    }

    /// Reset the selection when hiding a branch removed any selected commit
    /// from the visible graph. No-ops outside `Mode::RepoOpen` or when the
    /// selection is already visible.
    fn clear_selection_if_hidden(&mut self) {
        let Mode::RepoOpen { repo } = &self.mode else {
            return;
        };
        let head_sha = repo
            .commits
            .iter()
            .find(|commit| commit.is_head)
            .map(|commit| commit.sha.as_str());
        let visible = visible_commit_shas(
            &repo.commits,
            &repo.branches,
            head_sha,
            &self.hidden_branches,
        );
        let selection_hidden = match &self.selection {
            Selection::None => false,
            Selection::Single { sha } => !visible.contains(sha),
            Selection::Range { shas, .. } => shas.iter().any(|sha| !visible.contains(sha)),
        };
        if selection_hidden {
            self.selection = Selection::None;
        }
    }

    /// Select a branch's tip commit and bring its row into view, paging in
    /// older history first when the tip has not been loaded yet. The revwalk
    /// behind `load_older_commits` pushes every local and remote branch tip,
    /// so paging always reaches the commit unless loading itself fails.
    fn focus_branch(&mut self, tip_sha: String, window: &mut Window, cx: &mut Context<Self>) {
        let (commit_index, commit_count) = loop {
            let (tip_index, can_load_more, loaded_count, visible_count) = match &self.mode {
                Mode::RepoOpen { repo } => {
                    let visible = visible_commits(repo, &self.hidden_branches);
                    (
                        visible.iter().position(|commit| commit.sha == tip_sha),
                        repo.has_more_commits,
                        repo.commits.len(),
                        visible.len(),
                    )
                }
                Mode::NoRepo => return,
            };
            if let Some(index) = tip_index {
                break (index, visible_count);
            }
            if !can_load_more {
                return;
            }

            self.load_older_commits(window, cx);

            let loaded_count_after = match &self.mode {
                Mode::RepoOpen { repo } => repo.commits.len(),
                Mode::NoRepo => return,
            };
            if loaded_count_after == loaded_count {
                // Paging failed; the error is already on the notification
                // list, so stop rather than loop forever.
                return;
            }
        };

        self.selection = Selection::Single { sha: tip_sha };
        self.scroll_commit_row_into_view(commit_index, commit_count);
        cx.notify();
    }

    /// Center the commit row at `index` in the history viewport, clamped to
    /// the scrollable range. Content height comes from the commit count
    /// rather than the scroll handle's max offset, which is stale when this
    /// runs in the same frame that paged in more commits.
    fn scroll_commit_row_into_view(&self, index: usize, commit_count: usize) {
        let viewport_height = self.commit_history_scroll.bounds().size.height;
        let row_top = px(index as f32 * COMMIT_ROW_HEIGHT);
        if viewport_height <= px(0.) {
            // Not laid out yet; pin the row to the top rather than centering
            // against a zero-height viewport.
            self.commit_history_scroll
                .set_offset(point(px(0.), -row_top));
            return;
        }
        let centered_top = row_top - (viewport_height - px(COMMIT_ROW_HEIGHT)) / 2.;
        let content_height = px(commit_count as f32 * COMMIT_ROW_HEIGHT);
        let max_offset = (content_height - viewport_height).max(px(0.));
        let target = (-centered_top).clamp(-max_offset, px(0.));
        self.commit_history_scroll.set_offset(point(px(0.), target));
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
                // Each changeset starts with the default single-pane layout;
                // splits last only while the changeset stays open.
                self.workspace = crate::workspace::Workspace::new();
                self.pane_scrolls.borrow_mut().clear();
                self.diff_row_cache.borrow_mut().clear();
                self.file_tree_highlight_path = None;
                self.file_tree_scroll.set_offset(point(px(0.), px(0.)));
                self.file_tree_hscroll.set_offset(point(px(0.), px(0.)));
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
        self.workspace.clear();
        self.file_tree_highlight_path = None;
        self.reset_pane_scrolls();
        self.diff_row_cache.borrow_mut().clear();
        cx.notify();
    }

    fn quit_application(&mut self, cx: &mut Context<Self>) {
        cx.emit(QuitRequested);
        cx.quit();
    }

    /// The scroll handles for `pane`, created on first use. The returned
    /// clone shares its underlying state with every other clone for the pane.
    pub(crate) fn pane_scroll(&self, pane: crate::workspace::PaneId) -> PaneScrollState {
        self.pane_scrolls
            .borrow_mut()
            .entry(pane)
            .or_insert_with(PaneScrollState::new)
            .clone()
    }

    fn reset_pane_scrolls(&self) {
        for state in self.pane_scrolls.borrow().values() {
            state.diff.reset();
            state.tab_bar.set_offset(point(px(0.), px(0.)));
        }
    }

    /// Drop scroll state for panes the workspace no longer has. Called after
    /// operations that can collapse a pane as a side effect (moving or
    /// splitting out a pane's last tab).
    fn prune_pane_scrolls(&self) {
        let live = self.workspace.pane_ids();
        self.pane_scrolls
            .borrow_mut()
            .retain(|pane, _| live.contains(pane));
    }

    fn open_file_preview(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_file(path, false, cx);
    }

    fn open_file_pinned(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_file(path, true, cx);
    }

    fn open_file(&mut self, path: String, pinned: bool, cx: &mut Context<Self>) {
        self.file_tree_highlight_path = Some(path.clone());
        let pane = self.workspace.active_pane();
        let item = Box::new(FileDiffItem::new(path));
        let content_changed = if pinned {
            self.workspace.open_pinned(item)
        } else {
            self.workspace.open_preview(item)
        };
        if content_changed {
            self.pane_scroll(pane).diff.reset();
        }
        if let Some(index) = self.workspace.active_index(pane) {
            self.pane_scroll(pane).tab_bar.scroll_to_item(index);
        }
        cx.notify();
    }

    pub(crate) fn activate_workspace_tab(
        &mut self,
        pane: crate::workspace::PaneId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.activate_tab(pane, index) {
            self.pane_scroll(pane).diff.reset();
        }
        self.pane_scroll(pane).tab_bar.scroll_to_item(index);
        cx.notify();
    }

    pub(crate) fn promote_workspace_tab(
        &mut self,
        pane: crate::workspace::PaneId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.workspace.promote_tab(pane, index);
        cx.notify();
    }

    pub(crate) fn close_workspace_tab(
        &mut self,
        pane: crate::workspace::PaneId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.close_tab(pane, index) {
            if self.workspace.pane_ids().contains(&pane) {
                self.pane_scroll(pane).diff.reset();
                if let Some(index) = self.workspace.active_index(pane) {
                    self.pane_scroll(pane).tab_bar.scroll_to_item(index);
                }
            } else {
                // Closing the pane's last tab collapsed the pane itself.
                self.prune_pane_scrolls();
            }
        }
        cx.notify();
    }

    /// Whether the review screen currently shows a changeset; the workspace
    /// keyboard actions are no-ops anywhere else.
    fn changeset_open(&self) -> bool {
        matches!(self.mode, Mode::RepoOpen { .. })
            && matches!(self.review_screen, ReviewScreen::Changeset { .. })
    }

    fn close_active_workspace_tab(&mut self, cx: &mut Context<Self>) {
        if !self.changeset_open() {
            return;
        }
        let pane = self.workspace.active_pane();
        if let Some(index) = self.workspace.active_index(pane) {
            self.close_workspace_tab(pane, index, cx);
        }
    }

    fn cycle_workspace_tab(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.changeset_open() {
            return;
        }
        let changed = if forward {
            self.workspace.activate_next_tab()
        } else {
            self.workspace.activate_previous_tab()
        };
        if changed {
            let pane = self.workspace.active_pane();
            self.pane_scroll(pane).diff.reset();
            if let Some(index) = self.workspace.active_index(pane) {
                self.pane_scroll(pane).tab_bar.scroll_to_item(index);
            }
            cx.notify();
        }
    }

    fn split_active_workspace_pane(
        &mut self,
        direction: crate::workspace::SplitDirection,
        cx: &mut Context<Self>,
    ) {
        if !self.changeset_open() {
            return;
        }
        let pane = self.workspace.active_pane();
        self.split_workspace_pane(pane, direction, cx);
    }

    fn close_active_workspace_pane(&mut self, cx: &mut Context<Self>) {
        if !self.changeset_open() {
            return;
        }
        let pane = self.workspace.active_pane();
        self.close_workspace_pane(pane, cx);
    }

    pub(crate) fn activate_workspace_pane(
        &mut self,
        pane: crate::workspace::PaneId,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.activate_pane(pane) {
            cx.notify();
        }
    }

    pub(crate) fn split_workspace_pane(
        &mut self,
        pane: crate::workspace::PaneId,
        direction: crate::workspace::SplitDirection,
        cx: &mut Context<Self>,
    ) {
        if self
            .workspace
            .split_with_active_item(pane, direction)
            .is_some()
        {
            cx.notify();
        }
    }

    pub(crate) fn close_workspace_pane(
        &mut self,
        pane: crate::workspace::PaneId,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.close_pane(pane) {
            self.pane_scrolls.borrow_mut().remove(&pane);
            cx.notify();
        }
    }

    pub(crate) fn set_tab_drop_zone(
        &mut self,
        zone: Option<(crate::workspace::PaneId, crate::workspace::SplitDirection)>,
        cx: &mut Context<Self>,
    ) {
        if self.tab_drop_zone != zone {
            self.tab_drop_zone = zone;
            cx.notify();
        }
    }

    pub(crate) fn move_workspace_tab(
        &mut self,
        from_pane: crate::workspace::PaneId,
        from_index: usize,
        to_pane: crate::workspace::PaneId,
        to_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.tab_drop_zone = None;
        if self
            .workspace
            .move_tab(from_pane, from_index, to_pane, to_index)
        {
            self.pane_scroll(to_pane).diff.reset();
            if let Some(index) = self.workspace.active_index(to_pane) {
                self.pane_scroll(to_pane).tab_bar.scroll_to_item(index);
            }
            self.prune_pane_scrolls();
        }
        cx.notify();
    }

    pub(crate) fn split_workspace_pane_with_tab(
        &mut self,
        target_pane: crate::workspace::PaneId,
        direction: crate::workspace::SplitDirection,
        from_pane: crate::workspace::PaneId,
        from_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.tab_drop_zone = None;
        if let Some(new_pane) =
            self.workspace
                .split_with_tab(target_pane, direction, from_pane, from_index)
        {
            self.pane_scroll(new_pane).diff.reset();
            self.prune_pane_scrolls();
        }
        cx.notify();
    }

    pub(crate) fn resize_workspace_divider(
        &mut self,
        axis_id: usize,
        divider: usize,
        fraction: f32,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.resize(axis_id, divider, fraction) {
            cx.notify();
        }
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

    /// Whether the tree row at `path` carries the click-driven highlight. The
    /// workspace's active tab deliberately does not feed this.
    fn is_file_path_highlighted(&self, path: &str) -> bool {
        self.file_tree_highlight_path.as_deref() == Some(path)
    }

    #[cfg(test)]
    pub(crate) fn notification_count(&self, cx: &gpui::App) -> usize {
        self.notifications.read(cx).notifications().len()
    }

    #[cfg(test)]
    fn file_diff_old_scroll_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane())
            .diff
            .side_by_side_offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane())
            .diff
            .side_by_side_offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_max_offset(&self) -> gpui::Size<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane())
            .diff
            .side_by_side_max_offset()
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
            let visible_commits = visible_commits(repo, &self.hidden_branches);
            let graph_commits = visible_commits
                .iter()
                .map(|commit| graph::GraphCommit {
                    sha: commit.sha.clone(),
                    authored_timestamp: commit.authored_timestamp,
                    parent_shas: commit.parent_shas.clone(),
                })
                .collect::<Vec<_>>();
            let head_sha = visible_commits
                .iter()
                .find(|commit| commit.is_head)
                .map(|commit| commit.sha.as_str());
            let graph_rows = graph::layout_graph_anchored(&graph_commits, head_sha);
            let max_graph_lanes = graph_rows
                .iter()
                .map(|row| row.lane_count)
                .max()
                .unwrap_or(1);

            let commit_rows = visible_commits
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

        let history_panel = div()
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
            });

        div()
            .flex()
            .w_full()
            .h_full()
            .min_h_0()
            .bg(rgb(0x171717))
            .child(
                h_resizable("graph-split")
                    .with_state(&self.graph_resizable)
                    .child(
                        resizable_panel()
                            .size(px(BRANCH_SIDEBAR_DEFAULT_WIDTH))
                            .child(self.render_branch_sidebar(repo, cx)),
                    )
                    .child(resizable_panel().child(history_panel)),
            )
    }

    fn render_branch_sidebar(
        &self,
        repo: &repo::OpenRepository,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let list_content: AnyElement = if repo.branches.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("branch-sidebar-empty")
                .debug_selector(|| "branch-sidebar-empty".to_string())
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("No branches")
                .into_any_element()
        } else {
            let rows = build_branch_sidebar_rows(
                &repo.branches,
                &self.collapsed_branch_folders,
                &self.hidden_branches,
            );
            let rows = rows
                .iter()
                .enumerate()
                .map(|(index, row)| match row {
                    BranchTreeRow::Section(section) => self
                        .render_branch_section_row(index, section)
                        .into_any_element(),
                    BranchTreeRow::Folder(folder) => self
                        .render_branch_folder_row(index, folder, cx)
                        .into_any_element(),
                    BranchTreeRow::Branch(branch_row) => self
                        .render_branch_row(index, branch_row, cx)
                        .into_any_element(),
                })
                .collect::<Vec<_>>();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .id("branch-sidebar-scroll")
                .debug_selector(|| "branch-sidebar-scroll".to_string())
                .overflow_y_scroll()
                .track_scroll(&self.branch_sidebar_scroll)
                .child(div().flex().flex_col().w_full().children(rows))
                .into_any_element()
        };

        div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .min_h_0()
            .id("branch-sidebar")
            .debug_selector(|| "branch-sidebar".to_string())
            .border_1()
            .border_color(rgb(0x242424))
            .font_family(FILE_TREE_FONT_FAMILY)
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                if app.branch_sidebar_hovered != *hovered {
                    app.branch_sidebar_hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(list_content)
            .when(self.branch_sidebar_hovered, |container| {
                container.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .debug_selector(|| "branch-sidebar-scrollbar".to_string())
                        .child(
                            Scrollbar::vertical(&self.branch_sidebar_scroll)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
            })
    }

    fn render_branch_row(
        &self,
        index: usize,
        row: &BranchRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let branch = &row.branch;
        let selected = matches!(
            &self.selection,
            Selection::Single { sha } if sha == &branch.tip_sha
        );
        let key = branch_key(&branch.name, &branch.kind);
        let hidden = self.hidden_branches.contains(&key);
        let show_toggle = !branch.is_head && (hidden || self.hovered_branch_row == Some(index));
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let name_color = if hidden {
            rgb(0x999999)
        } else if branch.is_head {
            rgb(0xa3e635)
        } else if matches!(branch.kind, repo::BranchKind::Remote { .. }) {
            rgb(REMOTE_BRANCH_TINT)
        } else {
            rgb(0xe6e6e6)
        };
        let name_fragment = debug_ref_label_fragment(&key);
        let row_selector = if selected {
            format!("selected-branch-row-{name_fragment}")
        } else {
            format!("branch-row-{name_fragment}")
        };
        let marker_selector = format!("branch-head-marker-{name_fragment}");
        let toggle_selector = format!("branch-visibility-{name_fragment}");
        let tip_sha = branch.tip_sha.clone();
        let toggle_branch_key = key;
        let display_name = row.display_name.clone();

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_3()
            .bg(row_bg)
            .id(("branch-row", index))
            .debug_selector(move || row_selector.clone())
            .when(!selected && !hidden, |row| {
                row.hover(|style| style.bg(rgb(0x1f2733)))
            })
            .on_hover(cx.listener(move |app, hovered: &bool, _window, cx| {
                if *hovered {
                    if app.hovered_branch_row != Some(index) {
                        app.hovered_branch_row = Some(index);
                        cx.notify();
                    }
                } else if app.hovered_branch_row == Some(index) {
                    app.hovered_branch_row = None;
                    cx.notify();
                }
            }))
            .when(!hidden, |row| {
                row.cursor_pointer().on_click(cx.listener(
                    move |app, _event: &ClickEvent, window, cx| {
                        app.focus_branch(tip_sha.clone(), window, cx);
                    },
                ))
            })
            .when(row.depth > 0, |el| {
                // Plain spacer indent: the branch sidebar deliberately skips
                // the file tree's indent guides.
                el.child(
                    div()
                        .flex_none()
                        .w(px(row.depth as f32 * FILE_TREE_INDENT_WIDTH)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(name_color)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child(display_name),
            )
            .when(branch.is_head, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .debug_selector(move || marker_selector.clone())
                        .child(
                            Icon::new(LucideIcon::Check)
                                .text_color(rgb(0xa3e635))
                                .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            })
            .when(show_toggle, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .id(("branch-visibility", index))
                        .debug_selector(move || toggle_selector.clone())
                        .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                            app.toggle_branch_visibility(toggle_branch_key.clone(), cx);
                        }))
                        .child(
                            Icon::new(if hidden {
                                LucideIcon::EyeOff
                            } else {
                                LucideIcon::Eye
                            })
                            .text_color(rgb(0x999999))
                            .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            })
    }

    fn render_branch_section_row(
        &self,
        index: usize,
        section: &BranchSectionRow,
    ) -> impl IntoElement {
        let selector = format!(
            "branch-section-{}",
            debug_ref_label_fragment(&section.title)
        );
        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT))
            .px_3()
            .bg(rgb(0x171717))
            .id(("branch-section", index))
            .debug_selector(move || selector.clone())
            .text_color(rgb(0x999999))
            .text_size(px(FILE_TREE_TEXT_SIZE))
            .child(section.title.clone())
    }

    fn render_branch_folder_row(
        &self,
        index: usize,
        folder: &BranchFolderRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path_fragment = debug_ref_label_fragment(&folder.path);
        let row_selector = format!("branch-folder-{path_fragment}");
        let toggle_selector = format!("branch-folder-visibility-{path_fragment}");
        let collapse_path = folder.path.clone();
        let toggle_path = folder.path.clone();
        let hidden = folder.visibility == FolderVisibility::Hidden;
        let show_toggle = folder.visibility != FolderVisibility::Visible
            || self.hovered_branch_row == Some(index);
        let name_color = if hidden { rgb(0x999999) } else { rgb(0x8aa6bd) };
        let depth = folder.depth;

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_3()
            .bg(rgb(0x171717))
            .cursor_pointer()
            .id(("branch-folder", index))
            .debug_selector(move || row_selector.clone())
            .hover(|style| style.bg(rgb(0x1f2733)))
            .on_hover(cx.listener(move |app, hovered: &bool, _window, cx| {
                if *hovered {
                    if app.hovered_branch_row != Some(index) {
                        app.hovered_branch_row = Some(index);
                        cx.notify();
                    }
                } else if app.hovered_branch_row == Some(index) {
                    app.hovered_branch_row = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                app.toggle_branch_folder(collapse_path.clone(), cx);
            }))
            .when(depth > 0, |row| {
                // Plain spacer indent: the branch sidebar deliberately skips
                // the file tree's indent guides.
                row.child(
                    div()
                        .flex_none()
                        .w(px(depth as f32 * FILE_TREE_INDENT_WIDTH)),
                )
            })
            .child(
                Icon::new(if folder.collapsed {
                    LucideIcon::ChevronRight
                } else {
                    LucideIcon::ChevronDown
                })
                .text_color(name_color)
                .size(px(FILE_TREE_STATUS_ICON_SIZE)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(name_color)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child(folder.name.clone()),
            )
            .when(show_toggle, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .id(("branch-folder-visibility", index))
                        .debug_selector(move || toggle_selector.clone())
                        .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                            app.toggle_folder_visibility(&toggle_path, cx);
                        }))
                        .child(
                            Icon::new(if folder.visibility == FolderVisibility::Visible {
                                LucideIcon::Eye
                            } else {
                                LucideIcon::EyeOff
                            })
                            .text_color(rgb(0x999999))
                            .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
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
            Ok(entries) => div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(
                    h_resizable("changeset-split")
                        .with_state(&self.changeset_resizable)
                        .child(
                            resizable_panel()
                                .size(px(340.))
                                .child(self.render_file_list(repo, entries, cx)),
                        )
                        .child(resizable_panel().child(
                            crate::workspace::pane_grid::render_pane_group(
                                self,
                                self.workspace.layout(),
                                repo,
                                changeset,
                                cx,
                            ),
                        )),
                )
                .into_any_element(),
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
        repo: &repo::OpenRepository,
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
            let path_cells = rows
                .iter()
                .enumerate()
                .map(|(index, row)| self.render_file_tree_row(index, row, cx))
                .collect::<Vec<_>>();
            let gutter_cells = rows
                .iter()
                .enumerate()
                .map(|(index, row)| self.render_file_tree_gutter_cell(index, row, cx))
                .collect::<Vec<_>>();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .id("changed-files-scroll")
                .debug_selector(|| "changed-files-scroll".to_string())
                .overflow_y_scroll()
                .track_scroll(&self.file_tree_scroll)
                .child(
                    // Two columns share this one vertical scroll, so they scroll
                    // vertically together. min_w_full pins the gutter to the
                    // viewport's right edge.
                    div()
                        .flex()
                        .flex_row()
                        .min_w_full()
                        .child(
                            // Path pane: only this column scrolls horizontally.
                            // items_start() prevents cross-axis stretch, allowing
                            // the flex_none inner wrapper to exceed the viewport width.
                            div()
                                .id("changed-files-path-pane")
                                .debug_selector(|| "changed-files-path-pane".to_string())
                                .flex()
                                .flex_col()
                                .items_start()
                                .flex_1()
                                .min_w_0()
                                .overflow_x_scroll()
                                .track_scroll(&self.file_tree_hscroll)
                                .child(
                                    // flex_none inner column sizes to the widest
                                    // path; rows use w_full so backgrounds are uniform.
                                    div().flex().flex_col().flex_none().children(path_cells),
                                ),
                        )
                        .child(
                            // Frozen stat gutter: explicit width so w_full cells have
                            // a defined column width to fill.
                            div()
                                .flex()
                                .flex_col()
                                .flex_none()
                                .w(px(FILE_TREE_STAT_GUTTER_WIDTH))
                                .children(gutter_cells),
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
            .child(self.render_file_tree_repo_header(repo, folder_defaults, cx))
            .child(list_content)
            .when(self.file_tree_hovered, |container| {
                container.child(
                    div()
                        .absolute()
                        .top(px(FILE_TREE_ROW_HEIGHT))
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .debug_selector(|| "file-tree-scrollbar".to_string())
                        .child(
                            // Vertical bar spans the scrollable list below the header.
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .child(
                                    Scrollbar::vertical(&self.file_tree_scroll)
                                        .scrollbar_show(ScrollbarShow::Always),
                                ),
                        )
                        .child(
                            // Horizontal bar spans only the path pane, not the frozen gutter.
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .bottom_0()
                                .right(px(FILE_TREE_STAT_GUTTER_WIDTH))
                                .child(
                                    Scrollbar::horizontal(&self.file_tree_hscroll)
                                        .scrollbar_show(ScrollbarShow::Always),
                                ),
                        ),
                )
            })
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
        // Depth 0 belongs to the repo root header row; real rows nest under it.
        append_file_tree_rows(
            &root,
            1,
            "",
            &self.collapsed_file_tree_paths,
            collapse_unchanged_by_default,
            &changed_ancestor_paths,
            &mut rows,
        );
        rows
    }

    /// Static header row naming the repository, mirroring the titlebar. The
    /// tree controls live inline at its trailing edge, so they never overlap
    /// a row that carries diff stats.
    fn render_file_tree_repo_header(
        &self,
        repo: &repo::OpenRepository,
        folder_defaults: Vec<(String, bool)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = repository_title(&repo.path);
        let name_selector = format!("file-tree-repo-root-name-{}", debug_path_fragment(&name));

        div()
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .min_h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_2()
            .bg(rgb(0x171717))
            .debug_selector(|| "file-tree-repo-root".to_string())
            .child(render_file_tree_indent_guides(0, "repo-root"))
            .child(render_file_tree_folder_icon(
                "repo-root",
                false,
                rgb(0x8aa6bd),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(rgb(0x8aa6bd))
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
                    .font_family(FILE_TREE_FONT_FAMILY)
                    .whitespace_nowrap()
                    .debug_selector(move || name_selector.clone())
                    .child(name),
            )
            .child(self.render_file_tree_controls(folder_defaults, cx))
    }

    /// The icon-only controls at the trailing edge of the repo root header:
    /// a show-all-files toggle plus collapse-all / expand-all. The header sits
    /// outside the scroll area, so the controls stay pinned while the tree
    /// scrolls.
    fn render_file_tree_controls(
        &self,
        folder_defaults: Vec<(String, bool)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let collapse_folders = folder_defaults.clone();
        let expand_folders = folder_defaults;
        let show_all_active = matches!(self.file_list_mode, FileListMode::All);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
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
                let selected = self.is_file_path_highlighted(row.path());
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

    fn render_file_tree_gutter_cell(
        &self,
        index: usize,
        row: &FileTreeRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Every gutter cell is one row tall so it stays aligned with its path row.
        // w_full + justify_end fills the fixed-width column so all row backgrounds
        // (including selection blue) are exactly the same width.
        let base = |selected: bool| {
            div()
                .flex()
                .items_center()
                .justify_end()
                .w_full()
                .min_h(px(FILE_TREE_ROW_HEIGHT))
                .px_2()
                .bg(if selected {
                    rgb(0x223248)
                } else {
                    rgb(0x171717)
                })
        };

        match row {
            FileTreeRow::File {
                entry: FileListEntry::Changed(file),
                ..
            } => {
                let selected = self.is_file_path_highlighted(row.path());
                let path = file.path.clone();
                let path_fragment = debug_path_fragment(&file.path);
                let diff_stat_selector = format!("changed-file-diff-stat-{path_fragment}");
                let gutter_selector = format!("changed-file-gutter-{index}");
                base(selected)
                    .cursor_pointer()
                    .id(("changed-file-gutter", index))
                    .debug_selector(move || gutter_selector.clone())
                    .on_click(cx.listener(move |app, event: &ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            app.open_file_pinned(path.clone(), cx);
                        } else {
                            app.open_file_preview(path.clone(), cx);
                        }
                    }))
                    .child(render_file_diff_stat(diff_stat_selector, file.line_stats))
                    .into_any_element()
            }
            _ => base(false).into_any_element(),
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
            .on_click(cx.listener(move |app, event: &ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    app.open_file_pinned(path.clone(), cx);
                } else {
                    app.open_file_preview(path.clone(), cx);
                }
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
                                .child(format!("(from {old_path})")),
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
            .on_click(cx.listener(move |app, event: &ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    app.open_file_pinned(path.clone(), cx);
                } else {
                    app.open_file_preview(path.clone(), cx);
                }
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

    pub(crate) fn render_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        selected_path: Option<&str>,
        scroll: &FileDiffScroll,
    ) -> AnyElement {
        match selected_path {
            Some(path) => {
                if let Some(file) = changeset.files.iter().find(|file| file.path == path) {
                    return self.render_changed_file_detail(repo, changeset, file, scroll);
                }

                self.render_read_only_file_detail(repo, changeset, path, scroll)
            }
            None => div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .id("file-detail-empty")
                .debug_selector(|| "file-detail-empty".to_string())
                .items_center()
                .justify_center()
                .text_color(rgb(0x999999))
                .text_size(px(14.))
                .child("Select a file to inspect its diff.")
                .into_any_element(),
        }
    }

    /// The prepared diff for `file` in `changeset`, computed once and cached.
    /// On a miss this reads the file from git and computes the line diff; on a
    /// hit it returns the shared rows untouched. Read errors are not cached —
    /// they are surfaced to the caller and recomputed on the next render.
    fn prepared_file_diff(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        file: &repo::ChangedFile,
    ) -> Result<Rc<PreparedFileDiff>, String> {
        let key = DiffCacheKey {
            path: file.path.clone(),
            commit_sha: changeset.commit_sha.clone(),
            base_sha: changeset.base_sha.clone(),
        };

        if let Some(cached) = self.diff_row_cache.borrow().get(&key) {
            return Ok(cached.clone());
        }

        let diff = repo::file_diff_for_changed_file_between(
            &repo.path,
            &changeset.commit_sha,
            changeset.base_sha.as_deref(),
            file,
        )
        .map_err(|err| err.to_string())?;

        let prepared = Rc::new(PreparedFileDiff::from_content(
            diff.content,
            diff_highlight::language_for_path(&file.path),
        ));
        self.diff_row_cache
            .borrow_mut()
            .insert(key, prepared.clone());
        Ok(prepared)
    }

    fn render_changed_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        file: &repo::ChangedFile,
        scroll: &FileDiffScroll,
    ) -> AnyElement {
        let rename_source_selector = format!(
            "file-detail-rename-source-{}",
            debug_path_fragment(&file.path)
        );
        let content = match self.prepared_file_diff(repo, changeset, file) {
            Ok(prepared) => render_prepared_file_diff(&prepared, scroll),
            Err(err) => render_file_diff_error(err),
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
            .when_some(file.old_path.clone(), |detail, old_path| {
                detail.child(
                    div()
                        .px_3()
                        .py_1()
                        .border_b_1()
                        .border_color(rgb(0x2a2a2a))
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
        scroll: &FileDiffScroll,
    ) -> AnyElement {
        let content = match repo::file_content_at_commit(&repo.path, &changeset.commit_sha, path) {
            Ok(content) => render_file_content(
                content.content,
                scroll,
                diff_highlight::language_for_path(path),
            ),
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
                if event.click_count() >= 2 {
                    app.selection = Selection::Single { sha: sha.clone() };
                    app.open_changeset(window, cx);
                } else {
                    app.select_commit(sha.clone(), event.modifiers(), window, cx);
                }
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
            .child(render_commit_ref_labels(
                index,
                commit,
                &self.hidden_branches,
            ))
    }
}

/// Text tint shared by remote branch rows in the sidebar and remote ref
/// label pills in the graph, so the two surfaces read as one family.
const REMOTE_BRANCH_TINT: u32 = 0x94a3b8;

const COMMIT_ROW_HEIGHT: f32 = 44.;
const COMMIT_ROW_HORIZONTAL_PADDING: f32 = 16.;
const COMMIT_HASH_WIDTH: f32 = 72.;
const COMMIT_AUTHOR_WIDTH: f32 = 168.;
const COMMIT_TIME_WIDTH: f32 = 96.;

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
            .on_action(cx.listener(|app, _: &CloseActiveTab, _window, cx| {
                app.close_active_workspace_tab(cx);
            }))
            .on_action(cx.listener(|app, _: &ActivateNextTab, _window, cx| {
                app.cycle_workspace_tab(true, cx);
            }))
            .on_action(cx.listener(|app, _: &ActivatePreviousTab, _window, cx| {
                app.cycle_workspace_tab(false, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneLeft, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Left, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneRight, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Right, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneUp, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Up, cx);
            }))
            .on_action(cx.listener(|app, _: &SplitPaneDown, _window, cx| {
                app.split_active_workspace_pane(crate::workspace::SplitDirection::Down, cx);
            }))
            .on_action(cx.listener(|app, _: &CloseActivePane, _window, cx| {
                app.close_active_workspace_pane(cx);
            }))
            .child(self.render_title_bar(cx))
            .child(div().flex().flex_1().min_h(px(0.)).w_full().child(body))
            .children(self.render_context_popover(cx))
            .children(self.render_repo_switcher(cx))
            .child(self.notifications.clone())
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
        App, CloseChangeset, Mode, OpenChangeset, OpenFailed, PreparedFileDiff, ReviewScreen,
        Selection, FILE_TREE_ROW_HEIGHT,
    };
    use crate::repo::{ChangeKind, INITIAL_COMMIT_LIMIT};
    use crate::settings::RecentRepository;
    use crate::workspace::test_util::simulate_double_click;
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::{px, Modifiers, TestAppContext, VisualTestContext};
    use std::{fs, rc::Rc};

    use super::test_support::*;

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
    async fn graph_mode_lists_local_branches_with_head_marked(cx: &mut TestAppContext) {
        let (dir, _main_tip, _root) = init_repo_with_feature_branch();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-row-heads-feature")
            .expect("feature branch row renders");
        visual
            .debug_bounds("branch-row-heads-master")
            .expect("master branch row renders");
        visual
            .debug_bounds("branch-head-marker-heads-master")
            .expect("checked-out branch carries the HEAD marker");
        assert!(
            visual
                .debug_bounds("branch-head-marker-heads-feature")
                .is_none(),
            "non-checked-out branch has no HEAD marker",
        );
    }

    #[gpui::test]
    async fn changeset_mode_renders_no_branch_sidebar(cx: &mut TestAppContext) {
        let (dir, main_tip, _root) = init_repo_with_feature_branch();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(main_tip, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("branch-sidebar").is_none(),
            "changeset mode has no branch sidebar",
        );
        visual
            .debug_bounds("changed-files")
            .expect("changeset file tree still renders");
    }

    #[gpui::test]
    async fn clicking_a_branch_selects_and_reveals_its_tip_commit(cx: &mut TestAppContext) {
        let (dir, _main_tip, _root) = init_repo_with_feature_branch();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let feature_row = visual
            .debug_bounds("branch-row-heads-feature")
            .expect("feature branch row renders");
        visual.simulate_click(feature_row.center(), Modifiers::none());

        // The feature branch points at the root commit, which renders at index 1
        // (newest-first: master tip, then root).
        visual
            .debug_bounds("selected-commit-row-1")
            .expect("feature tip commit becomes the selected row");
        visual
            .debug_bounds("selected-branch-row-heads-feature")
            .expect("the branch row reflects the selection");
    }

    #[gpui::test]
    async fn clicking_a_branch_pages_in_history_until_its_tip_is_loaded(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("history.txt"), "base\n").expect("write file");
        let base_oid = commit_all(&repo, "Base", &[]);
        let base_commit = repo.find_commit(base_oid).expect("find base commit");
        repo.branch("old-base", &base_commit, false)
            .expect("create old-base branch");
        drop(base_commit);

        let mut parent = base_oid;
        for index in 0..(INITIAL_COMMIT_LIMIT + 1) {
            fs::write(dir.path().join("history.txt"), format!("commit {index}\n"))
                .expect("write history file");
            parent = commit_all(&repo, &format!("Commit {index}"), &[parent]);
        }
        drop(repo);

        let base_sha = base_oid.to_string();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");

        cx.run_until_parked();

        // The branch tip is beyond the initial page, yet the sidebar lists it.
        let mut visual = VisualTestContext::from_window(*window, cx);
        let branch_row = visual
            .debug_bounds("branch-row-heads-old-base")
            .expect("old-base branch row renders even though its tip is unloaded");
        visual.simulate_click(branch_row.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("repository stays open");
                };
                assert!(
                    repo.commits.iter().any(|commit| commit.sha == base_sha),
                    "clicking the branch paged its tip commit into the history",
                );
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: base_sha.clone()
                    },
                    "the branch tip commit is selected",
                );
                let offset = app.commit_history_scroll.offset();
                assert!(
                    offset.y < px(-1000.),
                    "focusing a deep branch tip scrolled the history substantially, got {:?}",
                    offset.y
                );
            })
            .expect("inspect state");
    }

    #[gpui::test]
    async fn branch_sidebar_shows_placeholder_when_repo_has_no_branches(cx: &mut TestAppContext) {
        let (dir, _tip) = init_repo_with_detached_head_no_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-sidebar-empty")
            .expect("empty branch sidebar shows the placeholder");
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
    async fn moving_out_a_panes_last_tab_prunes_its_scroll_state(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (repo_dir, _) = init_repo_with_one_commit();
        let repo_path = repo_dir.path().canonicalize().expect("canonical repo path");
        let window = add_app_window(cx);
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_path, window, cx);
                // Pane 0 holds one tab; pane 1 is the empty split target.
                app.open_file_pinned("hello.txt".to_string(), cx);
                let pane = app.workspace.active_pane();
                app.split_workspace_pane(pane, crate::workspace::SplitDirection::Right, cx);
                // Touch both panes' scroll state so both have map entries.
                app.pane_scroll(0);
                app.pane_scroll(1);
                // Moving pane 0's only tab collapses pane 0; its scroll
                // state must not linger in the map.
                app.move_workspace_tab(0, 0, 1, 0, cx);
                assert_eq!(app.workspace.pane_ids(), [1]);
                assert!(
                    !app.pane_scrolls.borrow().contains_key(&0),
                    "collapsed pane's scroll state is pruned"
                );
                assert!(app.pane_scrolls.borrow().contains_key(&1));
            })
            .expect("exercise scroll pruning");
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
    async fn hiding_a_branch_clears_a_selection_it_made_invisible(cx: &mut TestAppContext) {
        let (dir, _main_tip, feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(feature_tip.clone(), cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                assert_eq!(app.selection, Selection::None);
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn hiding_a_branch_keeps_a_still_visible_selection(cx: &mut TestAppContext) {
        let (dir, main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(main_tip.clone(), cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: main_tip.clone()
                    }
                );
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn toggling_a_branch_twice_restores_visibility(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                assert!(app.hidden_branches.contains("heads/feature"));
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                assert!(app.hidden_branches.is_empty());
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn opening_a_repository_resets_hidden_branches(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                assert!(!app.hidden_branches.is_empty());
                app.open_repository_at(path, window, cx);
                assert!(app.hidden_branches.is_empty());
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn hiding_a_branch_removes_its_exclusive_commits_and_label(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        // Three commits loaded, one (the feature-exclusive commit) hidden.
        assert!(visual.debug_bounds("commit-row-1").is_some());
        assert!(
            visual.debug_bounds("commit-row-2").is_none(),
            "the feature-exclusive commit must not render"
        );
        // The feature ref label is gone from every remaining row.
        for row in 0..2usize {
            let selector = test_debug_selector(format!("commit-ref-label-{row}-heads-feature"));
            assert!(
                visual.debug_bounds(selector).is_none(),
                "hidden branch label must not render on row {row}"
            );
        }
    }

    #[gpui::test]
    async fn showing_a_branch_again_restores_its_commits_and_label(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
            })
            .expect("hide and re-show feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        // Hide-then-show must round-trip to the identical render: all three
        // rows back, the feature label on exactly one of them.
        for row in 0..3 {
            assert!(
                visual
                    .debug_bounds(test_debug_selector(format!("commit-row-{row}")))
                    .is_some()
                    || visual
                        .debug_bounds(test_debug_selector(format!("selected-commit-row-{row}")))
                        .is_some(),
                "row {row} must render after re-showing the branch"
            );
        }
        let feature_label_rows = (0..3)
            .filter(|row| {
                visual
                    .debug_bounds(test_debug_selector(format!(
                        "commit-ref-label-{row}-heads-feature"
                    )))
                    .is_some()
            })
            .count();
        assert_eq!(
            feature_label_rows, 1,
            "the feature label renders on exactly one row"
        );
    }

    #[gpui::test]
    async fn focusing_a_branch_targets_its_visible_row_index(cx: &mut TestAppContext) {
        // Hiding `feature` removes row 1, shifting root from index 2 to 1.
        // Focusing master must select the row at its *visible* index.
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let master_row = visual
            .debug_bounds("branch-row-heads-master")
            .expect("master branch row renders");
        visual.simulate_click(master_row.center(), Modifiers::none());

        // With feature hidden the visible order is: master tip (0), root (1).
        visual
            .debug_bounds("selected-commit-row-0")
            .expect("master tip is the selected visible row");
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
    async fn double_clicking_a_commit_opens_its_changeset(cx: &mut TestAppContext) {
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
        let row_bounds = visual
            .debug_bounds("commit-row-1")
            .expect("middle commit row debug bounds");
        simulate_double_click(&mut visual, row_bounds.center());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: shas[1].clone()
                    },
                    "double-click selects exactly the double-clicked commit"
                );
                match &app.review_screen {
                    ReviewScreen::Changeset { sha, .. } => assert_eq!(sha, &shas[1]),
                    ReviewScreen::Graph => panic!("double-click must open the changeset"),
                }
            })
            .expect("read review screen");
    }

    #[gpui::test]
    async fn double_clicking_inside_a_range_opens_the_single_commit(cx: &mut TestAppContext) {
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

        // Range tip..root is selected; double-click the middle commit.
        let middle_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("middle commit row debug bounds");
        simulate_double_click(&mut visual, middle_bounds.center());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: shas[1].clone()
                    },
                    "double-click replaces the range with the single commit"
                );
                match &app.review_screen {
                    ReviewScreen::Changeset { sha, .. } => assert_eq!(sha, &shas[1]),
                    ReviewScreen::Graph => panic!(
                        "double-click inside a range must open the single commit's changeset"
                    ),
                }
            })
            .expect("read review screen");
    }

    #[gpui::test]
    async fn double_clicking_a_selected_commit_still_opens_its_changeset(cx: &mut TestAppContext) {
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
        let row_bounds = visual
            .debug_bounds("commit-row-1")
            .expect("middle commit row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        let selected_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("selected middle commit row debug bounds");
        simulate_double_click(&mut visual, selected_bounds.center());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: shas[1].clone()
                    },
                    "double-click overrides the click-again-to-clear toggle"
                );
                match &app.review_screen {
                    ReviewScreen::Changeset { sha, .. } => assert_eq!(sha, &shas[1]),
                    ReviewScreen::Graph => {
                        panic!("double-click on a selected commit must open its changeset")
                    }
                }
            })
            .expect("read review screen");
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
    async fn prepared_file_diff_reuses_cached_rows_within_a_changeset(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_three_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(shas[1].clone(), cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");
        cx.run_until_parked();

        window
            .update(cx, |app, _window, _cx| {
                let repo = match &app.mode {
                    Mode::RepoOpen { repo } => repo,
                    Mode::NoRepo => panic!("expected an open repository"),
                };
                let changeset = match &app.review_screen {
                    ReviewScreen::Changeset { changeset, .. } => changeset,
                    ReviewScreen::Graph => panic!("expected the changeset screen"),
                };
                let file = &changeset.files[0];

                let first = app
                    .prepared_file_diff(repo, changeset, file)
                    .expect("prepare diff");
                // The computed rows are non-empty for a real text change.
                match first.as_ref() {
                    PreparedFileDiff::SideBySide { rows } => assert!(!rows.is_empty()),
                    other => panic!("expected a side-by-side diff, got {other:?}"),
                }

                let second = app
                    .prepared_file_diff(repo, changeset, file)
                    .expect("prepare diff again");

                assert!(
                    Rc::ptr_eq(&first, &second),
                    "a repeated call should return the cached rows, not recompute them"
                );
                assert_eq!(
                    app.diff_row_cache.borrow().len(),
                    1,
                    "only one entry should be cached for the single rendered file"
                );
            })
            .expect("inspect cache");
    }

    #[gpui::test]
    async fn changing_changeset_selection_invalidates_diff_cache(cx: &mut TestAppContext) {
        let (dir, shas) = init_repo_with_three_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(shas[1].clone(), cx);
                app.open_changeset(window, cx);
            })
            .expect("open first changeset");
        cx.run_until_parked();

        window
            .update(cx, |app, _window, _cx| {
                let repo = match &app.mode {
                    Mode::RepoOpen { repo } => repo,
                    Mode::NoRepo => panic!("expected an open repository"),
                };
                let changeset = match &app.review_screen {
                    ReviewScreen::Changeset { changeset, .. } => changeset,
                    ReviewScreen::Graph => panic!("expected the changeset screen"),
                };
                let file = &changeset.files[0];
                app.prepared_file_diff(repo, changeset, file)
                    .expect("prime cache");
                assert_eq!(app.diff_row_cache.borrow().len(), 1);
            })
            .expect("prime cache");

        // Selecting and opening a different changeset must clear the cache so a
        // later render recomputes against the new commit.
        window
            .update(cx, |app, window, cx| {
                app.select_single_commit(shas[0].clone(), cx);
                app.open_changeset(window, cx);
            })
            .expect("open second changeset");
        cx.run_until_parked();

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.diff_row_cache.borrow().is_empty(),
                    "switching changesets should invalidate the diff cache"
                );
            })
            .expect("verify invalidation");
    }

    #[gpui::test]
    async fn opening_file_preview_records_tab_and_highlight(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("hello.txt".to_string(), cx);

                assert_eq!(
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("hello.txt".to_string()),
                );
                assert_eq!(app.file_tree_highlight_path, Some("hello.txt".to_string()));
            })
            .expect("open file preview");
    }

    #[gpui::test]
    async fn opening_a_changeset_resets_the_pane_layout(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_pinned("hello.txt".to_string(), cx);
                let pane = app.workspace.active_pane();
                app.split_workspace_pane(pane, crate::workspace::SplitDirection::Right, cx);
                assert_eq!(app.workspace.pane_ids().len(), 2);

                app.close_changeset(cx);
                app.open_changeset(window, cx);

                assert_eq!(
                    app.workspace.pane_ids(),
                    [0],
                    "each changeset starts with the default single-pane layout"
                );
                assert!(app.workspace.tabs(0).is_empty());
            })
            .expect("reopen changeset with reset layout");
    }

    #[gpui::test]
    async fn reopening_a_changeset_starts_with_no_tabs(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("hello.txt".to_string(), cx);
                app.close_changeset(cx);
                app.open_changeset(window, cx);

                assert!(app.workspace.tabs(0).is_empty());
                assert_eq!(app.file_tree_highlight_path, None);
            })
            .expect("reopen changeset");
    }

    #[gpui::test]
    async fn opening_changeset_clears_tabs_and_highlight(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.open_file_preview("missing.txt".to_string(), cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);

                assert!(app.workspace.tabs(0).is_empty());
                assert_eq!(app.file_tree_highlight_path, None);
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
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("hello.txt".to_string()),
                );
                assert_eq!(app.file_tree_highlight_path, Some("hello.txt".to_string()));
            })
            .expect("read selected changed file");
    }

    #[gpui::test]
    async fn clicking_gutter_cell_selects_file(cx: &mut TestAppContext) {
        // Use a two-file commit so that gutter row 1 is not obscured by the
        // floating file-tree controls (which occlude the top-right area at row 0).
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        fs::write(dir.path().join("alpha.txt"), "a\n").expect("write alpha");
        fs::write(dir.path().join("beta.txt"), "b\n").expect("write beta");
        let root_oid = commit_all(&repo, "Add two files", &[]);
        drop(repo);
        let oid_hex = root_oid.to_string();
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
        cx.run_until_parked();

        // Row 1 is clear of the floating controls that occlude the top-right corner.
        let gutter_bounds = visual
            .debug_bounds("changed-file-gutter-1")
            .expect("gutter cell 1 debug bounds");
        visual.simulate_click(gutter_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("selected-changed-file-row-1")
            .expect("selected changed file row 1 debug bounds");
        visual
            .debug_bounds("file-detail-shell")
            .expect("file detail shell debug bounds");

        window
            .read_with(cx, |app, _cx| {
                assert!(
                    app.workspace.active_item(0).is_some(),
                    "a file should be open after clicking gutter row 1"
                );
                assert!(
                    app.file_tree_highlight_path.is_some(),
                    "a file should be highlighted after clicking gutter row 1"
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
