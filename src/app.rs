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
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::tooltip::Tooltip;
use gpui_component::Icon;
use similar::{DiffTag, TextDiff};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::icons::LucideIcon;
use crate::settings::{self, RecentRepository, Settings, MAX_RECENT_REPOSITORIES};
use crate::workspace::FileDiffItem;
use crate::{graph, repo};

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
    /// Branch names the user has toggled off in the sidebar. Session-only:
    /// cleared whenever a repository is opened. The checked-out branch is
    /// never in this set (its row renders no toggle).
    hidden_branches: BTreeSet<String>,
    /// Sidebar folder paths the user has collapsed. Session-only: cleared
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
    old: ScrollHandle,
    new: ScrollHandle,
    side_by_side: ScrollHandle,
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
/// carry the full `LocalBranch` — selection, hiding, and debug selectors all
/// key on the full name; only `display_name` is shortened.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BranchTreeRow {
    Folder(BranchFolderRow),
    Branch(BranchRow),
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
    branch: repo::LocalBranch,
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
            .local_branches
            .iter()
            .filter(|branch| !branch.is_head && branch.name.starts_with(&prefix))
            .map(|branch| branch.name.clone())
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
            &repo.local_branches,
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
    /// behind `load_older_commits` pushes every local branch tip, so paging
    /// always reaches the commit unless loading itself fails.
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
            .side_by_side
            .offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane())
            .diff
            .side_by_side
            .offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_max_offset(&self) -> gpui::Size<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane())
            .diff
            .side_by_side
            .max_offset()
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
        let list_content: AnyElement = if repo.local_branches.is_empty() {
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
            let rows = build_branch_tree_rows(
                &repo.local_branches,
                &self.collapsed_branch_folders,
                &self.hidden_branches,
            );
            let rows = rows
                .iter()
                .enumerate()
                .map(|(index, row)| match row {
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
        let hidden = self.hidden_branches.contains(&branch.name);
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
        } else {
            rgb(0xe6e6e6)
        };
        let name_fragment = debug_ref_label_fragment(&branch.name);
        let row_selector = if selected {
            format!("selected-branch-row-{name_fragment}")
        } else {
            format!("branch-row-{name_fragment}")
        };
        let marker_selector = format!("branch-head-marker-{name_fragment}");
        let toggle_selector = format!("branch-visibility-{name_fragment}");
        let tip_sha = branch.tip_sha.clone();
        let toggle_branch_name = branch.name.clone();
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
                            app.toggle_branch_visibility(toggle_branch_name.clone(), cx);
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

        let prepared = Rc::new(PreparedFileDiff::from_content(diff.content));
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
        let title = file.path.clone();
        let kind = change_kind_label(file.kind);
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
        scroll: &FileDiffScroll,
    ) -> AnyElement {
        let content = match repo::file_content_at_commit(&repo.path, &changeset.commit_sha, path) {
            Ok(content) => render_file_content(content.content, scroll),
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
            .child(render_commit_ref_labels(
                index,
                commit,
                &self.hidden_branches,
            ))
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

fn render_commit_ref_labels(
    row_index: usize,
    commit: &repo::CommitInfo,
    hidden_branches: &BTreeSet<String>,
) -> gpui::Div {
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
            .filter(|name| !hidden_branches.contains(*name))
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

/// The set of loaded commits reachable from HEAD or from any branch whose
/// name is not in `hidden_branches`. Parents beyond the loaded page simply
/// terminate the walk: a commit that is not loaded cannot be rendered anyway,
/// and paging in more history re-runs this computation over the larger list.
fn visible_commit_shas(
    commits: &[repo::CommitInfo],
    local_branches: &[repo::LocalBranch],
    head_sha: Option<&str>,
    hidden_branches: &BTreeSet<String>,
) -> HashSet<String> {
    let commits_by_sha: HashMap<&str, &repo::CommitInfo> = commits
        .iter()
        .map(|commit| (commit.sha.as_str(), commit))
        .collect();

    let mut worklist: Vec<&str> = Vec::new();
    worklist.extend(head_sha);
    worklist.extend(
        local_branches
            .iter()
            .filter(|branch| !hidden_branches.contains(&branch.name))
            .map(|branch| branch.tip_sha.as_str()),
    );

    let mut visible = HashSet::new();
    while let Some(sha) = worklist.pop() {
        let Some(commit) = commits_by_sha.get(sha) else {
            continue;
        };
        if !visible.insert(commit.sha.clone()) {
            continue;
        }
        worklist.extend(commit.parent_shas.iter().map(String::as_str));
    }
    visible
}

/// The loaded commits that survive branch-visibility filtering, in history
/// order. Render and focus paths must both use this so row indices agree.
///
/// The empty-set fast-path is the identity over loaded commits. For real
/// repositories that equals the reachability walk, because `repo::read_commit_page`
/// seeds its revwalk only from local branch tips and HEAD — every loaded commit
/// is reachable from at least one of those seeds. Synthetic test fixtures seed
/// commits without `local_branches` and rely on the fast-path to render at
/// all; if the revwalk ever loads unreachable commits, hide-then-show would
/// no longer round-trip and this fast-path must be revisited.
fn visible_commits<'a>(
    repo: &'a repo::OpenRepository,
    hidden_branches: &BTreeSet<String>,
) -> Vec<&'a repo::CommitInfo> {
    if hidden_branches.is_empty() {
        return repo.commits.iter().collect();
    }
    let head_sha = repo
        .commits
        .iter()
        .find(|commit| commit.is_head)
        .map(|commit| commit.sha.as_str());
    let visible = visible_commit_shas(
        &repo.commits,
        &repo.local_branches,
        head_sha,
        hidden_branches,
    );
    repo.commits
        .iter()
        .filter(|commit| visible.contains(&commit.sha))
        .collect()
}

fn render_commit_graph_gutter(
    row_index: usize,
    row: &graph::GraphRow,
    previous_row: Option<&graph::GraphRow>,
    next_row: Option<&graph::GraphRow>,
    max_lanes: usize,
) -> impl IntoElement {
    let lane_count = max_lanes.max(1);
    let debug_selector = format!("commit-graph-gutter-{row_index}");

    // Lanes paint right to left so the edges in lower lanes draw above the
    // branches that join them.
    div()
        .relative()
        .w(px(commit_graph_gutter_width(lane_count)))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .font_family("monospace")
        .id(("commit-graph-gutter", row_index))
        .debug_selector(move || debug_selector.clone())
        .children(
            (0..lane_count)
                .rev()
                .map(|lane| {
                    div()
                        .absolute()
                        .left(px(lane as f32 * COMMIT_GRAPH_LANE_WIDTH))
                        .top_0()
                        .child(render_commit_graph_lane(
                            row_index,
                            lane,
                            row,
                            CommitGraphNeighborRows {
                                previous: previous_row,
                                next: next_row,
                            },
                        ))
                })
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
                                rows.get(row_index + 1),
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

#[derive(Clone, Copy)]
struct CommitGraphNeighborRows<'a> {
    previous: Option<&'a graph::GraphRow>,
    next: Option<&'a graph::GraphRow>,
}

fn render_commit_graph_lane(
    row_index: usize,
    lane: usize,
    row: &graph::GraphRow,
    neighbors: CommitGraphNeighborRows<'_>,
) -> gpui::Div {
    let has_incoming = row.incoming_lanes.contains(&lane);
    let has_outgoing = row.outgoing_lanes.contains(&lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let lane_selector = format!("commit-graph-lane-{row_index}-{lane}");
    // The upper merge curve into this commit paints first so the commit
    // lane's own vertical and dot stay on top of the joining branch.
    let upper_merge_target_elbow = (lane == row.lane)
        .then(|| {
            commit_graph_upper_merge_in_connectors(row)
                .into_iter()
                .min_by_key(|connector| connector.from_lane)
        })
        .flatten();

    div()
        .relative()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(COMMIT_GRAPH_LANE_HEIGHT))
        .debug_selector(move || lane_selector.clone())
        .when_some(upper_merge_target_elbow, |lane_div, connector| {
            lane_div.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(COMMIT_GRAPH_VERTICAL_HEIGHT))
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(commit_graph_middle_height()))
                    .child(render_commit_graph_rounded_elbow(
                        format!("commit-graph-rounded-upper-merge-target-elbow-{row_index}-{lane}"),
                        graph::GraphConnectorKind::MergeIn,
                        false,
                        commit_graph_upper_merge_in_horizontal_top_in_middle(),
                        commit_graph_connector_color(row, connector),
                    )),
            )
        })
        .child(render_commit_graph_vertical_segment(
            row_index,
            lane,
            row,
            neighbors,
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
            neighbors,
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
    neighbors: CommitGraphNeighborRows<'_>,
    position: &'static str,
    visible: bool,
    color: gpui::Rgba,
) -> gpui::Div {
    let selector = format!("commit-graph-vertical-{row_index}-{lane}-{position}");
    let (top, height) = commit_graph_vertical_segment_geometry(row, neighbors, lane, position);
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
    neighbors: CommitGraphNeighborRows<'_>,
    lane: usize,
    position: &'static str,
) -> (f32, f32) {
    if position == "top" {
        if let Some(top) = commit_graph_top_vertical_inset_after_previous_row_branch_out(
            row,
            neighbors.previous,
            lane,
        ) {
            return (top, COMMIT_GRAPH_VERTICAL_HEIGHT - top);
        }
    }

    if position == "bottom" {
        if let Some(height) =
            commit_graph_bottom_vertical_inset_before_next_row_merge_in(neighbors.next, lane)
        {
            return (0., height);
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
        || commit_graph_rounded_elbow_turns_up(previous_row, lane)
    {
        return None;
    }

    Some(commit_graph_bend_radius() - commit_graph_line_width())
}

/// When the next row merges this lane into its commit along the upper border
/// line, this row's outgoing vertical stops at the curve tangent instead of
/// running into the bend.
fn commit_graph_bottom_vertical_inset_before_next_row_merge_in(
    next_row: Option<&graph::GraphRow>,
    lane: usize,
) -> Option<f32> {
    let next_row = next_row?;
    commit_graph_upper_merge_in_connectors(next_row)
        .iter()
        .any(|connector| connector.from_lane == lane)
        .then(|| {
            COMMIT_GRAPH_VERTICAL_HEIGHT - commit_graph_bend_radius() + commit_graph_line_width()
        })
}

/// Merge connectors that terminate at this row's commit while the commit lane
/// is also fed from above. These render along the row's upper border: the
/// branch verticals curve to horizontal and join the commit lane's vertical
/// just above the dot.
fn commit_graph_upper_merge_in_connectors(row: &graph::GraphRow) -> Vec<graph::GraphConnector> {
    row.connectors
        .iter()
        .copied()
        .filter(|connector| commit_graph_connector_uses_upper_merge_in_line(row, *connector))
        .collect()
}

fn commit_graph_connector_uses_upper_merge_in_line(
    row: &graph::GraphRow,
    connector: graph::GraphConnector,
) -> bool {
    connector.kind == graph::GraphConnectorKind::MergeIn
        && connector.to_lane == row.lane
        && row.incoming_lanes.contains(&connector.to_lane)
}

fn commit_graph_uses_upper_merge_in_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    commit_graph_connector_uses_upper_merge_in_line(row, connector)
}

fn commit_graph_upper_merge_in_horizontal_top_in_middle() -> f32 {
    -(COMMIT_GRAPH_VERTICAL_HEIGHT + commit_graph_line_width() / 2.)
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
    let Some(connector) = commit_graph_target_connector_for_lane(row, lane) else {
        // A branch lane merging along the upper border leaves this row
        // entirely: its curve sits above the row, so no vertical remains.
        let source_connector = commit_graph_source_connector_for_lane(row, lane)?;
        if !commit_graph_connector_uses_upper_merge_in_line(row, source_connector) {
            return None;
        }

        let middle_center_y = COMMIT_GRAPH_VERTICAL_HEIGHT
            + commit_graph_upper_merge_in_horizontal_top_in_middle()
            + commit_graph_line_width() / 2.;
        return Some(middle_center_y - commit_graph_bend_radius());
    };

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

    Some(if commit_graph_rounded_elbow_turns_up(row, lane) {
        middle_center_y - commit_graph_bend_radius()
    } else {
        middle_center_y + commit_graph_bend_radius()
    })
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
            // Merges drawn along the upper border join the commit lane's
            // vertical above the dot, so they take no dot-height stub.
            let right_target_connector = commit_graph_target_connector_from_side(row, lane, true)
                .filter(|connector| !commit_graph_connector_uses_upper_merge_in_line(row, *connector));
            let right_side_has_non_upper_connector = row.connectors.iter().any(|connector| {
                connector.from_lane.max(connector.to_lane) > lane
                    && !commit_graph_connector_uses_upper_merge_in_line(row, *connector)
            });
            let has_right_connector = (has_connector
                && lane < max_lane
                && right_side_has_non_upper_connector)
                || right_target_connector.is_some();
            let left_connector = commit_graph_connector_on_side(row, lane, false);
            let right_connectors = commit_graph_connectors_on_side(row, lane, true);
            let right_connector =
                commit_graph_connector_on_side(row, lane, true).or(right_target_connector);
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
                                has_right_connector && !right_connector_is_rounded,
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

fn commit_graph_commit_side_rounded_connector(
    row: &graph::GraphRow,
    lane: usize,
    right: bool,
) -> Option<graph::GraphConnector> {
    if right {
        return commit_graph_connectors_on_side(row, lane, true)
            .into_iter()
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

fn commit_graph_rounded_elbow_turns_up(row: &graph::GraphRow, lane: usize) -> bool {
    row.incoming_lanes.contains(&lane) && !row.outgoing_lanes.contains(&lane)
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
        && row.outgoing_lanes.contains(&connector.to_lane)
}

fn commit_graph_uses_lower_merge_in_line(row: &graph::GraphRow, lane: usize) -> bool {
    let Some(connector) = commit_graph_connector_for_lane(row, lane) else {
        return false;
    };

    connector.kind == graph::GraphConnectorKind::MergeIn
        && connector.to_lane != row.lane
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
    let uses_upper_merge_in_line = commit_graph_uses_upper_merge_in_line(row, lane);
    let lane_color = commit_graph_lane_color(row, lane);
    let color = connector
        .map(|connector| commit_graph_connector_color(row, connector))
        .unwrap_or(lane_color);
    // An outer edge crossing this merging lane along the upper border keeps
    // its own color underneath this lane's bend.
    let upper_merge_crossing = source_connector
        .filter(|connector| commit_graph_connector_uses_upper_merge_in_line(row, *connector))
        .and_then(|_| commit_graph_spanning_connector_for_lane(row, lane))
        .filter(|connector| commit_graph_connector_uses_upper_merge_in_line(row, *connector));
    let (left_visible, right_visible) = match (target_connector, source_connector) {
        (Some(connector), _) => match connector.kind {
            graph::GraphConnectorKind::BranchOut => (true, false),
            graph::GraphConnectorKind::MergeIn => (false, true),
            graph::GraphConnectorKind::Straight => (true, true),
        },
        (None, Some(connector))
            if connector.kind == graph::GraphConnectorKind::MergeIn && connector.to_lane < lane =>
        {
            // Along the upper border a crossing edge is drawn as a full-width
            // underlay instead; on the middle line another edge merging across
            // this lane still needs the right half of the horizontal.
            (
                true,
                !uses_upper_merge_in_line
                    && commit_graph_spanning_connector_for_lane(row, lane).is_some(),
            )
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
    let horizontal_top_y = if uses_upper_merge_in_line {
        commit_graph_upper_merge_in_horizontal_top_in_middle()
    } else if uses_lower_merge_in_line || uses_lower_branch_out_line {
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
            || !commit_graph_rounded_elbow_turns_up(row, lane)
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

    let upper_merge_crossing_selector =
        format!("commit-graph-upper-merge-crossing-{row_index}-{lane}");
    let mut connector_shape = div()
        .relative()
        .w(px(COMMIT_GRAPH_LANE_WIDTH))
        .h(px(commit_graph_middle_height()))
        .when_some(upper_merge_crossing, |shape, crossing| {
            shape.child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(horizontal_top_y))
                    .w(px(COMMIT_GRAPH_LANE_WIDTH))
                    .h(px(COMMIT_GRAPH_LINE_WIDTH))
                    .bg(commit_graph_connector_color(row, crossing))
                    .debug_selector(move || upper_merge_crossing_selector.clone()),
            )
        })
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
                .map(|_| commit_graph_rounded_elbow_turns_up(row, lane))
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

fn render_prepared_file_diff(prepared: &PreparedFileDiff, scroll: &FileDiffScroll) -> AnyElement {
    match prepared {
        PreparedFileDiff::Single { side, rows } => {
            let side = *side;
            let label = match side {
                repo::DiffSide::Old => "Before",
                repo::DiffSide::New => "After",
            };
            let selector = match side {
                repo::DiffSide::Old => "file-diff-side-old",
                repo::DiffSide::New => "file-diff-side-new",
            };
            let cells = rows
                .iter()
                .map(|row| match side {
                    repo::DiffSide::Old => row.old.clone(),
                    repo::DiffSide::New => row.new.clone(),
                })
                .collect::<Vec<_>>();

            render_file_diff_side(label, selector, cells, scroll.handle_for(side))
                .into_any_element()
        }
        PreparedFileDiff::SideBySide { rows } => {
            let old_cells = rows.iter().map(|row| row.old.clone()).collect::<Vec<_>>();
            let new_cells = rows.iter().map(|row| row.new.clone()).collect::<Vec<_>>();

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
        PreparedFileDiff::Binary => render_binary_diff_placeholder(),
    }
}

fn render_binary_diff_placeholder() -> AnyElement {
    div()
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
        .into_any_element()
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
        repo::FileContentBody::Binary => render_binary_diff_placeholder(),
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

/// Identifies a cached diff: the changed file's path plus the commit and base
/// shas it was diffed against. Two changesets that touch the same path produce
/// different keys, so a stale entry is never served.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiffCacheKey {
    path: String,
    commit_sha: String,
    base_sha: Option<String>,
}

/// A changed file's diff content with the expensive work already done: the line
/// diff computed and the per-side rows aligned. This is what the diff cache
/// holds so `render_changed_file_detail` can rebuild its elements cheaply.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedFileDiff {
    Single {
        side: repo::DiffSide,
        rows: Vec<DiffRow>,
    },
    SideBySide {
        rows: Vec<DiffRow>,
    },
    Binary,
}

impl PreparedFileDiff {
    fn from_content(content: repo::FileDiffContent) -> Self {
        match content {
            repo::FileDiffContent::Single { side, text } => PreparedFileDiff::Single {
                side,
                rows: single_side_diff_rows(side, &text),
            },
            repo::FileDiffContent::SideBySide { old_text, new_text } => {
                PreparedFileDiff::SideBySide {
                    rows: side_by_side_diff_rows(&old_text, &new_text),
                }
            }
            repo::FileDiffContent::Binary => PreparedFileDiff::Binary,
        }
    }
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

pub(crate) fn change_kind_text(kind: repo::ChangeKind) -> gpui::Rgba {
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
                .left(px((level + 1) as f32 * FILE_TREE_INDENT_WIDTH
                    + if level > 0 {
                        FILE_TREE_GUIDE_TO_ITEM_GAP
                    } else {
                        0.
                    }
                    - FILE_TREE_INDENT_GUIDE_WIDTH / 2.))
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
        .w(px(FILE_TREE_DIFF_STAT_WIDTH))
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

/// Tree node used while grouping branches by slash-separated name segments.
/// The BTreeMap keeps sibling folders alphabetical; branches within a node
/// stay in input order, which is alphabetical because they share a prefix
/// and `local_branches` arrives sorted by full name.
#[derive(Debug, Default)]
struct BranchTreeNode {
    folders: BTreeMap<String, BranchTreeNode>,
    branches: Vec<repo::LocalBranch>,
}

/// Group branches into folders by `/`-separated name segments and flatten
/// the tree into depth-tagged sidebar rows. Folders list before branches at
/// each level. A collapsed folder contributes its own row and skips every
/// descendant. Git rejects ref names with empty segments, so segments are
/// always non-empty.
fn build_branch_tree_rows(
    local_branches: &[repo::LocalBranch],
    collapsed_folders: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
) -> Vec<BranchTreeRow> {
    let mut root = BranchTreeNode::default();
    for branch in local_branches {
        let mut segments = branch.name.split('/').collect::<Vec<_>>();
        segments.pop();
        let mut node = &mut root;
        for segment in segments {
            node = node.folders.entry(segment.to_string()).or_default();
        }
        node.branches.push(branch.clone());
    }

    let mut rows = Vec::new();
    append_branch_tree_rows(&root, 0, "", collapsed_folders, hidden_branches, &mut rows);
    rows
}

fn append_branch_tree_rows(
    node: &BranchTreeNode,
    depth: usize,
    prefix: &str,
    collapsed_folders: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
    rows: &mut Vec<BranchTreeRow>,
) {
    for (name, child) in &node.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let collapsed = collapsed_folders.contains(&path);
        rows.push(BranchTreeRow::Folder(BranchFolderRow {
            name: name.clone(),
            path: path.clone(),
            depth,
            collapsed,
            visibility: folder_visibility(child, hidden_branches),
        }));
        if !collapsed {
            append_branch_tree_rows(
                child,
                depth + 1,
                &path,
                collapsed_folders,
                hidden_branches,
                rows,
            );
        }
    }

    for branch in &node.branches {
        let display_name = branch
            .name
            .rsplit('/')
            .next()
            .unwrap_or(&branch.name)
            .to_string();
        rows.push(BranchTreeRow::Branch(BranchRow {
            branch: branch.clone(),
            display_name,
            depth,
        }));
    }
}

fn folder_visibility(
    node: &BranchTreeNode,
    hidden_branches: &BTreeSet<String>,
) -> FolderVisibility {
    let mut any_hidden = false;
    let mut any_visible = false;
    collect_folder_visibility(node, hidden_branches, &mut any_hidden, &mut any_visible);
    match (any_hidden, any_visible) {
        (true, false) => FolderVisibility::Hidden,
        (true, true) => FolderVisibility::Mixed,
        _ => FolderVisibility::Visible,
    }
}

fn collect_folder_visibility(
    node: &BranchTreeNode,
    hidden_branches: &BTreeSet<String>,
    any_hidden: &mut bool,
    any_visible: &mut bool,
) {
    for child in node.folders.values() {
        collect_folder_visibility(child, hidden_branches, any_hidden, any_visible);
    }
    for branch in &node.branches {
        if branch.is_head {
            continue;
        }
        if hidden_branches.contains(&branch.name) {
            *any_hidden = true;
        } else {
            *any_visible = true;
        }
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
        build_branch_tree_rows, commit_graph_connector_color_lane, commit_graph_connector_for_lane,
        commit_graph_line_width, commit_graph_merge_in_commit_line_y,
        commit_graph_spanning_connector_requires_center_fill, commit_row_separator_width,
        debug_ref_label_fragment, side_by_side_diff_rows, single_side_diff_rows,
        visible_commit_shas, App, BranchFolderRow, BranchRow, BranchTreeRow, CloseChangeset,
        DiffLineStatus, FileListEntry, FileListMode, FileTreeRow, FolderVisibility, Mode,
        OpenChangeset, OpenFailed, PreparedFileDiff, ReviewScreen, Selection,
        FILE_TREE_FOLDER_ICON_SIZE, FILE_TREE_FONT_FAMILY, FILE_TREE_INDENT_WIDTH,
        FILE_TREE_ROW_HEIGHT, FILE_TREE_STATUS_ICON_SIZE, FILE_TREE_TEXT_SIZE,
    };
    use crate::graph::{self, GraphConnectorKind};
    use crate::repo::{self, ChangeKind, DiffSide, INITIAL_COMMIT_LIMIT};
    use crate::settings::{self, RecentRepository, Settings};
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::{font, point, px, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
    use std::{collections::BTreeSet, fs, path::PathBuf, rc::Rc};

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

    fn commit_info(sha: &str, parent_shas: &[&str]) -> repo::CommitInfo {
        repo::CommitInfo {
            sha: sha.to_string(),
            short_sha: sha.chars().take(7).collect(),
            summary: format!("commit {sha}"),
            author: "Tester".to_string(),
            authored_timestamp: 0,
            authored_date: "2026-01-01".to_string(),
            parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
            branch_names: Vec::new(),
            parent_count: parent_shas.len(),
            is_head: false,
        }
    }

    fn local_branch(name: &str, tip_sha: &str) -> repo::LocalBranch {
        repo::LocalBranch {
            name: name.to_string(),
            tip_sha: tip_sha.to_string(),
            is_head: false,
        }
    }

    fn hidden(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn hiding_a_branch_removes_its_exclusive_commits() {
        // feature-tip -> root <- main-tip; hiding feature drops feature-tip only.
        let commits = vec![
            commit_info("feature-tip", &["root"]),
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "feature-tip"),
            local_branch("master", "main-tip"),
        ];

        let visible =
            visible_commit_shas(&commits, &branches, Some("main-tip"), &hidden(&["feature"]));

        assert!(!visible.contains("feature-tip"));
        assert!(visible.contains("main-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn shared_ancestry_survives_hiding_a_branch() {
        // feature points at root, which master also reaches: root stays.
        let commits = vec![commit_info("main-tip", &["root"]), commit_info("root", &[])];
        let branches = vec![
            local_branch("feature", "root"),
            local_branch("master", "main-tip"),
        ];

        let visible =
            visible_commit_shas(&commits, &branches, Some("main-tip"), &hidden(&["feature"]));

        assert!(visible.contains("root"));
        assert!(visible.contains("main-tip"));
    }

    #[test]
    fn head_chain_is_visible_even_with_no_visible_branches() {
        let commits = vec![commit_info("head-tip", &["root"]), commit_info("root", &[])];
        let branches = vec![local_branch("feature", "head-tip")];

        let visible =
            visible_commit_shas(&commits, &branches, Some("head-tip"), &hidden(&["feature"]));

        assert!(visible.contains("head-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn missing_head_walks_from_branch_tips_only() {
        let commits = vec![
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
            commit_info("orphan", &[]),
        ];
        let branches = vec![local_branch("master", "main-tip")];

        let visible = visible_commit_shas(&commits, &branches, None, &BTreeSet::new());

        assert!(visible.contains("main-tip"));
        assert!(visible.contains("root"));
        assert!(!visible.contains("orphan"));
    }

    #[test]
    fn empty_hidden_set_keeps_every_loaded_commit() {
        let commits = vec![
            commit_info("feature-tip", &["root"]),
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "feature-tip"),
            local_branch("master", "main-tip"),
        ];

        let visible = visible_commit_shas(&commits, &branches, Some("main-tip"), &BTreeSet::new());

        // Every commit here is reachable from a branch tip; the function
        // returns reachable commits, not loaded commits, so the counts only
        // match because this topology has no orphans.
        assert_eq!(visible.len(), commits.len());
    }

    #[test]
    fn parents_beyond_the_loaded_boundary_are_ignored() {
        // root's parent is not loaded; the walk must terminate, not panic.
        let commits = vec![commit_info("root", &["unloaded-parent"])];
        let branches = vec![local_branch("master", "root")];

        let visible = visible_commit_shas(&commits, &branches, Some("root"), &BTreeSet::new());

        assert!(visible.contains("root"));
        assert!(!visible.contains("unloaded-parent"));
    }

    #[test]
    fn flat_branch_names_produce_flat_rows() {
        let branches = vec![local_branch("feature", "f"), local_branch("master", "m")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("feature", "f"),
                    display_name: "feature".to_string(),
                    depth: 0,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("master", "m"),
                    display_name: "master".to_string(),
                    depth: 0,
                }),
            ]
        );
    }

    #[test]
    fn slash_named_branch_nests_under_a_folder_even_when_alone() {
        let branches = vec![local_branch("features/some-feature", "tip")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "features".to_string(),
                    depth: 0,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("features/some-feature", "tip"),
                    display_name: "some-feature".to_string(),
                    depth: 1,
                }),
            ]
        );
    }

    #[test]
    fn multi_level_names_nest_one_folder_per_segment() {
        let branches = vec![local_branch("team/alice/feature-x", "tip")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "team".to_string(),
                    path: "team".to_string(),
                    depth: 0,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
                }),
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "alice".to_string(),
                    path: "team/alice".to_string(),
                    depth: 1,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("team/alice/feature-x", "tip"),
                    display_name: "feature-x".to_string(),
                    depth: 2,
                }),
            ]
        );
    }

    #[test]
    fn folders_sort_before_branches_at_each_level() {
        // Input order is alphabetical by full name, as the repo layer
        // provides it: alpha, features/x, zeta.
        let branches = vec![
            local_branch("alpha", "a"),
            local_branch("features/x", "x"),
            local_branch("zeta", "z"),
        ];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        let order = rows
            .iter()
            .map(|row| match row {
                BranchTreeRow::Folder(folder) => format!("folder:{}", folder.path),
                BranchTreeRow::Branch(branch_row) => {
                    format!("branch:{}", branch_row.branch.name)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            vec![
                "folder:features",
                "branch:features/x",
                "branch:alpha",
                "branch:zeta"
            ]
        );
    }

    #[test]
    fn collapsed_folder_emits_no_descendant_rows() {
        let branches = vec![
            local_branch("features/inner/deep", "d"),
            local_branch("features/x", "x"),
            local_branch("master", "m"),
        ];
        let collapsed = ["features"]
            .iter()
            .map(|path| path.to_string())
            .collect::<BTreeSet<_>>();

        let rows = build_branch_tree_rows(&branches, &collapsed, &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "features".to_string(),
                    depth: 0,
                    collapsed: true,
                    visibility: FolderVisibility::Visible,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("master", "m"),
                    display_name: "master".to_string(),
                    depth: 0,
                }),
            ]
        );
    }

    fn head_branch(name: &str, tip_sha: &str) -> repo::LocalBranch {
        repo::LocalBranch {
            name: name.to_string(),
            tip_sha: tip_sha.to_string(),
            is_head: true,
        }
    }

    /// Extract (path, visibility) for every folder row.
    fn folder_visibilities(rows: &[BranchTreeRow]) -> Vec<(String, FolderVisibility)> {
        rows.iter()
            .filter_map(|row| match row {
                BranchTreeRow::Folder(folder) => Some((folder.path.clone(), folder.visibility)),
                BranchTreeRow::Branch(_) => None,
            })
            .collect()
    }

    #[test]
    fn folder_visibility_derives_from_descendants() {
        let branches = vec![
            local_branch("features/a", "a"),
            local_branch("features/b", "b"),
        ];

        let none_hidden = build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&[]));
        let some_hidden =
            build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&["features/a"]));
        let all_hidden = build_branch_tree_rows(
            &branches,
            &BTreeSet::new(),
            &hidden(&["features/a", "features/b"]),
        );

        assert_eq!(
            folder_visibilities(&none_hidden),
            vec![("features".to_string(), FolderVisibility::Visible)]
        );
        assert_eq!(
            folder_visibilities(&some_hidden),
            vec![("features".to_string(), FolderVisibility::Mixed)]
        );
        assert_eq!(
            folder_visibilities(&all_hidden),
            vec![("features".to_string(), FolderVisibility::Hidden)]
        );
    }

    #[test]
    fn folder_visibility_spans_nested_subfolders() {
        // Hiding the only branch in a deep subfolder marks every ancestor
        // folder Hidden, because each ancestor's full descendant set is hidden.
        let branches = vec![local_branch("team/alice/feature-x", "tip")];

        let rows = build_branch_tree_rows(
            &branches,
            &BTreeSet::new(),
            &hidden(&["team/alice/feature-x"]),
        );

        assert_eq!(
            folder_visibilities(&rows),
            vec![
                ("team".to_string(), FolderVisibility::Hidden),
                ("team/alice".to_string(), FolderVisibility::Hidden),
            ]
        );
    }

    #[test]
    fn folder_visibility_ignores_the_head_branch() {
        let branches = vec![
            head_branch("features/current", "c"),
            local_branch("features/other", "o"),
        ];

        let nothing_hidden = build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&[]));
        let other_hidden =
            build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&["features/other"]));

        // HEAD never counts: with the only hideable branch hidden, the folder
        // reads fully hidden even though HEAD inside it stays visible.
        assert_eq!(
            folder_visibilities(&nothing_hidden),
            vec![("features".to_string(), FolderVisibility::Visible)]
        );
        assert_eq!(
            folder_visibilities(&other_hidden),
            vec![("features".to_string(), FolderVisibility::Hidden)]
        );
    }

    #[test]
    fn folder_containing_only_the_head_branch_is_visible() {
        let branches = vec![head_branch("features/current", "c")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&[]));

        assert_eq!(
            folder_visibilities(&rows),
            vec![("features".to_string(), FolderVisibility::Visible)]
        );
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

    /// Two commits on master (HEAD at the tip) plus a `feature` branch pointing
    /// at the root commit. Returns (dir, master_tip_sha, root_sha).
    fn init_repo_with_feature_branch() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let root_oid = commit_all(&repo, "Root", &[]);

        fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
        let main_tip = commit_all(&repo, "Main tip", &[root_oid]);

        let root_commit = repo.find_commit(root_oid).expect("find root commit");
        repo.branch("feature", &root_commit, false)
            .expect("create feature branch");

        drop(root_commit);
        drop(repo);
        (dir, main_tip.to_string(), root_oid.to_string())
    }

    /// Two commits on master plus a `feature` branch carrying one exclusive
    /// commit branched from the root. Returns (dir, master_tip_sha,
    /// feature_tip_sha). HEAD stays on master.
    /// Timestamps are fixed so the loaded order is deterministic: main tip, feature tip, root.
    fn init_repo_with_unmerged_branch_commit() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

        fs::write(dir.path().join("feature.txt"), "feature\n").expect("write feature file");
        let feature_tip = commit_all_to_ref_at_time(
            &repo,
            Some("refs/heads/feature"),
            "Feature work",
            &[root_oid],
            20,
        );

        fs::remove_file(dir.path().join("feature.txt")).expect("remove feature file");
        fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
        let main_tip = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main tip", &[root_oid], 30);

        drop(repo);
        (dir, main_tip.to_string(), feature_tip.to_string())
    }

    /// Two commits on master (HEAD at the tip) plus `features/alpha` carrying
    /// one exclusive commit and `features/beta` pointing at the root.
    /// Exercises sidebar folder nesting. Timestamps are fixed so the loaded
    /// order is deterministic. Returns (dir, master_tip_sha, alpha_tip_sha).
    fn init_repo_with_slash_named_branches() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

        fs::write(dir.path().join("alpha.txt"), "alpha\n").expect("write alpha file");
        let alpha_tip = commit_all_to_ref_at_time(
            &repo,
            Some("refs/heads/features/alpha"),
            "Alpha work",
            &[root_oid],
            20,
        );

        let root_commit = repo.find_commit(root_oid).expect("find root commit");
        repo.branch("features/beta", &root_commit, false)
            .expect("create features/beta");
        drop(root_commit);

        fs::remove_file(dir.path().join("alpha.txt")).expect("remove alpha file");
        fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
        let main_tip = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main tip", &[root_oid], 30);

        drop(repo);
        (dir, main_tip.to_string(), alpha_tip.to_string())
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

    /// One commit with HEAD detached and the initial branch deleted, so the
    /// repository has zero local branches.
    fn init_repo_with_detached_head_no_branches() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("solo.txt"), "solo\n").expect("write file");
        let tip_oid = commit_all(&repo, "Solo", &[]);
        repo.set_head_detached(tip_oid).expect("detach HEAD");
        let mut branch = repo
            .find_branch("master", git2::BranchType::Local)
            .expect("find master");
        branch.delete().expect("delete master");

        drop(branch);
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
                local_branches: Vec::new(),
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
            .debug_bounds("branch-row-feature")
            .expect("feature branch row renders");
        visual
            .debug_bounds("branch-row-master")
            .expect("master branch row renders");
        visual
            .debug_bounds("branch-head-marker-master")
            .expect("checked-out branch carries the HEAD marker");
        assert!(
            visual.debug_bounds("branch-head-marker-feature").is_none(),
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
            .debug_bounds("branch-row-feature")
            .expect("feature branch row renders");
        visual.simulate_click(feature_row.center(), Modifiers::none());

        // The feature branch points at the root commit, which renders at index 1
        // (newest-first: master tip, then root).
        visual
            .debug_bounds("selected-commit-row-1")
            .expect("feature tip commit becomes the selected row");
        visual
            .debug_bounds("selected-branch-row-feature")
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
            .debug_bounds("branch-row-old-base")
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
                app.toggle_branch_visibility("feature".to_string(), cx);
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
                app.toggle_branch_visibility("feature".to_string(), cx);
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
                app.toggle_branch_visibility("feature".to_string(), cx);
                assert!(app.hidden_branches.contains("feature"));
                app.toggle_branch_visibility("feature".to_string(), cx);
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
                app.toggle_branch_visibility("feature".to_string(), cx);
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
                app.toggle_branch_visibility("feature".to_string(), cx);
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
            let selector = test_debug_selector(format!("commit-ref-label-{row}-feature"));
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
                app.toggle_branch_visibility("feature".to_string(), cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
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
                        "commit-ref-label-{row}-feature"
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
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let master_row = visual
            .debug_bounds("branch-row-master")
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

        let merge_in = rows[3]
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
        assert!(
            visual.debug_bounds("commit-graph-merge-in-2-0").is_none()
                && visual
                    .debug_bounds("commit-graph-rounded-merge-in-commit-elbow-2-1")
                    .is_none(),
            "the right branch commit should sit on a straight vertical instead of bending on its own row",
        );
        let right_branch_top_vertical = visual
            .debug_bounds("commit-graph-vertical-2-1-top")
            .expect("right branch incoming vertical debug bounds");
        let right_branch_bottom_vertical = visual
            .debug_bounds("commit-graph-vertical-2-1-bottom")
            .expect("right branch outgoing vertical debug bounds");
        assert_eq!(
            right_branch_top_vertical.origin.x, right_branch_bottom_vertical.origin.x,
            "right branch edge should continue straight through its commit row",
        );
        let merge_in_source_elbow_bounds = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-3-1")
            .expect("base row merge-in source elbow debug bounds");
        let upper_merge_target_elbow_bounds = visual
            .debug_bounds("commit-graph-rounded-upper-merge-target-elbow-3-0")
            .expect("base row upper merge target elbow debug bounds");
        let base_dot_bounds = visual
            .debug_bounds("commit-graph-dot-3")
            .expect("base commit dot debug bounds");
        let base_row_bounds = visual
            .debug_bounds("commit-row-3")
            .expect("base commit row debug bounds");
        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-3-0")
                .is_none(),
            "the merge should join the trunk vertical above the dot, not tee into the dot",
        );
        assert_eq!(
            upper_merge_target_elbow_bounds.origin.y, base_row_bounds.origin.y,
            "the merge target curve should sit on the base row's upper border",
        );
        assert_eq!(
            merge_in_source_elbow_bounds.origin.y, base_row_bounds.origin.y,
            "the merge source curve should sit on the base row's upper border",
        );
        assert!(
            merge_in_source_elbow_bounds.origin.x
                > base_dot_bounds.origin.x + base_dot_bounds.size.width,
            "merge-in source elbow should curve up in the branch lane right of the base dot",
        );
        assert_eq!(
            right_branch_bottom_vertical.size.height,
            px(
                super::COMMIT_GRAPH_VERTICAL_HEIGHT - super::commit_graph_bend_radius()
                    + commit_graph_line_width()
            ),
            "the branch edge should stop at the merge curve tangent above the base row",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-vertical-3-1-bottom")
                .is_none(),
            "the branch edge should end at the base row",
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
                assert_eq!(rows[3].connector_lanes, vec![1]);
                assert!(
                    rows[3]
                        .connectors
                        .iter()
                        .all(|connector| { connector.from_lane == 1 && connector.to_lane == 1 }),
                    "the docs branch edge should run straight down its own lane",
                );
                assert_eq!(
                    rows[3].outgoing_lanes,
                    vec![0, 1, 2],
                    "both side edges should stay active until the shared parent row",
                );
                assert_eq!(rows[4].connector_lanes, vec![0, 1, 2]);
                assert!(
                    rows[4].connectors.contains(&graph::GraphConnector {
                        from_lane: 1,
                        to_lane: 0,
                        kind: GraphConnectorKind::MergeIn,
                    }) && rows[4].connectors.contains(&graph::GraphConnector {
                        from_lane: 2,
                        to_lane: 0,
                        kind: GraphConnectorKind::MergeIn,
                    }),
                    "both side edges should merge into the shared parent on its own row",
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
        assert!(
            visual
                .debug_bounds("commit-graph-rounded-merge-in-commit-elbow-3-1")
                .is_none()
                && visual
                    .debug_bounds("commit-graph-rounded-merge-target-commit-elbow-3-1")
                    .is_none(),
            "the docs commit should sit on a straight vertical instead of bending on its own row",
        );
        let docs_row_lfs_top = visual
            .debug_bounds("commit-graph-vertical-3-2-top")
            .expect("lfs edge incoming vertical through the docs row");
        let docs_row_lfs_middle = visual
            .debug_bounds("commit-graph-middle-vertical-3-2")
            .expect("lfs edge middle vertical through the docs row");
        let docs_row_lfs_bottom = visual
            .debug_bounds("commit-graph-vertical-3-2-bottom")
            .expect("lfs edge outgoing vertical through the docs row");
        assert_eq!(
            docs_row_lfs_top.origin.y + docs_row_lfs_top.size.height,
            docs_row_lfs_middle.origin.y,
            "lfs edge should pass the docs row without a gap above the middle segment",
        );
        assert_eq!(
            docs_row_lfs_middle.origin.y + docs_row_lfs_middle.size.height,
            docs_row_lfs_bottom.origin.y,
            "lfs edge should pass the docs row without a gap below the middle segment",
        );
        let docs_merge_source_elbow = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-4-1")
            .expect("docs edge should curve into the shared parent on its row");
        let lfs_merge_source_elbow = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-4-2")
            .expect("lfs edge should curve into the shared parent on its row");
        let upper_merge_target_elbow = visual
            .debug_bounds("commit-graph-rounded-upper-merge-target-elbow-4-0")
            .expect("shared parent should curve the merge into its trunk vertical");
        let lfs_crossing_underlay = visual
            .debug_bounds("commit-graph-upper-merge-crossing-4-1")
            .expect("lfs edge should keep its own horizontal underneath the docs bend");
        let parent_row = visual
            .debug_bounds("commit-row-4")
            .expect("shared parent commit row debug bounds");
        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-4-0")
                .is_none(),
            "the merges should join the trunk vertical above the dot, not tee into the dot",
        );
        assert_eq!(
            lfs_crossing_underlay.origin.y + px(super::commit_graph_line_width() / 2.),
            parent_row.origin.y,
            "the crossing merge horizontal should be centered on the shared parent row's upper border",
        );
        assert_eq!(
            docs_merge_source_elbow.origin.y, lfs_merge_source_elbow.origin.y,
            "both merge source elbows should share the same vertical placement",
        );
        assert_eq!(
            upper_merge_target_elbow.origin.y, docs_merge_source_elbow.origin.y,
            "the trunk-side merge curve should align with the branch-side curves",
        );
        assert!(
            docs_merge_source_elbow.origin.x < lfs_merge_source_elbow.origin.x,
            "the docs elbow should sit in the inner lane, the lfs elbow in the outer lane",
        );
        let docs_row_lfs_bottom_inset = visual
            .debug_bounds("commit-graph-vertical-3-2-bottom")
            .expect("lfs edge outgoing vertical above the shared parent row");
        assert_eq!(
            docs_row_lfs_bottom_inset.size.height,
            px(
                super::COMMIT_GRAPH_VERTICAL_HEIGHT - super::commit_graph_bend_radius()
                    + super::commit_graph_line_width()
            ),
            "the lfs edge should stop at its merge curve tangent above the shared parent row",
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
    async fn commit_graph_renders_unmerged_branch_tip_above_head(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                let mut head_commit = commit_info_for_graph_at("head-tip", 20, &["fork"]);
                head_commit.is_head = true;
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph_at("feature-tip", 30, &["fork"]),
                        head_commit,
                        commit_info_for_graph_at("fork", 10, &[]),
                    ],
                );
                cx.notify();
            })
            .expect("seed open repository");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let feature_dot = visual
            .debug_bounds("commit-graph-dot-0")
            .expect("unmerged branch tip renders a commit dot");
        let head_dot = visual
            .debug_bounds("commit-graph-dot-1")
            .expect("HEAD commit renders a commit dot");
        let fork_dot = visual
            .debug_bounds("commit-graph-dot-2")
            .expect("fork commit renders a commit dot");

        assert!(
            feature_dot.origin.x > head_dot.origin.x,
            "the unmerged branch tip should sit in a side lane right of HEAD's trunk lane",
        );
        assert_eq!(
            head_dot.origin.x, fork_dot.origin.x,
            "HEAD's first-parent history should keep the trunk lane",
        );
        assert!(
            visual
                .debug_bounds("commit-graph-vertical-0-0-top")
                .is_none()
                && visual
                    .debug_bounds("commit-graph-middle-vertical-0-0")
                    .is_none(),
            "the trunk lane should stay empty above the HEAD row",
        );
        visual
            .debug_bounds("commit-graph-vertical-2-1-top")
            .expect("the branch lane should run into its fork row");
    }

    #[gpui::test]
    async fn commit_graph_keeps_merge_in_horizontal_aligned_across_occupied_lanes(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let window = add_app_window(cx);

        // feature-x passes through trunk-mid's row in lane 1 while feature-y
        // merges into trunk-mid from lane 2, crossing the occupied lane.
        window
            .update(cx, |app, _window, cx| {
                seed_repo_open_mode_with_commits(
                    app,
                    dir.path().to_path_buf(),
                    vec![
                        commit_info_for_graph_at("trunk-tip", 60, &["trunk-mid"]),
                        commit_info_for_graph_at("feature-x", 50, &["trunk-base"]),
                        commit_info_for_graph_at("feature-y", 40, &["trunk-mid"]),
                        commit_info_for_graph_at("trunk-mid", 30, &["trunk-base"]),
                        commit_info_for_graph_at("trunk-base", 20, &[]),
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
                assert_eq!(rows[1].lane, 1);
                assert_eq!(rows[2].lane, 2);
                assert_eq!(rows[3].lane, 0);
                assert_eq!(rows[3].incoming_lanes, vec![0, 1, 2]);
                assert_eq!(rows[3].outgoing_lanes, vec![0, 1]);
                assert_eq!(rows[3].connector_lanes, vec![0, 1, 2]);
                assert!(rows[3].connectors.contains(&graph::GraphConnector {
                    from_lane: 2,
                    to_lane: 0,
                    kind: GraphConnectorKind::MergeIn,
                }));
            })
            .expect("inspect graph layout");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let spanning_left = visual
            .debug_bounds("commit-graph-spanning-horizontal-left-3-1")
            .expect("occupied intermediate lane left merge-in horizontal debug bounds");
        let spanning_right = visual
            .debug_bounds("commit-graph-spanning-horizontal-right-3-1")
            .expect("occupied intermediate lane right merge-in horizontal debug bounds");
        let source_elbow = visual
            .debug_bounds("commit-graph-rounded-merge-in-source-elbow-3-2")
            .expect("source-side merge-in elbow debug bounds");
        let target_elbow = visual
            .debug_bounds("commit-graph-rounded-upper-merge-target-elbow-3-0")
            .expect("trunk-side merge curve debug bounds");
        let trunk_mid_dot = visual
            .debug_bounds("commit-graph-dot-3")
            .expect("trunk-mid commit dot debug bounds");
        let trunk_mid_row = visual
            .debug_bounds("commit-row-3")
            .expect("trunk-mid commit row debug bounds");
        let occupied_lane_above = visual
            .debug_bounds("commit-graph-vertical-2-1-bottom")
            .expect("occupied lane outgoing vertical above the merge row");
        let occupied_lane_top = visual
            .debug_bounds("commit-graph-vertical-3-1-top")
            .expect("occupied lane incoming vertical through the merge row");

        assert!(
            visual
                .debug_bounds("commit-graph-merge-in-horizontal-3-0")
                .is_none(),
            "the merge should join the trunk vertical above the dot, not tee into the dot",
        );
        assert_eq!(
            spanning_left.origin.y, spanning_right.origin.y,
            "merge-in horizontal should stay aligned on both sides of the occupied lane",
        );
        assert_eq!(
            spanning_left.origin.y + px(commit_graph_line_width() / 2.),
            trunk_mid_row.origin.y,
            "merge-in horizontal should be centered on the merge row's upper border",
        );
        assert_eq!(
            target_elbow.origin.y, source_elbow.origin.y,
            "the trunk-side merge curve should align with the branch-side curve",
        );
        assert_eq!(
            occupied_lane_above.size.height,
            px(super::COMMIT_GRAPH_VERTICAL_HEIGHT),
            "the occupied pass-through lane should keep its full vertical above the merge row",
        );
        assert_eq!(
            occupied_lane_above.origin.y + occupied_lane_above.size.height,
            occupied_lane_top.origin.y,
            "the occupied lane vertical should cross the merge horizontal without interruption",
        );
        assert!(
            source_elbow.origin.x > trunk_mid_dot.origin.x,
            "the merge source elbow should curve up in the outer branch lane",
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
                        local_branches: Vec::new(),
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
                        local_branches: Vec::new(),
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

        let v_max = window
            .read_with(cx, |app, _cx| app.file_tree_scroll.max_offset())
            .expect("v max");
        let h_max = window
            .read_with(cx, |app, _cx| app.file_tree_hscroll.max_offset())
            .expect("h max");
        assert!(
            v_max.height > px(0.),
            "should overflow vertically; v_max {v_max:?}"
        );
        assert!(
            h_max.width > px(0.),
            "should overflow horizontally; h_max {h_max:?}"
        );
    }

    #[gpui::test]
    async fn file_tree_rows_are_uniform_width(cx: &mut TestAppContext) {
        use gpui::px;

        let (_window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // The folder row "deeply" (index 0) has short content. The fixture's
        // deep_dir has 7 components, so folder rows occupy indices 0-6; the first
        // changed-file row is at index 7. Rows now live in the path pane only;
        // they must be equal width and wider than the path pane's own width
        // (proving they fill the full scrolled content width, not the viewport).
        let pane = visual
            .debug_bounds("changed-files-path-pane")
            .expect("path pane bounds");
        // NOTE: "file-tree-folder-deeply" is the top-level folder at index 0.
        // "changed-file-row-7" is index 7 because deep_dir
        // ("deeply/nested/directory/structure/that/keeps/going") has exactly 7
        // path components, placing folder rows at indices 0–6 and the first file
        // row at index 7. If deep_dir in init_repo_with_deeply_nested_long_paths
        // is ever changed to a path with a different component count, this index
        // must be updated to match.
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-deeply")
            .expect("top-level folder row must be rendered");
        let file_bounds = visual
            .debug_bounds("changed-file-row-7")
            .expect("first changed-file row (index 7, after 7 folder levels) must be rendered");

        assert!(
            (folder_bounds.size.width - file_bounds.size.width).abs() < px(1.),
            "rows must be uniform width: {:?} vs {:?}",
            folder_bounds.size.width,
            file_bounds.size.width
        );
        assert!(
            folder_bounds.size.width > pane.size.width,
            "rows should fill the full scrolled width, not just the pane"
        );
    }

    #[gpui::test]
    async fn diff_stats_stay_frozen_during_horizontal_scroll(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        let before = visual
            .debug_bounds("changed-file-gutter-7")
            .expect("stat gutter cell bounds before scroll");

        window
            .update(cx, |app, _window, _cx| {
                app.file_tree_hscroll.set_offset(point(px(-200.), px(0.)));
            })
            .expect("scroll path pane horizontally");
        cx.run_until_parked();

        let after = visual
            .debug_bounds("changed-file-gutter-7")
            .expect("stat gutter cell bounds after scroll");

        assert!(
            (before.origin.x - after.origin.x).abs() < px(1.),
            "diff-stat gutter should not move horizontally; before {:?} after {:?}",
            before.origin.x,
            after.origin.x
        );

        let max = window
            .read_with(cx, |app, _cx| app.file_tree_hscroll.max_offset())
            .expect("read hscroll max");
        assert!(
            max.width > px(0.),
            "path pane should overflow horizontally; max {max:?}"
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
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("context.txt".to_string()),
                );
                assert_eq!(
                    app.file_tree_highlight_path,
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
        let repo_icon_bounds = visual
            .debug_bounds("file-tree-folder-icon-open-repo-root")
            .expect("repo root folder icon debug bounds");
        let folder_icon_bounds = visual
            .debug_bounds("file-tree-folder-icon-open-src")
            .expect("folder icon debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-outline-src")
            .expect("open folder outline debug bounds");
        let root_guide_bounds = visual
            .debug_bounds("file-tree-indent-guide-src-notes.txt-0")
            .expect("root-level indent guide debug bounds");
        let guide_bounds = visual
            .debug_bounds("file-tree-indent-guide-src-notes.txt-1")
            .expect("nested file indent guide debug bounds");
        let changed_kind_bounds = visual
            .debug_bounds("changed-file-kind-src-notes.txt")
            .expect("changed file kind marker debug bounds");
        assert_eq!(
            root_guide_bounds.origin.x + root_guide_bounds.size.width / 2.,
            repo_icon_bounds.origin.x + repo_icon_bounds.size.width / 2.,
            "root guide should be centered under the repo root icon"
        );
        assert_eq!(
            guide_bounds.origin.x + guide_bounds.size.width / 2.,
            folder_icon_bounds.origin.x + folder_icon_bounds.size.width / 2.,
            "nested guide should be centered under its parent folder icon"
        );
        assert!(
            changed_kind_bounds.origin.x - (guide_bounds.origin.x + guide_bounds.size.width)
                >= px(7.),
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

    #[gpui::test]
    async fn file_tree_shows_repo_root_header_with_inline_controls(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_nested_line_stat_changes();
        let path = dir.path().to_path_buf();
        let repo_name = path
            .file_name()
            .expect("repo dir name")
            .to_string_lossy()
            .to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let header_bounds = visual
            .debug_bounds("file-tree-repo-root")
            .expect("repo root header debug bounds");
        visual
            .debug_bounds(test_debug_selector(format!(
                "file-tree-repo-root-name-{}",
                repo_name.replace('/', "-")
            )))
            .expect("repo root name debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-repo-root")
            .expect("repo root folder icon debug bounds");

        // The controls are inline children of the header row, not a floating overlay.
        for selector in [
            "file-list-mode-toggle",
            "file-tree-collapse-all",
            "file-tree-expand-all",
        ] {
            let control_bounds = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing control bounds for {selector}"));
            assert!(
                header_bounds.contains(&control_bounds.center()),
                "{selector} should sit inside the repo root header row"
            );
        }

        // The first file row's diff stats are clear of the controls.
        let stat_bounds = visual
            .debug_bounds("changed-file-diff-stat-src-notes.txt")
            .expect("diff stat debug bounds");
        let toggle_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("toggle debug bounds");
        assert!(
            !stat_bounds.intersects(&toggle_bounds),
            "diff stats must not collide with the tree controls"
        );
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
                app.open_file_preview("long.txt".to_string(), cx);
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
                app.open_file_preview("long.txt".to_string(), cx);
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
                app.open_file_preview("long.txt".to_string(), cx);
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

    #[gpui::test]
    async fn slash_named_branches_render_under_a_folder_row(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder = visual
            .debug_bounds("branch-folder-features")
            .expect("slash-named branches render a folder row");
        let alpha = visual
            .debug_bounds("branch-row-features-alpha")
            .expect("nested branch row renders, keyed by full name");
        let beta = visual
            .debug_bounds("branch-row-features-beta")
            .expect("sibling nested branch row renders");
        let master = visual
            .debug_bounds("branch-row-master")
            .expect("flat branch row renders");
        assert!(
            folder.origin.y < alpha.origin.y
                && alpha.origin.y < beta.origin.y
                && beta.origin.y < master.origin.y,
            "row order must be: folder, alpha, beta, master"
        );
    }

    #[gpui::test]
    async fn clicking_a_folder_row_collapses_and_expands_it(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder = visual
            .debug_bounds("branch-folder-features")
            .expect("folder row renders");
        visual.simulate_click(folder.center(), Modifiers::none());

        // Verify app state after the click.
        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.is_empty(),
                    "collapsing is visual only; no branch becomes hidden"
                );
                assert!(
                    app.collapsed_branch_folders.contains("features"),
                    "features must be in collapsed_branch_folders after click"
                );
                // Verify that the tree builder produces the collapsed layout.
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let rows = build_branch_tree_rows(
                    &repo.local_branches,
                    &app.collapsed_branch_folders,
                    &app.hidden_branches,
                );
                assert_eq!(
                    rows.len(),
                    2,
                    "collapsed folder must hide descendant rows; rows: {:?}",
                    rows
                );
                assert!(
                    matches!(&rows[0], BranchTreeRow::Folder(f) if f.collapsed),
                    "first row is the collapsed folder"
                );
                assert!(
                    matches!(&rows[1], BranchTreeRow::Branch(b) if b.branch.name == "master"),
                    "second row is the master branch"
                );
            })
            .expect("read state after collapse");

        // Verify the folder row is still rendered (collapsed, not removed).
        visual
            .debug_bounds("branch-folder-features")
            .expect("collapsed folder row still renders");

        // Click again to expand.
        let folder = visual
            .debug_bounds("branch-folder-features")
            .expect("collapsed folder row still renders for second click");
        visual.simulate_click(folder.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "second click must expand the folder"
                );
            })
            .expect("read state after expand");
    }

    #[gpui::test]
    async fn reopening_a_repository_expands_all_folders(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.toggle_branch_folder("features".to_string(), cx);
                assert!(app.collapsed_branch_folders.contains("features"));

                app.open_repository_at(path, window, cx);
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "reopening must reset collapse state"
                );
            })
            .expect("open, collapse, reopen");
    }

    /// Like `init_repo_with_slash_named_branches`, but HEAD is moved onto
    /// `features/alpha`, so a sidebar folder contains the checked-out branch.
    fn init_repo_with_head_inside_folder() -> (tempfile::TempDir, String) {
        let (dir, _master_tip, alpha_tip) = init_repo_with_slash_named_branches();
        let repo = Repository::open(dir.path()).expect("open repo");
        repo.set_head("refs/heads/features/alpha")
            .expect("set HEAD");
        drop(repo);
        (dir, alpha_tip)
    }

    #[gpui::test]
    async fn folder_visibility_toggle_hides_then_shows_all_descendants(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);

                app.toggle_folder_visibility("features", cx);
                assert!(app.hidden_branches.contains("features/alpha"));
                assert!(app.hidden_branches.contains("features/beta"));

                app.toggle_folder_visibility("features", cx);
                assert!(
                    app.hidden_branches.is_empty(),
                    "toggling a fully hidden folder must show every descendant"
                );
            })
            .expect("toggle folder visibility twice");
    }

    #[gpui::test]
    async fn folder_visibility_toggle_hides_the_remainder_when_mixed(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("features/alpha".to_string(), cx);

                app.toggle_folder_visibility("features", cx);
                assert!(
                    app.hidden_branches.contains("features/alpha")
                        && app.hidden_branches.contains("features/beta"),
                    "a mixed folder toggle must hide the remaining visible branches"
                );
            })
            .expect("toggle mixed folder");
    }

    #[gpui::test]
    async fn folder_visibility_toggle_skips_the_head_branch(cx: &mut TestAppContext) {
        let (dir, _alpha_tip) = init_repo_with_head_inside_folder();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);

                app.toggle_folder_visibility("features", cx);
                assert!(
                    !app.hidden_branches.contains("features/alpha"),
                    "the checked-out branch must never be hidden"
                );
                assert!(app.hidden_branches.contains("features/beta"));

                app.toggle_folder_visibility("features", cx);
                assert!(app.hidden_branches.is_empty());
            })
            .expect("toggle folder containing HEAD");
    }

    #[gpui::test]
    async fn hiding_a_folder_clears_a_selection_inside_it(cx: &mut TestAppContext) {
        let (dir, _master_tip, alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.selection = Selection::Single {
                    sha: alpha_tip.clone(),
                };

                app.toggle_folder_visibility("features", cx);
                assert_eq!(
                    app.selection,
                    Selection::None,
                    "hiding the folder removed the selected commit, so the selection clears"
                );
            })
            .expect("hide folder containing the selection");
    }

    #[gpui::test]
    async fn clicking_a_folder_eye_hides_every_descendant_branch(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // The toggle is hover-revealed; tests drive the hover state
                // directly. The features folder is row 0.
                app.hovered_branch_row = Some(0);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-folder-visibility-features")
            .expect("hovered folder row reveals its visibility toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.contains("features/alpha")
                        && app.hidden_branches.contains("features/beta"),
                    "folder toggle click must hide every descendant branch"
                );
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "the toggle click must not also collapse the folder"
                );
            })
            .expect("verify post-click state");
    }

    #[gpui::test]
    async fn fully_hidden_folder_keeps_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_folder_visibility("features", cx);
            })
            .expect("open repository and hide the folder");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-visibility-features")
            .expect("a fully hidden folder keeps its toggle visible without hover");
    }

    #[gpui::test]
    async fn partially_hidden_folder_keeps_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("features/alpha".to_string(), cx);
            })
            .expect("open repository and hide one branch");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-visibility-features")
            .expect("a mixed folder keeps its toggle visible without hover");
    }

    #[gpui::test]
    async fn clicking_the_eye_icon_hides_the_branch(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // The toggle is hover-revealed; tests drive the hover state
                // directly. Branches sort alphabetically: feature is row 0.
                app.hovered_branch_row = Some(0);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-visibility-feature")
            .expect("hovered branch row reveals its visibility toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        // Verify clicking the toggle called toggle_branch_visibility:
        // (a) the branch is now in hidden_branches, and
        // (b) the visible commit count dropped from 3 to 2.
        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.contains("feature"),
                    "visibility toggle click must add branch to hidden_branches"
                );
                assert_eq!(
                    app.selection,
                    Selection::None,
                    "visibility toggle click must not focus the branch"
                );
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let head_sha = repo
                    .commits
                    .iter()
                    .find(|c| c.is_head)
                    .map(|c| c.sha.as_str());
                let visible = visible_commit_shas(
                    &repo.commits,
                    &repo.local_branches,
                    head_sha,
                    &app.hidden_branches,
                );
                assert_eq!(
                    visible.len(),
                    2,
                    "feature-exclusive commit must be absent from visible set"
                );
            })
            .expect("verify post-click state");
    }

    #[gpui::test]
    async fn hidden_branch_shows_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-visibility-feature")
            .expect("hidden branch keeps its toggle visible without hover");
    }

    #[gpui::test]
    async fn head_branch_row_renders_no_visibility_toggle(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // master is the HEAD branch; alphabetically it is row 1.
                app.hovered_branch_row = Some(1);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("branch-visibility-master").is_none(),
            "the HEAD branch must not offer a visibility toggle"
        );
    }

    #[gpui::test]
    async fn clicking_a_hidden_branch_row_does_not_focus_it(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("branch-row-feature")
            .expect("hidden branch row still renders");
        let toggle = visual
            .debug_bounds("branch-visibility-feature")
            .expect("hidden branch renders its always-visible toggle");
        assert!(
            row.origin.x + px(8.) < toggle.origin.x,
            "left-edge click point must fall outside the toggle icon"
        );
        // Click near the left edge so the click cannot land on the
        // always-visible eye-off icon at the row's right edge.
        visual.simulate_click(
            point(row.origin.x + px(8.), row.center().y),
            Modifiers::none(),
        );

        window
            .update(cx, |app, _window, _cx| {
                assert_eq!(app.selection, Selection::None);
            })
            .expect("read selection");
    }
}
