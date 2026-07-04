//! Top-level application entity and root view.

mod branch_filter;
mod branch_sidebar;
mod commit_graph;
pub(crate) mod diff_selection;
mod diff_view;
mod file_tree;
pub mod menu;
pub mod path_picker;
#[cfg(test)]
mod test_support;
mod title_bar;

use self::branch_filter::*;
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
    actions, canvas, div, pattern_slash, point, px, uniform_list, AnyElement, AppContext,
    Background, Bounds, ClickEvent, Context, Entity, EventEmitter, FocusHandle, HighlightStyle,
    Hsla, InteractiveElement, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, PathBuilder, Pixels, Point, Render, ScrollHandle,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, StyledText, TextStyle,
    UniformListScrollHandle, Window,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::notification::{Notification, NotificationList};
use gpui_component::resizable::{h_resizable, resizable_panel, ResizableState};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::tooltip::Tooltip;
use gpui_component::Icon;
use similar::{DiffTag, TextDiff};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::icons::LucideIcon;
use crate::settings::{self, RecentRepository, Settings, SidebarWidths, MAX_RECENT_REPOSITORIES};
use crate::theme::palette;
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
        CloseActivePane,
        NextChangeBlock,
        PreviousChangeBlock
    ]
);

actions!(
    app,
    [
        DiffMoveLeft,
        DiffMoveRight,
        DiffMoveUp,
        DiffMoveDown,
        DiffMoveWordLeft,
        DiffMoveWordRight,
        DiffMoveLineStart,
        DiffMoveLineEnd,
        DiffMoveDocStart,
        DiffMoveDocEnd,
        DiffSelectLeft,
        DiffSelectRight,
        DiffSelectUp,
        DiffSelectDown,
        DiffSelectWordLeft,
        DiffSelectWordRight,
        DiffSelectLineStart,
        DiffSelectLineEnd,
        DiffSelectDocStart,
        DiffSelectDocEnd,
        DiffSelectAll,
        DiffCopy,
        DiffCancelSelection
    ]
);

/// The application-wide monospace font, used by every text surface (file tree,
/// branch sidebar, tab bar, diff view, and commit graph) so they render
/// consistently. The value is the installed family name of the Nerd Font
/// variant; see the resolution test in `file_tree` for the no-fallback contract.
pub(crate) const MONO_FONT_FAMILY: &str = "BerkeleyMono Nerd Font";
/// Interval between diff-caret blink phase flips.
const CARET_BLINK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
const FILE_TREE_INDENT_WIDTH: f32 = 16.;
const FILE_TREE_ROW_HEIGHT: f32 = 24.;
/// Vertical breathing room added inside each branch-sidebar row, within its
/// border and background so adjacent rows stay flush instead of showing a gap.
/// gpui lays out border-box, so the row height carries this padding: the box
/// grows by twice this value while the content area stays `FILE_TREE_ROW_HEIGHT`.
const BRANCH_ROW_VERTICAL_PADDING: f32 = 1.;
const FILE_TREE_TEXT_SIZE: f32 = 14.;
const FILE_TREE_ROW_TEXT_LINE_HEIGHT: f32 = 20.;
const FILE_TREE_SECONDARY_TEXT_SIZE: f32 = 10.;
const FILE_TREE_BADGE_TEXT_SIZE: f32 = 9.;
const FILE_TREE_DIFF_STAT_TEXT_SIZE: f32 = 13.;
const FILE_TREE_FOLDER_ICON_SIZE: f32 = 16.;
const FILE_TREE_STATUS_ICON_SIZE: f32 = 14.;
/// Leading branch-glyph size on sidebar branch rows: a touch smaller than the
/// shared status-icon size so the icon sits lighter beside the branch name.
const BRANCH_ROW_ICON_SIZE: f32 = FILE_TREE_STATUS_ICON_SIZE * 0.9;
const FILE_TREE_INDENT_GUIDE_WIDTH: f32 = 1.;
const FILE_TREE_GUIDE_TO_ITEM_GAP: f32 = 4.;
const FILE_TREE_CONTROL_BUTTON_SIZE: f32 = 22.;
const FILE_TREE_CONTROL_ICON_SIZE: f32 = 15.;
/// Inset between the control-button group and the container's top and right
/// edges. The header derives its height from this so the group stays
/// equidistant from both edges.
const FILE_TREE_HEADER_INSET: f32 = 8.;
const FILE_TREE_HEADER_HEIGHT: f32 = FILE_TREE_CONTROL_BUTTON_SIZE + 2. * FILE_TREE_HEADER_INSET;
const FILE_TREE_DIFF_STAT_WIDTH: f32 = 68.;
const FILE_TREE_STAT_GUTTER_WIDTH: f32 = 84.; // diff-stat width + horizontal cell padding
const BRANCH_SIDEBAR_DEFAULT_WIDTH: f32 = 240.;
/// Height of the graph's contextual selection bar (docked to the top of
/// the history panel while a selection is active).
const SELECTION_BAR_HEIGHT: f32 = 34.;
const CHANGESET_FILES_DEFAULT_WIDTH: f32 = 340.;
/// Smallest width a restored sidebar may take, guarding against a corrupt or
/// degenerate saved value. The resizable widget clamps further to its own
/// panel minimum and the container size.
const SIDEBAR_MIN_WIDTH: f32 = 120.;

/// Cache of read-only (unchanged-file) line cells, keyed by file path and the
/// commit sha the content was read at. See the `read_only_cell_cache` field.
type ReadOnlyCellCache = RefCell<HashMap<(String, String), Rc<Vec<diff_view::DiffLineCell>>>>;

pub struct App {
    pub mode: Mode,
    pub selection: Selection,
    /// The commit under the most recent single click in the graph. A
    /// double-click's first click can add or remove the selection bar above
    /// the graph, shifting every row mid-gesture, so the second click may
    /// land on a neighboring row or on the bar itself. gpui only reports
    /// `click_count >= 2` when both clicks land at nearly the same point, so
    /// this anchor is exactly the commit the user double-clicked. Cleared by
    /// a single click on the bar (the only other surface that consumes it).
    double_click_anchor: Option<String>,
    pub review_screen: ReviewScreen,
    /// While a comparison changeset is open, the commits a merge of the
    /// target into the base would introduce (base..target), newest first.
    /// Computed once when the changeset opens — the title-bar popover renders
    /// on every notify and must not re-walk history. None for single/range
    /// changesets and outside review mode.
    comparison_commit_shas: Option<Vec<String>>,
    /// Open diff tabs. Source of truth for what the detail area shows.
    pub workspace: crate::workspace::Workspace,
    /// Last file row the user clicked. Drives the tree highlight only; tab
    /// activation deliberately does not move it (spec: tree is click-driven).
    pub file_tree_highlight_path: Option<String>,
    /// Scroll handles per pane (tab strip + diff sides), created on demand.
    /// RefCell because render paths take `&self`; ScrollHandle clones share
    /// their underlying state, so handing out clones is safe.
    pane_scrolls: RefCell<HashMap<crate::workspace::PaneId, PaneScrollState>>,
    /// The selection (or bare caret) on one side of one open tab's diff, keyed
    /// by pane id and the tab's key (file path). Pruned whenever a workspace
    /// mutation can close or replace tabs, so an entry never outlives the tab
    /// it was made on.
    diff_selections: HashMap<(crate::workspace::PaneId, String), diff_selection::DiffSelection>,
    /// An in-flight mouse selection drag, if the user is currently dragging
    /// inside a diff. Lives from mouse-down to mouse-up.
    diff_drag: Option<DiffDrag>,
    /// Current blink phase for the diff caret: `true` paints it, `false`
    /// hides it. Read into `DiffSelectionContext::caret_visible` on every
    /// render. Starts `true`; the blink loop only starts once a caret is
    /// actually placed (see `pause_caret_blink`).
    caret_blink_visible: bool,
    /// Epoch guard for the blink loop: each armed timer captures the epoch
    /// it was scheduled under, and a fired timer whose epoch no longer
    /// matches `caret_blink_epoch` is stale and does nothing. Bumped by
    /// every `pause_caret_blink`/`blink_caret` call, so a fresh caret
    /// placement orphans whatever timer was previously in flight.
    caret_blink_epoch: usize,
    /// Computed diff rows for the changed-file detail view, keyed by file path
    /// plus the commit/base shas they were derived from. `render_changed_file_detail`
    /// runs on every App render; without this cache it would re-read the file from
    /// git and recompute the line diff each time. Cleared whenever the changeset
    /// selection changes (see `open_changeset`/`apply_open_repository`), so entries
    /// never outlive the changeset they describe. `RefCell` because render paths
    /// take `&self`; the `Rc` lets a cache hit hand back the rows without cloning.
    diff_row_cache: RefCell<HashMap<DiffCacheKey, Rc<PreparedFileDiff>>>,
    /// Line cells for read-only (unchanged-file) tabs, keyed by file path and
    /// the commit sha the content was read at. Keyboard selection resolves
    /// the active tab's content on every keystroke (twice, counting the
    /// caret's scroll-into-view); without this cache each resolution would
    /// re-read the file's blob from git. Cleared alongside `diff_row_cache`,
    /// so entries never outlive the changeset they were read for.
    read_only_cell_cache: ReadOnlyCellCache,
    /// While a tab drag hovers a pane's edge zone, the pane and the split
    /// direction its half-highlight previews. None when no edge is hovered.
    pub(crate) tab_drop_zone: Option<(crate::workspace::PaneId, crate::workspace::SplitDirection)>,
    pub file_list_mode: FileListMode,
    pub settings: Settings,
    collapsed_file_tree_paths: BTreeSet<String>,
    /// All AI threads and their subprocesses (ADR-0005); killed on changeset
    /// close and app quit so no `claude` process outlives its context.
    ai_sessions: Entity<crate::ai::AiSessions>,
    notifications: Entity<NotificationList>,
    path_picker: Box<dyn PathPicker>,
    settings_store_path: Option<PathBuf>,
    /// Memoized DAG layout; see [`GraphLayout`]. `RefCell` because render takes
    /// `&self`, mirroring `diff_row_cache`.
    graph_layout_cache: RefCell<Option<(GraphLayoutSignature, Rc<GraphLayout>)>>,
    /// Test-only counter proving the layout is reused across renders.
    #[cfg(test)]
    graph_layout_recompute_count: std::cell::Cell<u64>,
    commit_history_scroll: UniformListScrollHandle,
    file_tree_scroll: ScrollHandle,
    /// Horizontal scroll handle for the path pane only; the stat gutter stays
    /// fixed while this scrolls.
    file_tree_hscroll: ScrollHandle,
    /// True while the cursor is anywhere over the file-tree panel; gates the
    /// hover-revealed scrollbar overlay.
    file_tree_hovered: bool,
    /// Which workspace pane's diff content the pointer is over, if any.
    /// Drives that pane's hover-revealed horizontal scrollbar overlay,
    /// mirroring the file tree's hover-revealed scrollbars.
    pub(crate) hovered_diff_pane: Option<crate::workspace::PaneId>,
    changeset_resizable: Entity<ResizableState>,
    graph_resizable: Entity<ResizableState>,
    branch_sidebar_scroll: UniformListScrollHandle,
    /// Bumped whenever `repo.branches` is (re)loaded, so the sidebar row cache
    /// can detect a branch-set change without comparing the branch list.
    branches_generation: std::cell::Cell<u64>,
    /// Memoized flat sidebar row model; `RefCell` because render takes `&self`.
    sidebar_rows_cache: RefCell<Option<(SidebarRowsSignature, Rc<Vec<BranchTreeRow>>)>>,
    #[cfg(test)]
    sidebar_rows_recompute_count: std::cell::Cell<u64>,
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
    /// Sidebar section keys (`heads` / `remotes`) the user has collapsed.
    /// Session-only: cleared whenever a repository is opened. Sections default
    /// to expanded, so an empty set means both sections show their branches.
    collapsed_branch_sections: BTreeSet<String>,
    /// The branch-sidebar filter query field. Session-only: its value is
    /// cleared when a repository is opened. Purely a sidebar view filter — it
    /// never changes graph contents or the commit selection.
    filter_input: Entity<InputState>,
    focus_handle: FocusHandle,
    /// Whether the title-bar context popover (the diff "switcher") is open.
    context_popover_open: bool,
    /// Whether the title-bar repo switcher (sibling-repository list) is open.
    repo_switcher_open: bool,
    /// Roll-up of the pending working-tree state, rendered as the graph's
    /// synthetic top row. Recomputed by `refresh_pending_summary` on
    /// repository open (and, in a later slice, window activation and
    /// changeset close); no filesystem watcher in v1.
    pub(crate) pending_summary: repo::PendingSummary,
}

#[derive(Clone)]
pub(crate) struct FileDiffScroll {
    old: UniformListScrollHandle,
    new: UniformListScrollHandle,
    side_by_side: UniformListScrollHandle,
    /// One horizontal pan offset for every code cell this pane's diff shows,
    /// shared across rows and — in a side-by-side diff — across both sides,
    /// so the whole surface pans in lockstep. The gutter cells do not track
    /// it, which is what keeps them frozen.
    hscroll: ScrollHandle,
    /// Set when the shown file changes (via `reset`); consumed on the next
    /// render to scroll the diff to its first change block exactly once per
    /// open. Shared across clones so every reference to a pane's scroll sees
    /// the same pending state.
    pending_focus: Rc<Cell<bool>>,
}

/// One pane's scroll handles: the tab strip plus the diff content sides.
#[derive(Clone)]
pub(crate) struct PaneScrollState {
    pub(crate) tab_bar: ScrollHandle,
    pub(crate) diff: FileDiffScroll,
    /// Keyboard focus for this pane's diff selection. Mouse and keyboard
    /// selection handlers focus this handle so key events route to the pane
    /// the user is interacting with, and the caret paints only while the
    /// pane holds it.
    pub(crate) focus: FocusHandle,
    /// Last painted bounds of each side's list container, keyed by side
    /// selector (e.g. "file-diff-side-old"). Written during paint by the
    /// bounds-capturing canvas in `render_file_diff_side` and read back to
    /// translate pointer positions into diff coordinates.
    pub(crate) content_origins: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
}

impl PaneScrollState {
    fn new(cx: &gpui::App) -> Self {
        Self {
            tab_bar: ScrollHandle::new(),
            diff: FileDiffScroll::new(),
            focus: cx.focus_handle(),
            content_origins: Rc::new(RefCell::new(HashMap::new())),
        }
    }
}

/// The render-time context the `render_*_file_detail` chain needs beyond
/// `repo`/`changeset`/the selected path: which pane is rendering, its diff
/// scroll state, and whether the pointer is currently over it. Bundled so
/// those functions stay under clippy's argument-count limit.
pub(crate) struct PaneRenderContext<'a> {
    pub(crate) pane: crate::workspace::PaneId,
    pub(crate) scroll: &'a FileDiffScroll,
    pub(crate) hovered: bool,
}

/// An in-flight mouse selection drag. Lives from mouse-down to mouse-up.
pub(crate) struct DiffDrag {
    pub(crate) pane: crate::workspace::PaneId,
    pub(crate) key: String,
    pub(crate) mode: DiffDragMode,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DiffDragMode {
    Character,
    /// Original double-clicked word; drag unions the pointer's word with it.
    Word {
        origin: (diff_selection::DiffPoint, diff_selection::DiffPoint),
    },
    /// Original clicked line range; drag unions the pointer's line with it.
    Line {
        origin: (diff_selection::DiffPoint, diff_selection::DiffPoint),
    },
}

/// Shared union logic for `Word`/`Line` drag extension: the selection spans
/// the union of `origin` and `pointer`'s ranges. The head sits on whichever
/// end of that union the pointer's range extended toward, so the caret keeps
/// tracking the mouse; the anchor sits at the opposite (farther) end,
/// pinning the drag origin. Whether the pointer dragged backward past the
/// origin is decided by comparing the pointer's range against the origin's
/// range directly (not the raw click point against the union), since on a
/// same-row drag the click point rarely falls exactly on the union bound.
fn union_range_selection(
    side: repo::DiffSide,
    origin: (diff_selection::DiffPoint, diff_selection::DiffPoint),
    pointer: (diff_selection::DiffPoint, diff_selection::DiffPoint),
) -> diff_selection::DiffSelection {
    let start = origin.0.min(pointer.0);
    let end = origin.1.max(pointer.1);
    let (anchor, head) = if pointer.0 < origin.0 {
        (end, start)
    } else {
        (start, end)
    };
    diff_selection::DiffSelection {
        side,
        anchor,
        head,
        goal_x: None,
    }
}

impl FileDiffScroll {
    fn new() -> Self {
        Self {
            old: UniformListScrollHandle::new(),
            new: UniformListScrollHandle::new(),
            side_by_side: UniformListScrollHandle::new(),
            hscroll: ScrollHandle::new(),
            pending_focus: Rc::new(Cell::new(false)),
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
        self.hscroll.set_offset(origin);
        // A newly shown diff should land on its first change; the next render
        // consumes this to scroll there.
        self.pending_focus.set(true);
    }

    /// Take the pending-focus flag, returning whether a first-change scroll is
    /// owed and clearing it so it fires only once per open.
    fn take_pending_focus(&self) -> bool {
        self.pending_focus.replace(false)
    }

    #[cfg(test)]
    fn side_by_side_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.side_by_side.0.borrow().base_handle.offset()
    }

    #[cfg(test)]
    fn hscroll_offset(&self) -> gpui::Point<gpui::Pixels> {
        self.hscroll.offset()
    }

    #[cfg(test)]
    fn side_by_side_max_offset(&self) -> gpui::Size<gpui::Pixels> {
        self.side_by_side.0.borrow().base_handle.max_offset()
    }
}

/// Memoized commit-graph layout. Recomputed only when its signature changes,
/// so scroll, selection, and hover no longer pay the O(n^2) DAG relayout.
pub(crate) struct GraphLayout {
    pub(crate) rows: Vec<graph::GraphRow>,
    pub(crate) max_lanes: usize,
}

/// Inputs that determine the graph layout. Cheap to build and compare: the
/// loaded-commit count (history only ever grows by paging within a session),
/// the HEAD sha, and the hidden-branch set.
#[derive(PartialEq, Eq)]
struct GraphLayoutSignature {
    commit_count: usize,
    head_sha: Option<String>,
    hidden_branches: BTreeSet<String>,
}

/// Inputs that determine the flat branch-sidebar row model. Recomputed only
/// when the branch set, collapse state, or hidden-branch set changes.
#[derive(PartialEq, Eq)]
struct SidebarRowsSignature {
    branches_generation: u64,
    collapsed_folders: BTreeSet<String>,
    collapsed_sections: BTreeSet<String>,
    hidden_branches: BTreeSet<String>,
    query: String,
}

pub enum Mode {
    NoRepo,
    RepoOpen { repo: repo::OpenRepository },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    /// The synthetic pending-changes item. Only viewable on its own: range
    /// and comparison gestures to or from it are rejected.
    Pending,
    Single {
        sha: String,
    },
    Range {
        start_sha: String,
        end_sha: String,
        shas: Vec<String>,
    },
    /// A merge preview between two commits that need not share a linear
    /// ancestry: the changeset is what merging `target_sha` into `base_sha`
    /// would introduce (diff from their merge base to the target).
    Compare {
        base_sha: String,
        target_sha: String,
    },
}

/// First seven characters of a full commit sha — the short form shown across
/// the graph, selection bar, and title bar.
pub(crate) fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// The graph's default selection: the checked-out (HEAD) commit when one is
/// loaded, otherwise the newest visible commit. `Selection::None` only when
/// the graph shows no commits at all — the graph is meant to always carry a
/// selection, so the selection bar is a permanent fixture rather than an
/// occasional overlay.
fn default_selection(repo: &repo::OpenRepository, hidden_branches: &BTreeSet<String>) -> Selection {
    let visible = visible_commits(repo, hidden_branches);
    visible
        .iter()
        .find(|commit| commit.is_head)
        .or_else(|| visible.first())
        .map(|commit| Selection::Single {
            sha: commit.sha.clone(),
        })
        .unwrap_or(Selection::None)
}

/// Human-readable label for the graph's selection bar: how many commits the
/// current selection covers, or `None` when nothing is selected (the bar does
/// not render at all in that case).
fn selection_summary(selection: &Selection) -> Option<String> {
    match selection {
        Selection::None => None,
        Selection::Pending => Some("Pending changes selected".to_string()),
        Selection::Single { .. } => Some("1 commit selected".to_string()),
        Selection::Range { shas, .. } => Some(format!("{} commits selected", shas.len())),
        Selection::Compare {
            base_sha,
            target_sha,
        } => Some(format!(
            "Merge preview: {} into {}",
            short_sha(target_sha),
            short_sha(base_sha)
        )),
    }
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

/// Header introducing the Local or Remote half of the sidebar. Clicking it
/// collapses or expands the whole section; the header shows a section icon and
/// a count of the branches it contains.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchSectionRow {
    /// User-facing label, e.g. "Local" or "Remote".
    title: String,
    /// Ref-namespace key (`heads` / `remotes`); keys collapse state and
    /// selects the section icon.
    key: String,
    /// Number of branches the section contains (leaf refs, ignoring hidden and
    /// collapse state).
    count: usize,
    /// Whether the section is collapsed, hiding its descendant rows.
    collapsed: bool,
    /// Whether the header draws a separating top border. True only when the row
    /// directly above it is content (a branch or folder), never when it follows
    /// another header or heads the list — so stacked headers and the topmost
    /// section don't double the sidebar's own border.
    top_border: bool,
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

    /// Construct the app from settings already loaded by the caller (the app
    /// entry point loads them before opening the window so the saved geometry
    /// can be applied). Avoids a second read of the settings file.
    pub fn new_with_loaded_settings(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: Settings,
        settings_store_path: Option<PathBuf>,
    ) -> Self {
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
            window_state: None,
            sidebar_widths: SidebarWidths::default(),
            ai_enabled: false,
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

    #[cfg(test)]
    pub(crate) fn new_with_settings(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings: Settings,
    ) -> Self {
        Self::new_with_picker_settings_and_store_path(
            window,
            cx,
            Box::new(GpuiPathPicker),
            settings,
            None,
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
        let ai_sessions = cx.new(|_| crate::ai::AiSessions::new());
        let changeset_resizable = cx.new(|_| ResizableState::default());
        let graph_resizable = cx.new(|_| ResizableState::default());
        let focus_handle = cx.focus_handle();

        let filter_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search branches…")
                .clean_on_escape()
        });
        // Re-render the sidebar whenever the query changes so the filter
        // re-applies; the input owns its own state, App only observes it.
        cx.subscribe(&filter_input, |_app, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

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

        // Persist window geometry and sidebar widths when the window closes.
        // Two hooks cover the two macOS quit paths, which never both fire for a
        // single window:
        //   * close button / Cmd+W -> windowShouldClose
        //   * Cmd+Q / in-app Quit  -> applicationWillTerminate (on_app_quit)
        // No in-memory cache: each hook reads the live geometry and split widths
        // at the moment it fires.
        let should_close_entity = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            should_close_entity
                .update(cx, |app, cx| app.persist_session_state(window, cx))
                .ok();
            true
        });

        // Reach the window and this `App` WITHOUT assuming the window's root
        // view type. The window is rooted at a `gpui_component::Root` wrapper
        // (see `run` in lib.rs), so `handle.downcast::<App>()` would always be
        // `None` and this quit-time save would silently never run. Instead take
        // the live `Window` from the untyped handle and persist through a weak
        // handle to this entity, exactly as the close-button hook above does.
        // Do NOT replace this with `downcast::<Self>()`.
        let quit_entity = cx.entity().downgrade();
        gpui::App::on_app_quit(cx, move |cx| {
            for handle in cx.windows() {
                handle
                    .update(cx, |_root_view, window, cx| {
                        quit_entity
                            .update(cx, |app, cx| app.persist_session_state(window, cx))
                            .ok();
                    })
                    .ok();
            }
            async move {}
        })
        .detach();

        // The pending summary is event-driven, not watched: recompute it when
        // the window regains focus so edits made in another app show up.
        cx.observe_window_activation(window, |app, window, cx| {
            if window.is_window_active() {
                let before = app.pending_summary;
                app.refresh_pending_summary();
                if app.pending_summary != before {
                    cx.notify();
                }
            }
        })
        .detach();

        // No branches are hidden at construction, so the default selection is
        // computed against the fully visible graph.
        let selection = match &mode {
            Mode::RepoOpen { repo } => default_selection(repo, &BTreeSet::new()),
            Mode::NoRepo => Selection::None,
        };
        // Read errors degrade to a clean summary rather than surfacing; see
        // `refresh_pending_summary`, which this construction-time read mirrors.
        let pending_summary = match &mode {
            Mode::RepoOpen { repo } => repo::read_pending_summary(&repo.path).unwrap_or_default(),
            Mode::NoRepo => repo::PendingSummary::default(),
        };

        Self {
            mode,
            selection,
            double_click_anchor: None,
            review_screen: ReviewScreen::Graph,
            comparison_commit_shas: None,
            workspace: crate::workspace::Workspace::new(),
            file_tree_highlight_path: None,
            pane_scrolls: RefCell::new(HashMap::new()),
            diff_selections: HashMap::new(),
            caret_blink_visible: true,
            caret_blink_epoch: 0,
            diff_drag: None,
            diff_row_cache: RefCell::new(HashMap::new()),
            read_only_cell_cache: RefCell::new(HashMap::new()),
            tab_drop_zone: None,
            file_list_mode: FileListMode::Changed,
            settings,
            collapsed_file_tree_paths: BTreeSet::new(),
            ai_sessions,
            notifications,
            path_picker,
            settings_store_path,
            graph_layout_cache: RefCell::new(None),
            #[cfg(test)]
            graph_layout_recompute_count: std::cell::Cell::new(0),
            commit_history_scroll: UniformListScrollHandle::new(),
            file_tree_scroll: ScrollHandle::new(),
            file_tree_hscroll: ScrollHandle::new(),
            file_tree_hovered: false,
            hovered_diff_pane: None,
            changeset_resizable,
            graph_resizable,
            branch_sidebar_scroll: UniformListScrollHandle::new(),
            branches_generation: std::cell::Cell::new(0),
            sidebar_rows_cache: RefCell::new(None),
            #[cfg(test)]
            sidebar_rows_recompute_count: std::cell::Cell::new(0),
            branch_sidebar_hovered: false,
            hidden_branches: BTreeSet::new(),
            collapsed_branch_folders: BTreeSet::new(),
            collapsed_branch_sections: BTreeSet::new(),
            filter_input,
            focus_handle,
            context_popover_open: false,
            repo_switcher_open: false,
            pending_summary,
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

        // Hidden branches are reset below, so the default selection is
        // computed against the fully visible graph.
        let selection = default_selection(&repo, &BTreeSet::new());
        self.mode = Mode::RepoOpen { repo };
        self.selection = selection;
        self.refresh_pending_summary();
        self.review_screen = ReviewScreen::Graph;
        self.comparison_commit_shas = None;
        self.workspace = crate::workspace::Workspace::new();
        self.pane_scrolls.borrow_mut().clear();
        self.diff_selections.clear();
        self.stop_caret_blink();
        self.diff_row_cache.borrow_mut().clear();
        self.read_only_cell_cache.borrow_mut().clear();
        self.graph_layout_cache.borrow_mut().take();
        self.file_tree_highlight_path = None;
        self.file_list_mode = FileListMode::Changed;
        self.collapsed_file_tree_paths.clear();
        self.record_recent_repository(recent_path);
        self.persist_settings();
        self.commit_history_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        self.file_tree_scroll.set_offset(point(px(0.), px(0.)));
        self.file_tree_hscroll.set_offset(point(px(0.), px(0.)));
        self.branch_sidebar_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        self.branch_sidebar_hovered = false;
        self.hidden_branches.clear();
        self.collapsed_branch_folders.clear();
        self.collapsed_branch_sections.clear();
        self.filter_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.branches_generation
            .set(self.branches_generation.get() + 1);
        self.sidebar_rows_cache.borrow_mut().take();
        self.file_tree_hovered = false;
        self.hovered_diff_pane = None;
        self.context_popover_open = false;
        self.repo_switcher_open = false;
        cx.notify();
    }

    /// Recompute the pending working-tree summary. Called on repository open,
    /// window activation, and changeset close — no filesystem watcher in v1.
    /// Read errors degrade to a clean summary rather than surfacing.
    fn refresh_pending_summary(&mut self) {
        self.pending_summary = match &self.mode {
            Mode::RepoOpen { repo } => repo::read_pending_summary(&repo.path).unwrap_or_default(),
            Mode::NoRepo => repo::PendingSummary::default(),
        };
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

    /// Capture the live window geometry and sidebar widths, then persist once.
    /// Invoked from the window close/quit hooks. Each piece updates settings
    /// independently, so a missing display or an unrendered split never blocks
    /// saving the rest.
    fn persist_session_state(&mut self, window: &Window, cx: &gpui::App) {
        if let Some(state) = crate::window_placement::capture_window_state(window, cx) {
            self.settings.window_state = Some(state);
        }
        self.capture_sidebar_widths(cx);
        self.persist_settings();
    }

    /// Read each resizable split's current left-panel width and store it. A
    /// split whose `sizes()` is empty (never rendered this session) is skipped,
    /// so it never overwrites a previously-saved width with nothing.
    fn capture_sidebar_widths(&mut self, cx: &gpui::App) {
        if let Some(width) = self
            .graph_resizable
            .read(cx)
            .sizes()
            .first()
            .copied()
            .map(f32::from)
        {
            self.settings.sidebar_widths.branch_sidebar = Some(width);
        }
        if let Some(width) = self
            .changeset_resizable
            .read(cx)
            .sizes()
            .first()
            .copied()
            .map(f32::from)
        {
            self.settings.sidebar_widths.changeset_files = Some(width);
        }
    }

    /// Make `sha` the single selected commit. Clicking the already-selected
    /// commit keeps it selected: the graph always carries a selection (see
    /// `default_selection`), so nothing ever toggles back to no selection.
    pub(crate) fn select_single_commit(&mut self, sha: String, cx: &mut Context<Self>) {
        self.selection = Selection::Single { sha };
        cx.notify();
    }

    /// Flip a branch's graph visibility. Hiding may make the selected
    /// commit(s) invisible, in which case the selection resets to the
    /// default. The HEAD branch never reaches this path: its sidebar row
    /// renders no toggle.
    pub(crate) fn toggle_branch_visibility(&mut self, name: String, cx: &mut Context<Self>) {
        if self.hidden_branches.remove(&name) {
            cx.notify();
            return;
        }
        self.hidden_branches.insert(name);
        self.reset_selection_if_hidden();
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

    /// Collapse or expand a whole sidebar section (Local or Remote, keyed by
    /// its ref namespace `heads` / `remotes`). Purely visual: removes the
    /// section's descendant rows without touching graph visibility.
    pub(crate) fn toggle_branch_section(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.collapsed_branch_sections.insert(key.clone()) {
            self.collapsed_branch_sections.remove(&key);
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
            self.reset_selection_if_hidden();
        } else {
            for name in &descendants {
                self.hidden_branches.remove(name);
            }
        }
        cx.notify();
    }

    /// Reset the selection to the default when hiding a branch removed any
    /// selected commit from the visible graph. The checked-out branch can
    /// never be hidden, so the default stays visible. No-ops outside
    /// `Mode::RepoOpen` or when the selection is already visible.
    fn reset_selection_if_hidden(&mut self) {
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
            // The pending selection is not tied to any branch or commit, so
            // hiding branches never affects it.
            Selection::Pending => false,
            Selection::Single { sha } => !visible.contains(sha),
            Selection::Range { shas, .. } => shas.iter().any(|sha| !visible.contains(sha)),
            Selection::Compare {
                base_sha,
                target_sha,
            } => !visible.contains(base_sha) || !visible.contains(target_sha),
        };
        if selection_hidden {
            self.selection = default_selection(repo, &self.hidden_branches);
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
        // Row 0 is always the pending row, so commit `commit_index` sits at
        // graph row `commit_index + 1`, and the total row count grows by one.
        self.scroll_commit_row_into_view(commit_index + 1, commit_count + 1);
        cx.notify();
    }

    /// Center the commit row at `index` in the history viewport, clamped to
    /// the scrollable range. Content height comes from the commit count
    /// rather than the scroll handle's max offset, which is stale when this
    /// runs in the same frame that paged in more commits.
    fn scroll_commit_row_into_view(&self, index: usize, commit_count: usize) {
        let viewport_height = self
            .commit_history_scroll
            .0
            .borrow()
            .base_handle
            .bounds()
            .size
            .height;
        let row_top = px(index as f32 * COMMIT_ROW_HEIGHT);
        if viewport_height <= px(0.) {
            // Not laid out yet; pin the row to the top rather than centering
            // against a zero-height viewport.
            self.commit_history_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), -row_top));
            return;
        }
        let centered_top = row_top - (viewport_height - px(COMMIT_ROW_HEIGHT)) / 2.;
        let content_height = px(commit_count as f32 * COMMIT_ROW_HEIGHT);
        let max_offset = (content_height - viewport_height).max(px(0.));
        let target = (-centered_top).clamp(-max_offset, px(0.));
        self.commit_history_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), target));
    }

    fn select_commit(
        &mut self,
        sha: String,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending_gesture = sha == repo::PENDING_SHA;
        let pending_selected = self.selection == Selection::Pending;
        if (pending_gesture || pending_selected) && (modifiers.secondary() || modifiers.shift) {
            // Exception: a shift/cmd click on the pending row while pending is
            // already selected is a no-op, mirroring same-commit gestures.
            if pending_gesture && pending_selected {
                return;
            }
            self.push_open_failed(
                "Pending changes can only be reviewed on their own.".to_string(),
                window,
                cx,
            );
            return;
        }
        if pending_gesture {
            if !pending_selected {
                self.selection = Selection::Pending;
                cx.notify();
            }
            return;
        }

        // The secondary (cmd/ctrl) modifier wins over shift, so a
        // cmd+shift-click reads as a comparison rather than a range.
        if modifiers.secondary() {
            self.compare_with_commit(sha, window, cx);
            return;
        }

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

    /// Turn the current selection into a merge-preview comparison whose merge
    /// destination is `sha`. The selection's anchor commit is the merge
    /// source — the side whose changes the preview shows — and each cmd-click
    /// picks where it would merge into: first select the branch under review,
    /// then cmd-click the branch it would land on. The anchor is a Single's
    /// commit, a Range's first-clicked endpoint, or an active comparison's
    /// source (so a third cmd-click re-aims the preview at a new destination
    /// while the source stays put). Cmd-clicking either endpoint of the
    /// pending comparison is a no-op, mirroring the shift-click same-commit
    /// no-op.
    fn compare_with_commit(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        let target_sha = match &self.selection {
            Selection::Single { sha } => sha.clone(),
            Selection::Range { start_sha, .. } => start_sha.clone(),
            Selection::Compare {
                base_sha,
                target_sha,
            } => {
                if *base_sha == sha {
                    return;
                }
                target_sha.clone()
            }
            Selection::None => {
                self.select_single_commit(sha, cx);
                return;
            }
            // Unreachable: select_commit intercepts every secondary-modifier
            // gesture to or from the pending selection before calling here.
            Selection::Pending => return,
        };

        if target_sha == sha {
            return;
        }

        match self.comparison_base_exists(&sha, &target_sha) {
            Ok(true) => {
                self.selection = Selection::Compare {
                    base_sha: sha,
                    target_sha,
                };
                cx.notify();
            }
            Ok(false) => self.push_open_failed(
                "Those commits share no common history to compare.".to_string(),
                window,
                cx,
            ),
            Err(err) => self.push_open_failed(err.to_string(), window, cx),
        }
    }

    /// Whether a comparison between two loaded commits has a merge base.
    /// Mirrors `range_shas_between`: both endpoints must be in the loaded
    /// history, and the repository must report a common ancestor.
    fn comparison_base_exists(
        &self,
        base_sha: &str,
        target_sha: &str,
    ) -> Result<bool, repo::ChangeSetError> {
        let open_repo = match &self.mode {
            Mode::RepoOpen { repo } => repo,
            Mode::NoRepo => return Ok(false),
        };

        if !open_repo
            .commits
            .iter()
            .any(|commit| commit.sha == base_sha)
        {
            return Ok(false);
        }
        if !open_repo
            .commits
            .iter()
            .any(|commit| commit.sha == target_sha)
        {
            return Ok(false);
        }

        Ok(repo::merge_base_sha(&open_repo.path, base_sha, target_sha)?.is_some())
    }

    /// Reverse the pending comparison's direction. The merge base is
    /// symmetric, so no revalidation is needed. No-ops unless a comparison
    /// is the current selection.
    fn swap_comparison(&mut self, cx: &mut Context<Self>) {
        if let Selection::Compare {
            base_sha,
            target_sha,
        } = &mut self.selection
        {
            std::mem::swap(base_sha, target_sha);
            cx.notify();
        }
    }

    /// True while the branch-filter input owns keyboard focus. The enter
    /// binding is app-global and would otherwise fire while the user is
    /// typing a filter query (the input's own bindings do not consume it),
    /// so the open-changeset action handler bails out in that state.
    fn branch_filter_has_focus(&self, window: &Window, cx: &Context<Self>) -> bool {
        gpui::Focusable::focus_handle(&self.filter_input, cx).is_focused(window)
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
        // Re-opening while a changeset is already open would rebuild the
        // workspace and throw away the user's tabs and splits. The spec makes
        // changing the open changeset an explicit close-adjust-reopen flow,
        // so ignore the action (reachable via the enter binding) until then.
        if matches!(self.review_screen, ReviewScreen::Changeset { .. }) {
            return;
        }
        let repo_path = match &self.mode {
            Mode::RepoOpen { repo } => repo.path.clone(),
            Mode::NoRepo => return,
        };

        let changeset = match &self.selection {
            Selection::Pending => repo::changeset_for_pending(&repo_path),
            Selection::Single { sha } => repo::changeset_for_single_commit(&repo_path, sha),
            Selection::Range { shas, .. } => {
                let (Some(newest_sha), Some(oldest_sha)) = (shas.first(), shas.last()) else {
                    return;
                };
                repo::changeset_for_commit_range(&repo_path, oldest_sha, newest_sha)
            }
            Selection::Compare {
                base_sha,
                target_sha,
            } => repo::changeset_for_comparison(&repo_path, base_sha, target_sha),
            Selection::None => return,
        };

        // The title-bar popover lists the commits a comparison would merge in;
        // walk them once here rather than on every popover render.
        let comparison_commit_shas = match &self.selection {
            Selection::Compare {
                base_sha,
                target_sha,
            } if changeset.is_ok() => repo::commit_shas_introduced_by(
                &repo_path,
                base_sha,
                target_sha,
                repo::INITIAL_COMMIT_LIMIT,
            )
            .ok(),
            _ => None,
        };

        match changeset {
            Ok(changeset) => {
                self.comparison_commit_shas = comparison_commit_shas;
                // Each changeset starts with the default single-pane layout;
                // splits last only while the changeset stays open.
                self.workspace = crate::workspace::Workspace::new();
                self.pane_scrolls.borrow_mut().clear();
                self.diff_selections.clear();
                self.stop_caret_blink();
                self.diff_row_cache.borrow_mut().clear();
                self.read_only_cell_cache.borrow_mut().clear();
                self.file_tree_highlight_path = None;
                self.file_tree_scroll.set_offset(point(px(0.), px(0.)));
                self.file_tree_hscroll.set_offset(point(px(0.), px(0.)));
                self.file_tree_hovered = false;
                self.hovered_diff_pane = None;
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
        self.comparison_commit_shas = None;
        self.context_popover_open = false;
        self.workspace.clear();
        self.file_tree_highlight_path = None;
        self.file_tree_hovered = false;
        self.hovered_diff_pane = None;
        self.reset_pane_scrolls();
        self.diff_selections.clear();
        // AI threads are ephemeral and scoped to the open changeset
        // (ADR-0005): closing it kills every running session.
        self.ai_sessions
            .update(cx, |sessions, cx| sessions.cancel_all(cx));
        self.stop_caret_blink();
        self.diff_row_cache.borrow_mut().clear();
        self.read_only_cell_cache.borrow_mut().clear();
        self.refresh_pending_summary();
        cx.notify();
    }

    fn quit_application(&mut self, cx: &mut Context<Self>) {
        self.ai_sessions
            .update(cx, |sessions, cx| sessions.cancel_all(cx));
        cx.emit(QuitRequested);
        cx.quit();
    }

    /// The AI session manager (ADR-0005). Feature surfaces reach AI threads
    /// exclusively through this entity.
    pub fn ai_sessions(&self) -> &Entity<crate::ai::AiSessions> {
        &self.ai_sessions
    }

    /// The scroll handles for `pane`, created on first use. The returned
    /// clone shares its underlying state with every other clone for the pane.
    pub(crate) fn pane_scroll(
        &self,
        pane: crate::workspace::PaneId,
        cx: &gpui::App,
    ) -> PaneScrollState {
        self.pane_scrolls
            .borrow_mut()
            .entry(pane)
            .or_insert_with(|| PaneScrollState::new(cx))
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

    /// The selection on `key`'s diff in `pane`, if one has been made.
    pub(crate) fn diff_selection(
        &self,
        pane: crate::workspace::PaneId,
        key: &str,
    ) -> Option<diff_selection::DiffSelection> {
        self.diff_selections.get(&(pane, key.to_string())).copied()
    }

    /// Bump and return the new blink epoch, orphaning whatever timer was
    /// previously in flight.
    fn next_blink_epoch(&mut self) -> usize {
        self.caret_blink_epoch += 1;
        self.caret_blink_epoch
    }

    /// Epoch-guarded blink loop, after gpui-component's `BlinkCursor`: each
    /// tick flips visibility and re-arms under a fresh epoch; a stale timer
    /// (its epoch no longer current) is a no-op instead of a spurious flip.
    fn blink_caret(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if epoch != self.caret_blink_epoch {
            return;
        }
        self.caret_blink_visible = !self.caret_blink_visible;
        cx.notify();
        let next = self.next_blink_epoch();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CARET_BLINK_INTERVAL).await;
            this.update(cx, |app, cx| app.blink_caret(next, cx)).ok();
        })
        .detach();
    }

    /// Any caret activity (placement, move, selection change) shows the
    /// caret solid immediately, then resumes blinking after one interval.
    /// Also the lazy start for the blink loop: the first call arms the
    /// recurring timer. The loop keeps running (re-arming itself) for as
    /// long as it's left unpaused; `stop_caret_blink` is what actually
    /// silences it, so callers that bulk-clear every selection must call
    /// that instead of relying on the absence of a caret to stop the timer.
    fn pause_caret_blink(&mut self, cx: &mut Context<Self>) {
        self.caret_blink_visible = true;
        cx.notify();
        let next = self.next_blink_epoch();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CARET_BLINK_INTERVAL).await;
            this.update(cx, |app, cx| app.blink_caret(next, cx)).ok();
        })
        .detach();
    }

    /// Silence the blink loop: orphan whatever timer is in flight (a stale
    /// epoch makes its next tick a no-op, see `blink_caret`) and leave the
    /// caret visible. Callers that bulk-clear `diff_selections` (opening a
    /// repository or changeset, closing a changeset) must call this beside
    /// the clear, since none of those paths otherwise stop a blink chain
    /// that a prior selection may have started — without it the 2Hz timer
    /// would run forever with nothing left to paint.
    fn stop_caret_blink(&mut self) {
        self.caret_blink_epoch += 1;
        self.caret_blink_visible = true;
    }

    /// Replace the selection on `key`'s diff in `pane`. A no-op (no insert,
    /// no notify, no blink pause) when `selection` is identical to what's
    /// already stored: an unchanged selection has nothing new to repaint,
    /// and leaving the blink chain alone means a stationary drag's per-tick
    /// mouse-move events don't each spawn a fresh pause/timer task. Callers
    /// that need a repaint independent of the selection (e.g. autoscroll)
    /// already issue their own `cx.notify()`.
    pub(crate) fn set_diff_selection(
        &mut self,
        pane: crate::workspace::PaneId,
        key: &str,
        selection: diff_selection::DiffSelection,
        cx: &mut Context<Self>,
    ) {
        let key_pair = (pane, key.to_string());
        if self.diff_selections.get(&key_pair) == Some(&selection) {
            return;
        }
        self.diff_selections.insert(key_pair, selection);
        self.pause_caret_blink(cx);
        cx.notify();
    }

    /// Drop selection entries whose `(pane, key)` no longer identifies an
    /// open tab. Called after every workspace mutation that can close or
    /// replace tabs, mirroring `prune_pane_scrolls`.
    fn prune_diff_selections(&mut self) {
        let live: HashSet<(crate::workspace::PaneId, String)> =
            self.workspace.open_keys().into_iter().collect();
        self.diff_selections.retain(|entry, _| live.contains(entry));
    }

    /// Entry point for every mouse-down inside a diff's code content: focuses
    /// the pane so keyboard selection routes there, then dispatches on click
    /// count and modifiers. Shift+click extends the existing selection on the
    /// same side (Task 8); a shift+click on the other side behaves like a
    /// plain click there, since a selection lives on exactly one side.
    /// A double-click selects the word under the pointer; a triple-click (or
    /// higher) selects the whole line. Any other click count falls through to
    /// placing a bare caret.
    pub(crate) fn begin_diff_mouse_selection(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        point: diff_selection::DiffPoint,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane_scroll(ctx.pane, cx).focus.focus(window);

        // Shift-extend takes priority over click-count dispatch and always
        // extends character-wise from the prior anchor, regardless of
        // whether the shift-click itself is a double- or triple-click.
        if event.modifiers.shift {
            if let Some(existing) = self.diff_selection(ctx.pane, &ctx.key) {
                if existing.side == ctx.side {
                    self.extend_diff_selection_from_anchor(ctx, existing.anchor, point, cx);
                    return;
                }
            }
        }

        match event.click_count {
            0 => {}
            2 => self.select_diff_word(ctx, point, cx),
            count if count >= 3 => self.select_diff_line(ctx, point, cx),
            _ => self.place_diff_caret(ctx, point, cx),
        }
    }

    /// Double-click's behavior: select the word under `point` (per
    /// `diff_selection::word_range_at`) and arm a `Word`-mode drag from it.
    fn select_diff_word(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        point: diff_selection::DiffPoint,
        cx: &mut Context<Self>,
    ) {
        let text = &ctx.content.cell(point.row).text;
        let range = diff_selection::word_range_at(text, point.column);
        let anchor = diff_selection::DiffPoint {
            row: point.row,
            column: range.start,
        };
        let head = diff_selection::DiffPoint {
            row: point.row,
            column: range.end,
        };
        self.set_diff_selection(
            ctx.pane,
            &ctx.key,
            diff_selection::DiffSelection {
                side: ctx.side,
                anchor,
                head,
                goal_x: None,
            },
            cx,
        );
        self.diff_drag = Some(DiffDrag {
            pane: ctx.pane,
            key: ctx.key.clone(),
            mode: DiffDragMode::Word {
                origin: (anchor, head),
            },
        });
    }

    /// Triple-click's (and gutter click's) behavior: select the whole line
    /// containing `point` (column 0 through the line's length) and arm a
    /// `Line`-mode drag from it.
    fn select_diff_line(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        point: diff_selection::DiffPoint,
        cx: &mut Context<Self>,
    ) {
        let anchor = diff_selection::line_start(point);
        let head = diff_selection::line_end(&ctx.content, point);
        self.set_diff_selection(
            ctx.pane,
            &ctx.key,
            diff_selection::DiffSelection {
                side: ctx.side,
                anchor,
                head,
                goal_x: None,
            },
            cx,
        );
        self.diff_drag = Some(DiffDrag {
            pane: ctx.pane,
            key: ctx.key.clone(),
            mode: DiffDragMode::Line {
                origin: (anchor, head),
            },
        });
    }

    /// Task 7's plain-click behavior: replace the selection with a bare
    /// caret at `point` and arm a character-mode drag from it.
    fn place_diff_caret(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        point: diff_selection::DiffPoint,
        cx: &mut Context<Self>,
    ) {
        self.set_diff_selection(
            ctx.pane,
            &ctx.key,
            diff_selection::DiffSelection::caret_at(point, ctx.side),
            cx,
        );
        self.diff_drag = Some(DiffDrag {
            pane: ctx.pane,
            key: ctx.key.clone(),
            mode: DiffDragMode::Character,
        });
    }

    /// Shift+click's behavior: keep `anchor`, move the head to `point`, and
    /// arm a character-mode drag so a shift+click-then-drag keeps extending.
    fn extend_diff_selection_from_anchor(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        anchor: diff_selection::DiffPoint,
        point: diff_selection::DiffPoint,
        cx: &mut Context<Self>,
    ) {
        self.set_diff_selection(
            ctx.pane,
            &ctx.key,
            diff_selection::DiffSelection {
                side: ctx.side,
                anchor,
                head: point,
                goal_x: None,
            },
            cx,
        );
        self.diff_drag = Some(DiffDrag {
            pane: ctx.pane,
            key: ctx.key.clone(),
            mode: DiffDragMode::Character,
        });
    }

    /// Extend the in-flight drag's selection to `point`. `Character` mode
    /// moves the head directly; `Word`/`Line` modes union the pointer's
    /// word/line range with the drag's origin range, anchoring at whichever
    /// end of the union is farther from the pointer so dragging back past the
    /// origin keeps growing the selection from the opposite end.
    pub(crate) fn extend_diff_mouse_selection(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        point: diff_selection::DiffPoint,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = &self.diff_drag else {
            return;
        };
        if drag.pane != ctx.pane || drag.key != ctx.key {
            return;
        }
        match drag.mode {
            DiffDragMode::Character => {
                let Some(mut selection) = self.diff_selection(ctx.pane, &ctx.key) else {
                    return;
                };
                // A selection lives on exactly one side; a drag that strayed
                // onto the other side's container (or a stale drag from a
                // since-replaced selection) does not move this side's head.
                if selection.side != ctx.side {
                    return;
                }
                selection.head = point;
                selection.goal_x = None;
                self.set_diff_selection(ctx.pane, &ctx.key, selection, cx);
            }
            DiffDragMode::Word { origin } => {
                let text = &ctx.content.cell(point.row).text;
                let range = diff_selection::word_range_at(text, point.column);
                let pointer = (
                    diff_selection::DiffPoint {
                        row: point.row,
                        column: range.start,
                    },
                    diff_selection::DiffPoint {
                        row: point.row,
                        column: range.end,
                    },
                );
                self.set_diff_selection(
                    ctx.pane,
                    &ctx.key,
                    union_range_selection(ctx.side, origin, pointer),
                    cx,
                );
            }
            DiffDragMode::Line { origin } => {
                let pointer = (
                    diff_selection::line_start(point),
                    diff_selection::line_end(&ctx.content, point),
                );
                self.set_diff_selection(
                    ctx.pane,
                    &ctx.key,
                    union_range_selection(ctx.side, origin, pointer),
                    cx,
                );
            }
        }
    }

    /// One mouse-move tick of an in-flight drag on a diff side container:
    /// autoscrolls the side (and shared horizontal pan) when the pointer sits
    /// in the edge margin, then maps the (possibly now-scrolled) pointer
    /// position to a `DiffPoint` and extends the selection. A no-op while no
    /// drag is armed for this pane+key, so a plain hover costs one field read.
    fn drag_diff_mouse_move(
        &mut self,
        ctx: &diff_view::DiffSelectionContext,
        side_scroll: &diff_view::DiffSideScroll,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = &self.diff_drag else {
            return;
        };
        if drag.pane != ctx.pane || drag.key != ctx.key {
            return;
        }
        let Some(side_bounds) = self
            .pane_scroll(ctx.pane, cx)
            .content_origins
            .borrow()
            .get(side_scroll.selector)
            .copied()
        else {
            return;
        };

        let scrolled = diff_view::autoscroll_diff_side(side_scroll, side_bounds, event.position);

        let point = diff_view::drag_point(window, ctx, side_scroll, side_bounds, event.position);
        self.extend_diff_mouse_selection(ctx, point, cx);
        // `extend_diff_mouse_selection` only notifies when the head actually
        // moves; an autoscroll tick that repeats the same point (pointer held
        // still in the margin) must still repaint to show the new scroll
        // position.
        if scrolled {
            cx.notify();
        }
    }

    /// End an in-flight diff mouse drag on mouse-up (inside or outside the
    /// side container). A no-op when the drag already belongs to a different
    /// pane+key, or none is armed.
    fn end_diff_mouse_drag(&mut self, ctx: &diff_view::DiffSelectionContext) {
        if self
            .diff_drag
            .as_ref()
            .is_some_and(|drag| drag.pane == ctx.pane && drag.key == ctx.key)
        {
            self.diff_drag = None;
        }
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
            self.pane_scroll(pane, cx).diff.reset();
            // A replaced preview tab keeps the same (pane, index) slot but a
            // new key, so the old key's selection would otherwise linger.
            self.prune_diff_selections();
        }
        if let Some(index) = self.workspace.active_index(pane) {
            self.pane_scroll(pane, cx).tab_bar.scroll_to_item(index);
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
            self.pane_scroll(pane, cx).diff.reset();
        }
        self.pane_scroll(pane, cx).tab_bar.scroll_to_item(index);
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
                self.pane_scroll(pane, cx).diff.reset();
                if let Some(index) = self.workspace.active_index(pane) {
                    self.pane_scroll(pane, cx).tab_bar.scroll_to_item(index);
                }
            } else {
                // Closing the pane's last tab collapsed the pane itself.
                self.prune_pane_scrolls();
            }
            self.prune_diff_selections();
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
            self.pane_scroll(pane, cx).diff.reset();
            if let Some(index) = self.workspace.active_index(pane) {
                self.pane_scroll(pane, cx).tab_bar.scroll_to_item(index);
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

    /// The prepared diff and scroll state for the file open in the active pane,
    /// or `None` when no changeset is open, no file is shown, or the file has
    /// no diff to prepare. Shared by change-block navigation and its footer.
    fn active_pane_prepared_diff(
        &self,
        cx: &gpui::App,
    ) -> Option<(Rc<PreparedFileDiff>, FileDiffScroll)> {
        let Mode::RepoOpen { repo } = &self.mode else {
            return None;
        };
        let ReviewScreen::Changeset { changeset, .. } = &self.review_screen else {
            return None;
        };
        let pane = self.workspace.active_pane();
        let path = self
            .workspace
            .active_item(pane)
            .map(|item| item.path().to_string())?;
        let file = changeset.files.iter().find(|file| file.path == path)?;
        let prepared = self.prepared_file_diff(repo, changeset, file).ok()?;
        Some((prepared, self.pane_scroll(pane, cx).diff))
    }

    /// Scroll the active pane's diff to the next or previous change block,
    /// wrapping around at the ends. A no-op outside a changeset or when the
    /// open file has no change blocks. Navigation is relative to the top of the
    /// viewport, so a block scrolled off screen is stepped to rather than
    /// skipped.
    fn navigate_change_block(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some((prepared, scroll)) = self.active_pane_prepared_diff(cx) else {
            return;
        };
        let blocks = prepared.blocks();
        if blocks.is_empty() {
            return;
        }
        let Some(anchor) = diff_view::change_block_anchor_row(&prepared, &scroll) else {
            return;
        };
        let Some((handle, _row_count)) = diff_view::change_block_scroll_target(&prepared, &scroll)
        else {
            return;
        };

        let target = if forward {
            if diff_view::change_block_scrolled_to_bottom(&prepared, &scroll) {
                // A late block clamped at the bottom counts as the last block,
                // so the next step wraps to the first rather than sticking.
                0
            } else {
                diff_view::next_block_index(blocks, anchor)
            }
        } else {
            diff_view::previous_block_index(blocks, anchor)
        };
        // Set the offset directly (as `FileDiffScroll::reset` does) rather than
        // via a deferred `scroll_to_item`, so the footer counter reads the new
        // position in the same frame instead of lagging one behind.
        diff_view::set_diff_scroll_top(
            &handle,
            diff_view::scroll_offset_for_block_top(
                blocks[target].start_row,
                diff_view::CHANGE_BLOCK_CONTEXT_ROWS,
            ),
        );
        cx.notify();
    }

    /// The active pane's change-block position as `(current_index, total)`,
    /// both derived from the live scroll offset. `None` when there is no
    /// navigable diff. Test-only observation hook.
    #[cfg(test)]
    fn active_diff_block_position(&self, cx: &gpui::App) -> Option<(usize, usize)> {
        let (prepared, scroll) = self.active_pane_prepared_diff(cx)?;
        let current = diff_view::current_change_block(&prepared, &scroll)?;
        Some((current, prepared.blocks().len()))
    }

    /// The active pane's active tab, its selectable content, and its stored
    /// selection — the shared resolution every keyboard selection action
    /// starts from. `None` when there is no changeset open, no active tab, or
    /// (the "no caret yet" rule) no selection has been made on that tab, since
    /// every keyboard selection action is a no-op before the first click.
    /// Mirrors `render_file_detail`'s changed-vs-read-only dispatch and, for a
    /// changed file rendered side-by-side, uses the stored selection's `side`
    /// to pick which side's content to resolve (a selection lives on exactly
    /// one side).
    fn active_diff_selection_context(
        &self,
    ) -> Option<(
        crate::workspace::PaneId,
        String,
        diff_selection::DiffSideContent,
        diff_selection::DiffSelection,
    )> {
        let Mode::RepoOpen { repo } = &self.mode else {
            return None;
        };
        let ReviewScreen::Changeset { changeset, .. } = &self.review_screen else {
            return None;
        };
        let pane = self.workspace.active_pane();
        let path = self.workspace.active_item(pane)?.path().to_string();
        let selection = self.diff_selection(pane, &path)?;

        let content = if let Some(file) = changeset.files.iter().find(|file| file.path == path) {
            let prepared = self.prepared_file_diff(repo, changeset, file).ok()?;
            diff_selection::DiffSideContent::Prepared {
                diff: prepared,
                side: selection.side,
            }
        } else {
            diff_selection::DiffSideContent::ReadOnly {
                cells: self.read_only_cells(repo, changeset, &path)?,
            }
        };

        Some((pane, path, content, selection))
    }

    /// The line cells for the read-only (unchanged) file at `path`, read once
    /// per changeset and cached — the read-only counterpart of
    /// `prepared_file_diff`, so consecutive keystrokes don't each re-read the
    /// blob from git. `None` for binary content (no selectable text) or a
    /// failed read; neither is cached, mirroring `prepared_file_diff`'s
    /// error handling.
    fn read_only_cells(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        path: &str,
    ) -> Option<Rc<Vec<diff_view::DiffLineCell>>> {
        let key = (path.to_string(), changeset.commit_sha.clone());
        if let Some(cached) = self.read_only_cell_cache.borrow().get(&key) {
            return Some(cached.clone());
        }
        let file_content = if changeset.commit_sha == repo::PENDING_SHA {
            repo::file_content_in_worktree(&repo.path, path)
        } else {
            repo::file_content_at_commit(&repo.path, &changeset.commit_sha, path)
        }
        .ok()?;
        let repo::FileContentBody::Text(text) = file_content.content else {
            return None;
        };
        let cells = Rc::new(diff_view::read_only_file_cells(&text));
        self.read_only_cell_cache
            .borrow_mut()
            .insert(key, cells.clone());
        Some(cells)
    }

    /// The shape every keyboard motion/extension action shares: resolve the
    /// active caret (no-op if there is none), run `motion` to get the new
    /// head, clamp it onto real content, move the head (and, unless
    /// `extend`, the anchor too), store the result, and scroll the caret into
    /// view. Horizontal motions call this directly; vertical motions wrap it
    /// (see `diff_vertical_motion`) since they must manage `goal_x`
    /// themselves rather than have it cleared here.
    pub(crate) fn diff_motion(
        &mut self,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        motion: impl Fn(
            &diff_selection::DiffSideContent,
            diff_selection::DiffPoint,
        ) -> diff_selection::DiffPoint,
    ) {
        let Some((pane, key, content, mut selection)) = self.active_diff_selection_context() else {
            return;
        };
        // A keypress always snaps the caret solid, even when the motion is a
        // boundary no-op (e.g. left at the document start) that
        // `set_diff_selection` skips as an identical write.
        self.pause_caret_blink(cx);
        let new_head = content.clamp(motion(&content, selection.head));
        selection.head = new_head;
        if !extend {
            selection.anchor = new_head;
        }
        selection.goal_x = None;
        self.set_diff_selection(pane, &key, selection, cx);
        self.scroll_diff_caret_into_view(pane, &key, window, cx);
    }

    /// Vertical motion: `Up`/`Down`, honoring and updating the goal-x
    /// position (the remembered pixel column vertical steps track across
    /// rows of different widths). Unlike `diff_motion`, this does not clear
    /// `goal_x` — it sets it on the first vertical step and preserves it
    /// across consecutive ones, only clearing when `active_diff_selection_context`
    /// changes it via a horizontal motion or a fresh click. A no-op at the
    /// document edge, matching `vertical_target_row`'s `None` there.
    pub(crate) fn diff_vertical_motion(
        &mut self,
        extend: bool,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((pane, key, content, mut selection)) = self.active_diff_selection_context() else {
            return;
        };
        // As in `diff_motion`: a keypress snaps the caret solid even when the
        // step is a no-op at the document edge.
        self.pause_caret_blink(cx);
        let Some(target_row) =
            diff_selection::vertical_target_row(&content, selection.head.row, forward)
        else {
            return;
        };
        let current_text = &content.cell(selection.head.row).text;
        let goal_x = selection.goal_x.unwrap_or_else(|| {
            diff_view::x_for_column(window, current_text, selection.head.column)
        });
        let target_text = content.cell(target_row).text.clone();
        let target_column = diff_view::column_for_x(window, &target_text, goal_x);
        let new_head = content.clamp(diff_selection::DiffPoint {
            row: target_row,
            column: target_column,
        });
        selection.head = new_head;
        if !extend {
            selection.anchor = new_head;
        }
        selection.goal_x = Some(goal_x);
        self.set_diff_selection(pane, &key, selection, cx);
        self.scroll_diff_caret_into_view(pane, &key, window, cx);
    }

    /// `Cmd+A`: select the caret's entire side. A no-op when the content has
    /// no selectable rows at all (`document_start`/`document_end` are `None`)
    /// — vacuously nothing to select.
    pub(crate) fn select_all_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((pane, key, content, mut selection)) = self.active_diff_selection_context() else {
            return;
        };
        let Some(start) = diff_selection::document_start(&content) else {
            return;
        };
        let Some(end) = diff_selection::document_end(&content) else {
            return;
        };
        selection.anchor = start;
        selection.head = end;
        selection.goal_x = None;
        self.set_diff_selection(pane, &key, selection, cx);
        self.scroll_diff_caret_into_view(pane, &key, window, cx);
    }

    /// `Escape`: collapse the selection to a bare caret at the head, per
    /// `docs/superpowers/specs/2026-07-02-diff-selection-design.md` ("Escape
    /// collapses the selection to the caret") — it does not clear the
    /// selection entirely.
    pub(crate) fn cancel_diff_selection(&mut self, cx: &mut Context<Self>) {
        let Some((pane, key, _content, mut selection)) = self.active_diff_selection_context()
        else {
            return;
        };
        if selection.is_caret() {
            return;
        }
        selection.anchor = selection.head;
        self.set_diff_selection(pane, &key, selection, cx);
    }

    /// `Cmd+C`: copy the selected text as plain characters, no line numbers or
    /// `+`/`-` markers. A bare caret copies nothing.
    pub(crate) fn copy_diff_selection(&mut self, cx: &mut Context<Self>) {
        let Some((_pane, _key, content, selection)) = self.active_diff_selection_context() else {
            return;
        };
        if selection.is_caret() {
            return;
        }
        let text = diff_selection::selection_text(&content, &selection);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    /// Scroll `pane`'s `key` diff so its caret is on screen on both axes,
    /// after any keyboard motion. Vertical: if the caret's row sits outside
    /// the visible row window (derived from the side's current scroll offset
    /// and its painted content height), jump so the caret rests one line
    /// inside the nearer edge. Horizontal: if the caret's shaped x position
    /// falls outside the panned viewport, adjust the shared horizontal pan
    /// the same way, keeping `DIFF_CARET_H_MARGIN` between the caret and the
    /// pane edge. A no-op when there is no active selection or the side's
    /// bounds have not been painted yet.
    fn scroll_diff_caret_into_view(
        &mut self,
        pane: crate::workspace::PaneId,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = self.diff_selection(pane, key) else {
            return;
        };
        let Some((_pane, _key, content, _selection)) = self.active_diff_selection_context() else {
            return;
        };
        let scroll = self.pane_scroll(pane, cx);
        let handle = scroll.diff.handle_for(selection.side).clone();
        let selector = match selection.side {
            repo::DiffSide::Old => "file-diff-side-old",
            repo::DiffSide::New => "file-diff-side-new",
        };
        let side_bounds = scroll.content_origins.borrow().get(selector).copied();

        let line_height = px(diff_view::DIFF_LINE_HEIGHT);
        let row = selection.head.row;
        let current_offset = diff_view::diff_scroll_top(&handle);
        if let Some(bounds) = side_bounds {
            let visible_rows = (bounds.size.height / line_height).floor().max(1.) as usize;
            let topmost = diff_view::topmost_row_for_offset(current_offset, content.len());
            // One line of margin inside each edge, per the brief.
            if row < topmost + 1 {
                let target_row = row.saturating_sub(1);
                diff_view::set_diff_scroll_top(&handle, -px(target_row as f32 * DIFF_LINE_HEIGHT));
            } else if row + 1 >= topmost + visible_rows {
                let target_row = (row + 2).saturating_sub(visible_rows);
                diff_view::set_diff_scroll_top(&handle, -px(target_row as f32 * DIFF_LINE_HEIGHT));
            }
        }

        if let Some(bounds) = side_bounds {
            let text = &content.cell(row).text;
            let caret_x = diff_view::x_for_column(window, text, selection.head.column);
            let pan = scroll.diff.hscroll.offset().x;
            let viewport_width =
                (bounds.size.width - px(diff_view::DIFF_GUTTER_WIDTH) - px(12.)).max(px(0.));
            // A small margin inside each edge, mirroring the vertical axis's
            // one-line margin, halved when the viewport is too narrow to
            // afford one on both sides.
            let margin = px(diff_view::DIFF_CARET_H_MARGIN).min(viewport_width / 2.);
            let visible_left = -pan;
            let visible_right = visible_left + viewport_width;
            if caret_x < visible_left + margin {
                scroll.diff.hscroll.set_offset(point(
                    (margin - caret_x).min(px(0.)),
                    scroll.diff.hscroll.offset().y,
                ));
            } else if caret_x > visible_right - margin {
                scroll.diff.hscroll.set_offset(point(
                    -(caret_x + margin - viewport_width),
                    scroll.diff.hscroll.offset().y,
                ));
            }
        }
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
            self.prune_diff_selections();
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
            self.pane_scroll(to_pane, cx).diff.reset();
            if let Some(index) = self.workspace.active_index(to_pane) {
                self.pane_scroll(to_pane, cx).tab_bar.scroll_to_item(index);
            }
            self.prune_pane_scrolls();
            self.prune_diff_selections();
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
            self.pane_scroll(new_pane, cx).diff.reset();
            self.prune_pane_scrolls();
            self.prune_diff_selections();
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
        let max_offset = self
            .commit_history_scroll
            .0
            .borrow()
            .base_handle
            .max_offset()
            .height;
        let current_offset = self.commit_history_scroll.0.borrow().base_handle.offset().y;
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
            // The pending row is not a commit; no commit row reads as
            // selected while it is active.
            Selection::Pending => false,
            Selection::Single { sha: selected_sha } => selected_sha == sha,
            Selection::Range { shas, .. } => shas.iter().any(|selected_sha| selected_sha == sha),
            Selection::Compare {
                base_sha,
                target_sha,
            } => base_sha == sha || target_sha == sha,
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
    fn file_diff_old_scroll_offset(&self, cx: &gpui::App) -> gpui::Point<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane(), cx)
            .diff
            .side_by_side_offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_offset(&self, cx: &gpui::App) -> gpui::Point<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane(), cx)
            .diff
            .side_by_side_offset()
    }

    #[cfg(test)]
    fn file_diff_new_scroll_max_offset(&self, cx: &gpui::App) -> gpui::Size<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane(), cx)
            .diff
            .side_by_side_max_offset()
    }

    #[cfg(test)]
    fn file_diff_hscroll_offset(&self, cx: &gpui::App) -> gpui::Point<gpui::Pixels> {
        self.pane_scroll(self.workspace.active_pane(), cx)
            .diff
            .hscroll_offset()
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
                    .border_color(palette().border)
                    .bg(palette().surface)
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(palette().border)
                            .text_color(palette().text_muted)
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
                    .text_color(palette().text)
                    .text_size(px(20.))
                    .child("No repository open"),
            )
            .child(
                div()
                    .text_color(palette().text_muted)
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
            palette().text
        } else {
            palette().text_disabled
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
            .border_color(palette().border)
            .cursor_pointer()
            .id(("recent-repository-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, _event, window, cx| {
                app.open_recent_repository(open_path.clone(), window, cx);
            }))
            .child(
                div()
                    .flex_1()
                    .font_family(MONO_FONT_FAMILY)
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
                                .border_color(palette().danger_border)
                                .bg(palette().danger_bg)
                                .text_color(palette().danger_fg)
                                .text_size(px(11.))
                                .child("Unavailable"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .border_1()
                                .border_color(palette().border)
                                .bg(palette().element_bg)
                                .text_color(palette().text)
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

    /// Returns the memoized graph layout for `repo`, recomputing only when the
    /// loaded commits, HEAD, or hidden branches change.
    fn graph_layout(&self, repo: &repo::OpenRepository) -> Rc<GraphLayout> {
        let visible_commits = visible_commits(repo, &self.hidden_branches);
        let head_sha = visible_commits
            .iter()
            .find(|commit| commit.is_head)
            .map(|commit| commit.sha.clone());
        let signature = GraphLayoutSignature {
            commit_count: visible_commits.len(),
            // Defensive: HEAD movement in practice only happens on repo reopen,
            // which already clears this cache. Keying on it too costs nothing
            // and guards against any future in-session HEAD change.
            head_sha: head_sha.clone(),
            hidden_branches: self.hidden_branches.clone(),
        };

        if let Some((cached_signature, layout)) = self.graph_layout_cache.borrow().as_ref() {
            if *cached_signature == signature {
                return Rc::clone(layout);
            }
        }

        let mut graph_commits = Vec::with_capacity(visible_commits.len() + 1);
        // The synthetic pending-changes node: newest possible timestamp so the
        // trunk-extension rule keeps it in lane 0 as HEAD's continuation, with
        // HEAD as its only parent (parentless on an unborn branch).
        graph_commits.push(graph::GraphCommit {
            sha: repo::PENDING_SHA.to_string(),
            authored_timestamp: i64::MAX,
            parent_shas: head_sha.iter().cloned().collect(),
        });
        graph_commits.extend(visible_commits.iter().map(|commit| graph::GraphCommit {
            sha: commit.sha.clone(),
            authored_timestamp: commit.authored_timestamp,
            parent_shas: commit.parent_shas.clone(),
        }));
        let rows = graph::layout_graph_anchored(&graph_commits, head_sha.as_deref());
        let max_lanes = rows.iter().map(|row| row.lane_count).max().unwrap_or(1);
        let layout = Rc::new(GraphLayout { rows, max_lanes });

        #[cfg(test)]
        self.graph_layout_recompute_count
            .set(self.graph_layout_recompute_count.get() + 1);

        *self.graph_layout_cache.borrow_mut() = Some((signature, Rc::clone(&layout)));
        layout
    }

    #[cfg(test)]
    pub(crate) fn graph_layout_recompute_count(&self) -> u64 {
        self.graph_layout_recompute_count.get()
    }

    fn render_commit_history_list(
        &self,
        item_count: usize,
        scroll_handle: UniformListScrollHandle,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        uniform_list(
            "commit-history",
            item_count,
            cx.processor(move |app, range: std::ops::Range<usize>, _window, cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    return Vec::new();
                };
                // Cheap O(loaded-commits) filter of references (no heavy
                // clones); recomputed per frame intentionally. The
                // expensive O(n^2) DAG layout it feeds is memoized by
                // `graph_layout`.
                let visible_commits = visible_commits(repo, &app.hidden_branches);
                let layout = app.graph_layout(repo);
                range
                    .map(|index| {
                        if index == 0 {
                            app.render_pending_row(
                                &layout.rows,
                                layout.max_lanes,
                                app.selection == Selection::Pending,
                                cx,
                            )
                            .into_any_element()
                        } else {
                            app.render_commit_row(
                                index,
                                visible_commits[index - 1],
                                &layout.rows,
                                layout.max_lanes,
                                app.is_commit_selected(&visible_commits[index - 1].sha),
                                cx,
                            )
                            .into_any_element()
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .min_h_0()
        .min_w_0()
        .pr(px(COMMIT_HISTORY_SCROLLBAR_GUTTER))
        .track_scroll(scroll_handle)
        .on_scroll_wheel(cx.listener(|app, event, window, cx| {
            app.load_older_commits_after_scroll(event, window, cx);
        }))
        .debug_selector(|| "commit-history".to_string())
        .into_any_element()
    }

    fn render_graph_screen(
        &self,
        repo: &repo::OpenRepository,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        // The pending row always occupies row 0, so the list renders even when
        // the repository has no commits; the "no commits" message becomes a
        // sibling below the pending row rather than replacing the list.
        let layout = self.graph_layout(repo);
        let item_count = layout.rows.len();
        let scroll_handle = self.commit_history_scroll.clone();
        // Selection highlight, painted behind the rows. Commit rows carry
        // no background of their own: the graph gutter's bend overlays
        // deliberately spill across row borders (curves are centered on
        // the boundary between rows), and rows paint top to bottom, so an
        // opaque background on a row would erase whatever the row above
        // drew below the border. The highlight therefore lives in this
        // canvas underlay, which reads the scroll offset at paint time so
        // it can never lag the rows during a scroll.
        let mut selected_indices: Vec<usize> = visible_commits(repo, &self.hidden_branches)
            .iter()
            .enumerate()
            .filter(|(_, commit)| self.is_commit_selected(&commit.sha))
            .map(|(index, _)| index + 1)
            .collect();
        if self.selection == Selection::Pending {
            selected_indices.insert(0, 0);
        }
        let underlay_scroll = self.commit_history_scroll.clone();
        let selection_underlay = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let scroll_offset = underlay_scroll.0.borrow().base_handle.offset();
                for rect in commit_history_selection_underlay_rects(
                    bounds,
                    scroll_offset.y,
                    &selected_indices,
                ) {
                    window.paint_quad(gpui::fill(rect, palette().row_selected));
                }
            },
        )
        .absolute()
        .left_0()
        .top_0()
        .size_full();

        let history = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_h_0()
            .min_w_0()
            .debug_selector(|| "commit-history-container".to_string())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(selection_underlay)
                    .child(self.render_commit_history_list(item_count, scroll_handle, cx)),
            )
            .when(
                repo.commits.is_empty() && !self.pending_summary.is_dirty(),
                |history| {
                    history.child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .py_4()
                            .id("commit-history-empty")
                            .debug_selector(|| "commit-history-empty".to_string())
                            .text_color(palette().text_muted)
                            .text_size(px(14.))
                            .child("This repository has no commits to review."),
                    )
                },
            );

        let history_panel = div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(palette().background)
            // Contextual selection bar, docked to the top edge of the
            // history panel while a selection is active (near the cursor,
            // pushing the graph down). It owns the open-changeset
            // affordance; enter drives the same transition through the
            // OpenChangeset action. A selection always exists in a
            // non-empty graph, so the bar is effectively a permanent
            // fixture there.
            .when_some(selection_summary(&self.selection), |screen, summary| {
                screen.child(self.render_selection_bar(summary, cx))
            })
            .child(history);

        div()
            .flex()
            .w_full()
            .h_full()
            .min_h_0()
            .bg(palette().background)
            .child(
                h_resizable("graph-split")
                    .with_state(&self.graph_resizable)
                    .child(
                        resizable_panel()
                            .size(px(self.branch_sidebar_width()))
                            .child(self.render_branch_sidebar(repo, cx)),
                    )
                    .child(resizable_panel().child(history_panel)),
            )
    }

    /// The graph's contextual selection bar: selection count on the left,
    /// the open-changeset affordance (with its keyboard hint) on the right.
    /// Rendered only while a selection is active.
    fn render_selection_bar(
        &self,
        summary: String,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let keycap = |label: &'static str, border: Hsla, text: Hsla| {
            div()
                .flex_none()
                .px(px(4.))
                .border_1()
                .border_color(border)
                .rounded(px(3.))
                .text_size(px(9.))
                .text_color(text)
                .child(label)
        };

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(SELECTION_BAR_HEIGHT))
            .px_3()
            .bg(palette().surface)
            .border_b_1()
            .border_color(palette().border)
            .text_size(px(12.))
            .id("selection-bar")
            .debug_selector(|| "selection-bar".to_string())
            // A double-click on the top commit row makes the bar appear
            // under the cursor after the first click, so the second click
            // lands here. Complete that gesture from the anchor; a plain
            // click on the bar instead invalidates the anchor so a later
            // double-click on the bar cannot resurrect a stale one.
            .on_click(cx.listener(|app, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    if let Some(sha) = app.double_click_anchor.take() {
                        app.selection = Selection::Single { sha };
                        app.open_changeset(window, cx);
                    }
                } else {
                    app.double_click_anchor = None;
                }
            }))
            .child(div().text_color(palette().accent).child(summary))
            // A pending comparison is directional; the swap control reverses
            // which endpoint the merge preview targets.
            .when(matches!(self.selection, Selection::Compare { .. }), |bar| {
                bar.child(
                    div()
                        .flex()
                        .items_center()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(4.))
                        .bg(palette().accent_bg)
                        .text_color(palette().accent)
                        .cursor_pointer()
                        .hover(|style| style.bg(palette().accent_bg_hover))
                        .id("swap-comparison")
                        .debug_selector(|| "swap-comparison".to_string())
                        .on_click(cx.listener(|app, _event, _window, cx| {
                            app.swap_comparison(cx);
                        }))
                        .child("\u{21c4}"),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py(px(3.))
                    .rounded(px(4.))
                    .bg(palette().accent_bg)
                    .text_color(palette().accent)
                    .cursor_pointer()
                    .hover(|style| style.bg(palette().accent_bg_hover))
                    .id("open-changeset")
                    .debug_selector(|| "open-changeset".to_string())
                    .on_click(cx.listener(|app, _event, window, cx| {
                        app.open_changeset(window, cx);
                    }))
                    .child("Open changeset")
                    .child(keycap("\u{23ce}", palette().accent, palette().accent)),
            )
    }

    /// Width the branch sidebar opens at: the saved width when present and
    /// sane, otherwise the default. See [`restored_width`].
    fn branch_sidebar_width(&self) -> f32 {
        restored_width(
            self.settings.sidebar_widths.branch_sidebar,
            BRANCH_SIDEBAR_DEFAULT_WIDTH,
        )
    }

    /// Width the changed-files list opens at: the saved width when present and
    /// sane, otherwise the default. See [`restored_width`].
    fn changeset_files_width(&self) -> f32 {
        restored_width(
            self.settings.sidebar_widths.changeset_files,
            CHANGESET_FILES_DEFAULT_WIDTH,
        )
    }

    /// Returns the memoized flat sidebar row model, recomputing only when the
    /// branch set, collapse state, or hidden-branch set changes.
    fn sidebar_rows(&self, repo: &repo::OpenRepository, query: &str) -> Rc<Vec<BranchTreeRow>> {
        let signature = SidebarRowsSignature {
            branches_generation: self.branches_generation.get(),
            collapsed_folders: self.collapsed_branch_folders.clone(),
            collapsed_sections: self.collapsed_branch_sections.clone(),
            hidden_branches: self.hidden_branches.clone(),
            query: query.to_string(),
        };

        if let Some((cached_signature, rows)) = self.sidebar_rows_cache.borrow().as_ref() {
            if *cached_signature == signature {
                return Rc::clone(rows);
            }
        }

        let rows = Rc::new(build_branch_sidebar_rows(
            &repo.branches,
            &self.collapsed_branch_folders,
            &self.collapsed_branch_sections,
            &self.hidden_branches,
            query,
        ));

        #[cfg(test)]
        self.sidebar_rows_recompute_count
            .set(self.sidebar_rows_recompute_count.get() + 1);

        *self.sidebar_rows_cache.borrow_mut() = Some((signature, Rc::clone(&rows)));
        rows
    }

    #[cfg(test)]
    pub(crate) fn sidebar_rows_recompute_count(&self) -> u64 {
        self.sidebar_rows_recompute_count.get()
    }

    fn render_branch_sidebar(
        &self,
        repo: &repo::OpenRepository,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self.filter_input.read(cx).value().to_string();
        let has_query = !query.is_empty();

        // Lay the filter row out explicitly rather than via the Input's own
        // prefix/cleanable slots: that keeps the search glyph aligned with the
        // section/folder chevrons below (both at `px_3`) and pins the clear
        // button to the right edge. The Input sits in a `flex_1` middle with its
        // internal horizontal padding zeroed so the text starts right after the
        // glyph and the row's own padding governs alignment.
        let search_field = div()
            .flex()
            .items_center()
            .flex_none()
            .w_full()
            .gap_2()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(palette().border)
            .id("branch-filter-input")
            .debug_selector(|| "branch-filter-input".to_string())
            .child(
                Icon::new(LucideIcon::Search)
                    .text_color(palette().text_muted)
                    .size(px(FILE_TREE_STATUS_ICON_SIZE)),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.filter_input)
                        .appearance(false)
                        .pl_0()
                        .pr_0(),
                ),
            )
            .when(has_query, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .id("branch-filter-clear")
                        .debug_selector(|| "branch-filter-clear".to_string())
                        .on_click(cx.listener(|app, _event: &ClickEvent, window, cx| {
                            app.filter_input
                                .update(cx, |state, cx| state.set_value("", window, cx));
                        }))
                        .child(
                            Icon::new(LucideIcon::X)
                                .text_color(palette().text_muted)
                                .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            });

        let list_content: AnyElement = if repo.branches.is_empty() {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .id("branch-sidebar-empty")
                .debug_selector(|| "branch-sidebar-empty".to_string())
                .text_color(palette().text_muted)
                .text_size(px(14.))
                .child("No branches")
                .into_any_element()
        } else {
            let rows = self.sidebar_rows(repo, &query);
            if rows.is_empty() {
                // Invariant: `build_branch_sidebar_rows` with a non-empty
                // branch list and an empty query always emits at least a
                // section header, so an empty `rows` here can only mean a
                // non-empty query matched zero branches (an empty repo took
                // the branch above). Show the no-match message.
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .id("branch-filter-empty")
                    .debug_selector(|| "branch-filter-empty".to_string())
                    .text_color(palette().text_muted)
                    .text_size(px(14.))
                    .child("No matching branches")
                    .into_any_element()
            } else {
                let item_count = rows.len();
                let scroll_handle = self.branch_sidebar_scroll.clone();
                let processor_query = query.clone();
                uniform_list(
                    "branch-sidebar-scroll",
                    item_count,
                    cx.processor(move |app, range: std::ops::Range<usize>, _window, cx| {
                        let Mode::RepoOpen { repo } = &app.mode else {
                            return Vec::new();
                        };
                        let rows = app.sidebar_rows(repo, &processor_query);
                        range
                            .map(|index| match &rows[index] {
                                BranchTreeRow::Section(section) => app
                                    .render_branch_section_row(index, section, cx)
                                    .into_any_element(),
                                BranchTreeRow::Folder(folder) => app
                                    .render_branch_folder_row(index, folder, &processor_query, cx)
                                    .into_any_element(),
                                BranchTreeRow::Branch(branch_row) => app
                                    .render_branch_row(index, branch_row, &processor_query, cx)
                                    .into_any_element(),
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h_0()
                .track_scroll(scroll_handle)
                .debug_selector(|| "branch-sidebar-scroll".to_string())
                .into_any_element()
            }
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .min_h_0()
            .id("branch-sidebar")
            .debug_selector(|| "branch-sidebar".to_string())
            .border_1()
            .border_color(palette().border)
            .bg(palette().surface)
            .font_family(MONO_FONT_FAMILY)
            .on_hover(cx.listener(|app, hovered: &bool, _window, cx| {
                if app.branch_sidebar_hovered != *hovered {
                    app.branch_sidebar_hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(search_field)
            .child(
                // The scrollbar overlay lives inside this list region, not the
                // whole sidebar, so it starts at the top of the scrollable area
                // rather than over the fixed search field above it.
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
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
                                    Scrollbar::vertical(
                                        &self.branch_sidebar_scroll.0.borrow().base_handle,
                                    )
                                    .scrollbar_show(ScrollbarShow::Always),
                                ),
                        )
                    }),
            )
    }

    /// Render a sidebar row label, painting a highlight background behind the
    /// `highlight` char indices (positions within `text`). With no highlights
    /// this is an ordinary colored text node, identical to the pre-filter
    /// rendering.
    fn branch_label(&self, text: &str, highlight: &[usize], color: Hsla) -> AnyElement {
        if highlight.is_empty() {
            return div()
                .text_color(color)
                .child(text.to_string())
                .into_any_element();
        }

        let ranges = highlight_byte_ranges(text, highlight);
        // `StyledText::with_default_highlights` takes the font family from this
        // base style (it only inherits font_size/line_height from the ambient
        // window text style), so set the sidebar's monospace family explicitly
        // to keep matched labels visually consistent with unhighlighted rows.
        let base = TextStyle {
            color,
            font_family: MONO_FONT_FAMILY.into(),
            ..Default::default()
        };
        let highlights = ranges.into_iter().map(|range| {
            (
                range,
                HighlightStyle {
                    background_color: Some(palette().match_highlight_bg),
                    ..Default::default()
                },
            )
        });
        StyledText::new(text.to_string())
            .with_default_highlights(&base, highlights)
            .into_any_element()
    }

    fn render_branch_row(
        &self,
        index: usize,
        row: &BranchRow,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let branch = &row.branch;
        let selected = matches!(
            &self.selection,
            Selection::Single { sha } if sha == &branch.tip_sha
        );
        let key = branch_key(&branch.name, &branch.kind);
        let hidden = self.hidden_branches.contains(&key);
        // The icon is always laid out; hover/opacity controls visibility.
        let show_toggle = !branch.is_head;
        // A hidden branch keeps the icon opaque without hover.
        let always_show = hidden;
        // The checked-out branch is marked by a subtle background tint instead
        // of a check icon; an active commit selection still takes precedence.
        let row_bg = if selected {
            palette().row_selected
        } else if branch.is_head {
            palette().current_branch_bg
        } else {
            palette().surface
        };
        let name_color = if hidden {
            palette().text_muted
        } else {
            palette().text
        };
        let name_fragment = debug_ref_label_fragment(&key);
        let row_selector = if selected {
            format!("selected-branch-row-{name_fragment}")
        } else {
            format!("branch-row-{name_fragment}")
        };
        let toggle_selector = format!("branch-visibility-{name_fragment}");
        // Tag rows carry the tag icon; the selector prefix mirrors the choice
        // so the two stay distinguishable.
        let (row_icon, icon_prefix) = match branch.kind {
            repo::BranchKind::Tag => (LucideIcon::Tag, "tag"),
            _ => (LucideIcon::GitBranch, "branch"),
        };
        let icon_selector = format!("{icon_prefix}-icon-{name_fragment}");
        let group_name = format!("branch-row-group-{name_fragment}");
        let tip_sha = branch.tip_sha.clone();
        let toggle_branch_key = key;
        let display_name = row.display_name.clone();

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT + BRANCH_ROW_VERTICAL_PADDING * 2.))
            .py(px(BRANCH_ROW_VERTICAL_PADDING))
            .gap_2()
            .px_3()
            .bg(row_bg)
            .id(("branch-row", index))
            .group(group_name.clone())
            .debug_selector(move || row_selector.clone())
            .when(!selected && !hidden, |row| {
                row.hover(|style| style.bg(palette().element_hover))
            })
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
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .debug_selector(move || icon_selector.clone())
                    .child(
                        Icon::new(row_icon)
                            .text_color(name_color)
                            .size(px(BRANCH_ROW_ICON_SIZE)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child({
                        let highlight = if query.is_empty() {
                            Vec::new()
                        } else {
                            fuzzy_match(&branch.name, query)
                                .map(|idx| final_segment_highlights(&branch.name, &idx))
                                .unwrap_or_default()
                        };
                        self.branch_label(&display_name, &highlight, name_color)
                    }),
            )
            .when(show_toggle, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .id(("branch-visibility", index))
                        .debug_selector(move || toggle_selector.clone())
                        .when(!always_show, |toggle| {
                            toggle
                                .opacity(0.)
                                .group_hover(group_name.clone(), |toggle| toggle.opacity(1.))
                        })
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
                            .text_color(palette().text_muted)
                            .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            })
    }

    fn render_branch_section_row(
        &self,
        index: usize,
        section: &BranchSectionRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fragment = debug_ref_label_fragment(&section.title);
        let row_selector = format!("branch-section-{fragment}");
        let icon_selector = format!("branch-section-icon-{fragment}");
        let count_selector = format!("branch-section-count-{fragment}");
        let section_icon = if section.key == "remotes" {
            LucideIcon::Cloud
        } else {
            LucideIcon::Monitor
        };
        let toggle_key = section.key.clone();

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT + BRANCH_ROW_VERTICAL_PADDING * 2.))
            .py(px(BRANCH_ROW_VERTICAL_PADDING))
            .gap_2()
            .px_3()
            .bg(palette().surface)
            .border_b_1()
            .when(section.top_border, |header| header.border_t_1())
            .border_color(palette().border)
            .cursor_pointer()
            .id(("branch-section", index))
            .debug_selector(move || row_selector.clone())
            .hover(|style| style.bg(palette().element_hover))
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                app.toggle_branch_section(toggle_key.clone(), cx);
            }))
            .child(
                Icon::new(if section.collapsed {
                    LucideIcon::ChevronRight
                } else {
                    LucideIcon::ChevronDown
                })
                .text_color(palette().text_muted)
                .size(px(FILE_TREE_STATUS_ICON_SIZE)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .debug_selector(move || icon_selector.clone())
                    .child(
                        Icon::new(section_icon)
                            .text_color(palette().text_muted)
                            .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette().text_muted)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child(section.title.to_uppercase()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(palette().text_muted)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .debug_selector(move || count_selector.clone())
                    .child(section.count.to_string()),
            )
    }

    fn render_branch_folder_row(
        &self,
        index: usize,
        folder: &BranchFolderRow,
        query: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path_fragment = debug_ref_label_fragment(&folder.path);
        let row_selector = format!("branch-folder-{path_fragment}");
        let toggle_selector = format!("branch-folder-visibility-{path_fragment}");
        let collapse_path = folder.path.clone();
        let toggle_path = folder.path.clone();
        let always_show = folder.visibility != FolderVisibility::Visible;
        let name_color = palette().text_muted;
        let depth = folder.depth;
        let group_name = format!("branch-folder-group-{path_fragment}");

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT + BRANCH_ROW_VERTICAL_PADDING * 2.))
            .py(px(BRANCH_ROW_VERTICAL_PADDING))
            .gap_2()
            .px_3()
            .bg(palette().surface)
            .cursor_pointer()
            .id(("branch-folder", index))
            .group(group_name.clone())
            .debug_selector(move || row_selector.clone())
            .hover(|style| style.bg(palette().element_hover))
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
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child({
                        let highlight = if query.is_empty() {
                            Vec::new()
                        } else {
                            // Strip the `heads`/`remotes` key prefix to get the
                            // display path this folder's segment lives in.
                            let display_path = folder
                                .path
                                .split_once('/')
                                .map(|(_, rest)| rest)
                                .unwrap_or(folder.path.as_str());
                            let idx = prefix_match_indices(display_path, query);
                            final_segment_highlights(display_path, &idx)
                        };
                        self.branch_label(&folder.name, &highlight, name_color)
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .cursor_pointer()
                    .id(("branch-folder-visibility", index))
                    .debug_selector(move || toggle_selector.clone())
                    .when(!always_show, |toggle| {
                        toggle
                            .opacity(0.)
                            .group_hover(group_name.clone(), |toggle| toggle.opacity(1.))
                    })
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
                        .text_color(palette().text_muted)
                        .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                    ),
            )
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
                                .size(px(self.changeset_files_width()))
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
            .bg(palette().background)
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
                let files = if changeset.commit_sha == repo::PENDING_SHA {
                    repo::files_for_pending(&repo.path)
                } else {
                    repo::files_at_commit(&repo.path, &changeset.commit_sha)
                };
                files.map(|files| {
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
                .text_color(palette().text_muted)
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

            let mut scroll_container = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .id("changed-files-scroll")
                .debug_selector(|| "changed-files-scroll".to_string())
                .overflow_y_scroll()
                .track_scroll(&self.file_tree_scroll);
            // Without this, gpui redirects a *horizontal* wheel gesture onto
            // this vertical-only container (an unused axis delta falls through
            // to the scrollable axis), fighting the path pane's own pan.
            scroll_container
                .interactivity()
                .base_style
                .restrict_scroll_to_axis = Some(true);

            // Path pane: only this column scrolls horizontally.
            // items_start() prevents cross-axis stretch, allowing
            // the flex_none inner wrapper to exceed the viewport width.
            let mut path_pane = div()
                .id("changed-files-path-pane")
                .debug_selector(|| "changed-files-path-pane".to_string())
                .flex()
                .flex_col()
                .items_start()
                .flex_1()
                .min_w_0()
                .overflow_x_scroll()
                .track_scroll(&self.file_tree_hscroll);
            // Without this, gpui redirects a *vertical* wheel gesture onto this
            // horizontal-only pane, so a plain mouse wheel panned the paths
            // sideways while the outer container scrolled the rows.
            path_pane.interactivity().base_style.restrict_scroll_to_axis = Some(true);

            scroll_container
                .child(
                    // Two columns share this one vertical scroll, so they scroll
                    // vertically together. min_w_full pins the gutter to the
                    // viewport's right edge.
                    div()
                        .flex()
                        .flex_row()
                        .min_w_full()
                        .child(
                            path_pane.child(
                                // flex_none inner column sizes to the widest
                                // path so long paths scroll; min_w_full keeps
                                // it at least the pane's width when the pane
                                // is wider than the content, so rows (w_full)
                                // fill the pane and the selection background
                                // has no trailing gap.
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_none()
                                    .min_w_full()
                                    .children(path_cells),
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
            .border_color(palette().border)
            .bg(palette().surface)
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
                        .top(px(FILE_TREE_HEADER_HEIGHT))
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
            .h(px(FILE_TREE_HEADER_HEIGHT))
            .gap_2()
            .px(px(FILE_TREE_HEADER_INSET))
            .bg(palette().surface)
            .debug_selector(|| "file-tree-repo-root".to_string())
            .child(render_file_tree_indent_guides(0, "repo-root"))
            .child(render_file_tree_folder_icon(
                "repo-root",
                false,
                palette().text_muted.into(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(palette().text_muted)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
                    .font_family(MONO_FONT_FAMILY)
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
        // Ghost buttons, matching Zed's panel toolbars: no background at
        // rest, a subtle fill on hover. The active toggle keeps an accent
        // tint so its on/off state stays visible.
        let text_color = if active {
            palette().accent
        } else {
            palette().icon_muted
        };
        let hover_bg = if active {
            palette().accent_bg_hover
        } else {
            palette().ghost_element_hover
        };

        div()
            .id(selector)
            .debug_selector(move || selector.to_string())
            .flex()
            .items_center()
            .justify_center()
            .size(px(FILE_TREE_CONTROL_BUTTON_SIZE))
            .rounded(px(4.))
            .when(active, |button| button.bg(palette().accent_bg))
            .text_color(text_color)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
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
                    palette().row_selected
                } else {
                    palette().surface
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
            FileTreeRow::File {
                entry: FileListEntry::Unchanged(file),
                ..
            } => {
                let selected = self.is_file_path_highlighted(row.path());
                let path = file.path.clone();
                let gutter_selector = if selected {
                    format!("selected-unchanged-file-gutter-{index}")
                } else {
                    format!("unchanged-file-gutter-{index}")
                };
                base(selected)
                    .cursor_pointer()
                    .id(("unchanged-file-gutter", index))
                    .debug_selector(move || gutter_selector.clone())
                    .on_click(cx.listener(move |app, event: &ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            app.open_file_pinned(path.clone(), cx);
                        } else {
                            app.open_file_preview(path.clone(), cx);
                        }
                    }))
                    .into_any_element()
            }
            FileTreeRow::Folder { .. } => base(false).into_any_element(),
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
            .bg(palette().surface)
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
                palette().text_muted.into(),
            ))
            .child(
                div()
                    .flex_1()
                    .text_color(palette().text_muted)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
                    .font_family(MONO_FONT_FAMILY)
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
            palette().row_selected
        } else {
            palette().surface
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
                                .border_color(palette().border)
                                .bg(palette().element_bg)
                                .text_color(palette().text)
                                .text_size(px(FILE_TREE_BADGE_TEXT_SIZE))
                                .font_family(MONO_FONT_FAMILY)
                                .debug_selector(move || binary_selector.clone())
                                .child("Binary"),
                        )
                    })
                    .when_some(file.old_path.clone(), |row, old_path| {
                        row.child(
                            div()
                                .text_color(palette().text_muted)
                                .text_size(px(FILE_TREE_SECONDARY_TEXT_SIZE))
                                .font_family(MONO_FONT_FAMILY)
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
            palette().row_selected
        } else {
            palette().surface
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
                palette().text_muted.into(),
            ))
            .child(
                div()
                    .flex_1()
                    .text_color(palette().text)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
                    .font_family(MONO_FONT_FAMILY)
                    .whitespace_nowrap()
                    .child(display_name.to_string()),
            )
    }

    pub(crate) fn render_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        selected_path: Option<&str>,
        pane_render: PaneRenderContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match selected_path {
            Some(path) => {
                if let Some(file) = changeset.files.iter().find(|file| file.path == path) {
                    return self.render_changed_file_detail(repo, changeset, file, pane_render, cx);
                }

                self.render_read_only_file_detail(repo, changeset, path, pane_render, cx)
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
                .text_color(palette().text_muted)
                .text_size(px(14.))
                .child("Select a file to inspect its diff.")
                .into_any_element(),
        }
    }

    /// The selection-painting bundle for `path`'s diff in `pane`, shared by
    /// the changed- and read-only-file render paths. `side`/`content` are
    /// placeholders the callee specializes per side via `clone_for_side`.
    fn diff_selection_context(
        &self,
        pane: crate::workspace::PaneId,
        path: &str,
        cx: &mut Context<Self>,
    ) -> diff_view::DiffSelectionContext {
        let scroll = self.pane_scroll(pane, cx);
        diff_view::DiffSelectionContext {
            pane,
            key: path.to_string(),
            // Placeholder, replaced by `clone_for_side` for each side that
            // actually renders.
            side: repo::DiffSide::New,
            content: diff_selection::DiffSideContent::ReadOnly {
                cells: Rc::new(Vec::new()),
            },
            selection: self.diff_selection(pane, path),
            focus: scroll.focus,
            caret_visible: self.caret_blink_visible,
            app: cx.entity(),
            origins: scroll.content_origins,
            hscroll: scroll.diff.hscroll.clone(),
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

        let diff = if changeset.commit_sha == repo::PENDING_SHA {
            repo::file_diff_for_pending_file(&repo.path, file)
        } else {
            repo::file_diff_for_changed_file_between(
                &repo.path,
                &changeset.commit_sha,
                changeset.base_sha.as_deref(),
                file,
            )
        }
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
        pane_render: PaneRenderContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PaneRenderContext {
            pane,
            scroll,
            hovered,
        } = pane_render;
        let rename_source_selector = format!(
            "file-detail-rename-source-{}",
            debug_path_fragment(&file.path)
        );
        let prepared = self.prepared_file_diff(repo, changeset, file);
        if let Ok(prepared) = prepared.as_ref() {
            // On first display of this diff, land on its first change block
            // before the footer and content read the scroll offset.
            focus_first_change_block(prepared, scroll);
        }
        let footer = prepared
            .as_ref()
            .ok()
            .and_then(|prepared| render_change_block_footer(prepared, scroll));
        let selection_ctx = self.diff_selection_context(pane, &file.path, cx);
        let content = match prepared {
            Ok(prepared) => {
                render_prepared_file_diff(&prepared, scroll, hovered, Some(&selection_ctx))
            }
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
                        .border_color(palette().border)
                        .text_color(palette().text_muted)
                        .text_size(px(12.))
                        .font_family(MONO_FONT_FAMILY)
                        .debug_selector(move || rename_source_selector.clone())
                        .child(format!("Renamed from {old_path}")),
                )
            })
            .child(content)
            .when_some(footer, |detail, footer| detail.child(footer))
            .into_any_element()
    }

    fn render_read_only_file_detail(
        &self,
        repo: &repo::OpenRepository,
        changeset: &repo::ChangeSet,
        path: &str,
        pane_render: PaneRenderContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PaneRenderContext {
            pane,
            scroll,
            hovered,
        } = pane_render;
        let selection_ctx = self.diff_selection_context(pane, path, cx);
        let content = if changeset.commit_sha == repo::PENDING_SHA {
            repo::file_content_in_worktree(&repo.path, path)
        } else {
            repo::file_content_at_commit(&repo.path, &changeset.commit_sha, path)
        };
        let content = match content {
            Ok(content) => render_file_content(
                content.content,
                scroll,
                diff_highlight::language_for_path(path),
                hovered,
                Some(&selection_ctx),
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
        graph_rows: &[graph::GraphRow],
        max_graph_lanes: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
            .font_family(MONO_FONT_FAMILY)
            .gap_3()
            .px_4()
            // No background: the selection highlight paints from the list's
            // underlay canvas (see `render_graph_screen`), and an opaque row
            // background here would erase the graph bend overlays the row
            // above draws across the shared border.
            .when(commit_row_separator_width() > 0., |row| {
                row.border_b(px(commit_row_separator_width()))
                    .border_color(commit_row_separator_color(selected))
            })
            .cursor_pointer()
            .id(("commit-row", index))
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(move |app, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    // The first click may have shifted the rows (selection
                    // bar appearing/disappearing above the graph), so this
                    // row may not be the one the gesture started on; the
                    // anchor is (see its field doc).
                    let sha = app
                        .double_click_anchor
                        .take()
                        .unwrap_or_else(|| sha.clone());
                    app.selection = Selection::Single { sha };
                    app.open_changeset(window, cx);
                } else {
                    app.double_click_anchor = Some(sha.clone());
                    app.select_commit(sha.clone(), event.modifiers(), window, cx);
                }
            }))
            .child(render_commit_graph_gutter(
                index,
                &graph_rows[index],
                index.checked_sub(1).and_then(|prev| graph_rows.get(prev)),
                graph_rows.get(index + 1),
                max_graph_lanes,
                CommitDotStyle::Solid,
            ))
            .child(
                div()
                    .w(px(COMMIT_HASH_WIDTH))
                    .flex_shrink_0()
                    .text_color(palette().commit_hash_fg)
                    .text_size(px(12.))
                    .font_family(MONO_FONT_FAMILY)
                    .debug_selector(move || format!("commit-hash-{index}"))
                    .child(commit.short_sha.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette().text)
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
                    .text_color(palette().text_muted)
                    .text_size(px(12.))
                    .truncate()
                    .debug_selector(move || format!("commit-author-{index}"))
                    .child(commit.author.clone()),
            )
            .child(
                div()
                    .w(px(COMMIT_TIME_WIDTH))
                    .flex_shrink_0()
                    .text_color(palette().text_muted)
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

    /// Render the synthetic pending-changes row that always tops the graph
    /// (row 0). Mirrors `render_commit_row`'s shell (height, gap, padding,
    /// click gesture) but has no commit metadata of its own; its gutter dot
    /// renders hollow to read as distinct from committed rows.
    fn render_pending_row(
        &self,
        graph_rows: &[graph::GraphRow],
        max_graph_lanes: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let debug_selector = if selected {
            "selected-pending-row".to_string()
        } else {
            "pending-row".to_string()
        };
        let summary = self.pending_summary;
        let summary_text = if summary.is_dirty() {
            format!(
                "{} file{} changed   +{} \u{2212}{}",
                summary.file_count,
                if summary.file_count == 1 { "" } else { "s" },
                summary.line_stats.added,
                summary.line_stats.removed,
            )
        } else {
            "No pending changes".to_string()
        };

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(COMMIT_ROW_HEIGHT))
            .font_family(MONO_FONT_FAMILY)
            .gap_3()
            .px_4()
            .cursor_pointer()
            .id("pending-row")
            .debug_selector(move || debug_selector.clone())
            .on_click(cx.listener(|app, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    app.double_click_anchor = None;
                    app.selection = Selection::Pending;
                    app.open_changeset(window, cx);
                } else {
                    app.select_commit(repo::PENDING_SHA.to_string(), event.modifiers(), window, cx);
                }
            }))
            .child(render_commit_graph_gutter(
                0,
                &graph_rows[0],
                None,
                graph_rows.get(1),
                max_graph_lanes,
                CommitDotStyle::Hollow,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette().text)
                    .text_size(px(14.))
                    .truncate()
                    .debug_selector(|| "pending-title".to_string())
                    .child("Pending changes"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(palette().text_muted)
                    .text_size(px(12.))
                    .debug_selector(|| "pending-summary".to_string())
                    .child(summary_text),
            )
    }
}

const COMMIT_ROW_HEIGHT: f32 = 44.;
const COMMIT_HASH_WIDTH: f32 = 72.;
const COMMIT_AUTHOR_WIDTH: f32 = 168.;
const COMMIT_TIME_WIDTH: f32 = 96.;

/// Width reserved on the right edge of the history list for the scrollbar;
/// the commit rows and the selection underlay both stop short of it.
const COMMIT_HISTORY_SCROLLBAR_GUTTER: f32 = 12.;

/// The rectangles the selection underlay paints behind the selected commit
/// rows: one row-sized rect per selected index, shifted by the list's current
/// scroll offset and clipped to the list bounds so nothing paints outside the
/// visible list.
pub(crate) fn commit_history_selection_underlay_rects(
    bounds: gpui::Bounds<gpui::Pixels>,
    scroll_offset_y: gpui::Pixels,
    selected_indices: &[usize],
) -> Vec<gpui::Bounds<gpui::Pixels>> {
    let width = (bounds.size.width - px(COMMIT_HISTORY_SCROLLBAR_GUTTER)).max(px(0.));
    selected_indices
        .iter()
        .filter_map(|index| {
            let top = bounds.origin.y + scroll_offset_y + px(*index as f32 * COMMIT_ROW_HEIGHT);
            let row = gpui::Bounds::new(
                point(bounds.origin.x, top),
                gpui::size(width, px(COMMIT_ROW_HEIGHT)),
            );
            let clipped = row.intersect(&bounds);
            (clipped.size.height > px(0.)).then_some(clipped)
        })
        .collect()
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
                if app.branch_filter_has_focus(window, cx) {
                    return;
                }
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
            .on_action(cx.listener(|app, _: &NextChangeBlock, _window, cx| {
                app.navigate_change_block(true, cx);
            }))
            .on_action(cx.listener(|app, _: &PreviousChangeBlock, _window, cx| {
                app.navigate_change_block(false, cx);
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

/// Resolve a persisted sidebar width to the value to open at: the saved width
/// when it is a sane positive value at least [`SIDEBAR_MIN_WIDTH`], the minimum
/// when a saved value is present but too small/corrupt (including negatives and
/// NaN), or `default` when nothing has been saved.
fn restored_width(saved: Option<f32>, default: f32) -> f32 {
    match saved {
        Some(width) if width >= SIDEBAR_MIN_WIDTH => width,
        Some(_) => SIDEBAR_MIN_WIDTH,
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        restored_width, selection_summary, App, CloseChangeset, DiffDrag, DiffDragMode,
        FileListMode, Mode, OpenChangeset, OpenFailed, PreparedFileDiff, ReviewScreen, Selection,
        FILE_TREE_ROW_HEIGHT, SIDEBAR_MIN_WIDTH,
    };
    use crate::repo::{ChangeKind, INITIAL_COMMIT_LIMIT};
    use crate::settings::{RecentRepository, Settings, SidebarWidths, WindowMode};
    use crate::workspace::test_util::simulate_double_click;
    use git2::{IndexAddOption, Repository, Signature};
    use gpui::{px, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
    use std::{fs, rc::Rc};

    use super::test_support::*;

    #[gpui::test]
    fn closing_the_changeset_cancels_ai_threads(cx: &mut TestAppContext) {
        let window = crate::app::test_support::add_app_window(cx);

        // Start a long-running AI thread against a stub CLI, then close the
        // changeset; the thread must be cancelled and nothing left running.
        let dir = tempfile::tempdir().expect("tempdir");
        let stub = dir.path().join("claude-stub.sh");
        std::fs::write(&stub, "#!/bin/sh\nsleep 300\nexit 0\n").expect("write stub");
        let mut perms = std::fs::metadata(&stub).expect("stat").permissions();
        use std::os::unix::fs::PermissionsExt as _;
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub, perms).expect("chmod");

        let id = window
            .update(cx, |app, _window, cx| {
                app.ai_sessions().update(cx, |sessions, cx| {
                    *sessions = crate::ai::AiSessions::with_cli_program(stub.clone());
                    sessions.start_thread(
                        dir.path().to_path_buf(),
                        crate::ai::ThreadKind::Ask,
                        None,
                        "q".to_string(),
                        None,
                        cx,
                    )
                })
            })
            .expect("window update")
            .expect("start thread");

        window
            .update(cx, |app, _window, cx| {
                app.close_changeset(cx);
                app.ai_sessions().read(cx).running_count()
            })
            .map(|running| assert_eq!(running, 0))
            .expect("window update");

        window
            .update(cx, |app, _window, cx| {
                let status = app
                    .ai_sessions()
                    .read(cx)
                    .thread(id)
                    .expect("thread exists")
                    .status
                    .clone();
                assert_eq!(status, crate::ai::ThreadStatus::Cancelled);
            })
            .expect("window update");
    }

    #[test]
    fn selection_underlay_rects_follow_scroll_and_clip_to_the_list() {
        use super::{
            commit_history_selection_underlay_rects, COMMIT_HISTORY_SCROLLBAR_GUTTER,
            COMMIT_ROW_HEIGHT,
        };
        use gpui::{point, size, Bounds};

        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(500.), px(200.)));

        // Unscrolled, row 1 sits one row height below the list top and stops
        // short of the scrollbar gutter.
        let rects = commit_history_selection_underlay_rects(bounds, px(0.), &[1]);
        assert_eq!(rects.len(), 1);
        assert_eq!(
            rects[0].origin,
            point(px(100.), px(50. + COMMIT_ROW_HEIGHT))
        );
        assert_eq!(
            rects[0].size,
            size(
                px(500. - COMMIT_HISTORY_SCROLLBAR_GUTTER),
                px(COMMIT_ROW_HEIGHT)
            ),
        );

        // Scrolling up by half a row shifts the highlight with the rows.
        let rects =
            commit_history_selection_underlay_rects(bounds, px(-COMMIT_ROW_HEIGHT / 2.), &[1]);
        assert_eq!(
            rects[0].origin.y,
            px(50. + COMMIT_ROW_HEIGHT - COMMIT_ROW_HEIGHT / 2.)
        );

        // A row scrolled partly above the list clips to the list's top edge...
        let rects =
            commit_history_selection_underlay_rects(bounds, px(-COMMIT_ROW_HEIGHT / 2.), &[0]);
        assert_eq!(rects[0].origin.y, px(50.));
        assert_eq!(rects[0].size.height, px(COMMIT_ROW_HEIGHT / 2.));

        // ...and rows entirely outside the list paint nothing.
        let rects =
            commit_history_selection_underlay_rects(bounds, px(-COMMIT_ROW_HEIGHT), &[0, 40]);
        assert!(rects.is_empty());
    }

    #[test]
    fn restored_width_uses_default_when_unset_and_clamps_bad_values() {
        // Unset -> default.
        assert_eq!(restored_width(None, 240.), 240.);
        // Valid saved value passes through.
        assert_eq!(restored_width(Some(300.), 240.), 300.);
        // Below the minimum clamps up to the minimum.
        assert_eq!(restored_width(Some(10.), 240.), SIDEBAR_MIN_WIDTH);
        // Negative and NaN are treated as corrupt and clamped to the minimum.
        assert_eq!(restored_width(Some(-5.), 240.), SIDEBAR_MIN_WIDTH);
        assert_eq!(restored_width(Some(f32::NAN), 240.), SIDEBAR_MIN_WIDTH);
    }

    #[gpui::test]
    async fn restores_saved_branch_sidebar_width_on_render(cx: &mut TestAppContext) {
        let (dir, _) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();

        // Seed a non-default width, then build the app from those settings.
        let window = add_app_window_with_recent_and_widths(
            cx,
            vec![RecentRepository::available(path.clone())],
            SidebarWidths {
                branch_sidebar: Some(400.),
                changeset_files: None,
            },
        );
        cx.run_until_parked();

        let left = window
            .update(cx, |app, _window, cx| {
                app.graph_resizable
                    .read(cx)
                    .sizes()
                    .first()
                    .copied()
                    .map(f32::from)
            })
            .expect("read graph split sizes");

        let left = left.expect("branch sidebar should have a measured width after render");
        assert!(
            (left - 400.).abs() <= 2.0,
            "restored branch sidebar width should be ~400, got {left}"
        );
    }

    #[gpui::test]
    async fn closing_the_window_persists_its_bounds(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = dir.path().join("settings.json");
        let window = add_app_window_with_store_path(cx, store.clone());

        let expected = window
            .update(cx, |_app, window, _cx| window.window_bounds())
            .expect("read window bounds");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.simulate_close(), "window should accept the close");

        let state = crate::settings::load(&store)
            .window_state
            .expect("window state persisted on close");
        assert_eq!(state.mode, WindowMode::Windowed);
        let gpui::WindowBounds::Windowed(bounds) = expected else {
            panic!("test platform always reports windowed bounds");
        };
        assert_eq!(state.width, f32::from(bounds.size.width));
        assert_eq!(state.height, f32::from(bounds.size.height));
        assert!(!state.display.is_empty());
    }

    #[gpui::test]
    async fn closing_the_window_persists_the_branch_sidebar_width(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = dir.path().join("settings.json");
        let (repo_dir, _) = init_repo_with_one_commit();
        let repo_path = repo_dir.path().to_path_buf();

        let window = add_app_window_with_store_path(cx, store.clone());
        // Render the graph split so its ResizableState measures a width.
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_path, window, cx);
            })
            .expect("open repo");
        cx.run_until_parked();

        let measured = window
            .update(cx, |app, _window, cx| {
                app.graph_resizable
                    .read(cx)
                    .sizes()
                    .first()
                    .copied()
                    .map(f32::from)
            })
            .expect("read sizes")
            .expect("branch sidebar measured after render");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.simulate_close(), "window should accept the close");

        let saved = crate::settings::load(&store)
            .sidebar_widths
            .branch_sidebar
            .expect("branch sidebar width persisted on close");
        assert!(
            (saved - measured).abs() <= 0.5,
            "persisted width {saved} should match measured {measured}"
        );
    }

    #[gpui::test]
    async fn closing_without_rendering_a_split_does_not_clobber_its_saved_width(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let store = dir.path().join("settings.json");
        // Seed a previously-saved changeset width on disk.
        crate::settings::save(
            &store,
            &Settings {
                recent_repositories: vec![],
                window_state: None,
                sidebar_widths: SidebarWidths {
                    branch_sidebar: None,
                    changeset_files: Some(333.0),
                },
                ai_enabled: false,
            },
        )
        .expect("seed settings");

        // Open a window (no repo -> changeset split never renders) and close it.
        let window = add_app_window_with_store_path(cx, store.clone());
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.simulate_close(), "window should accept the close");

        // The unrendered changeset width must be preserved, not overwritten.
        assert_eq!(
            crate::settings::load(&store).sidebar_widths.changeset_files,
            Some(333.0),
            "an unrendered split must not clobber its saved width"
        );
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
    async fn graph_mode_lists_local_branches_without_a_head_check_marker(cx: &mut TestAppContext) {
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
        // The checked-out branch's tip is the default selection on open, so
        // its row renders with the selected treatment.
        visual
            .debug_bounds("selected-branch-row-heads-master")
            .expect("master branch row renders");
        // The checked-out branch is now distinguished by its row background
        // rather than a check icon, so no marker element renders for it.
        assert!(
            visual
                .debug_bounds("branch-head-marker-heads-master")
                .is_none(),
            "the checked-out branch carries no check marker",
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
                let offset = app.commit_history_scroll.0.borrow().base_handle.offset();
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
                app.pane_scroll(0, cx);
                app.pane_scroll(1, cx);
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

    /// Open the three-block modified file in a single pane at a size that
    /// keeps the diff scrollable, returning the temp repo (kept alive by the
    /// caller), the window, and its visual context. Mirrors the
    /// identically-named helper in `diff_view`'s test module, which is
    /// private to that module.
    fn open_multi_block_diff(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        use gpui::size;

        let (dir, oid_hex) = init_repo_with_multiple_change_blocks();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("blocks.txt".to_string(), cx);
            })
            .expect("open multi-block diff");

        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(360.)));
        cx.run_until_parked();

        (dir, window, visual)
    }

    #[gpui::test]
    async fn diff_selection_is_pruned_when_its_tab_closes(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_dir, window, _visual) = open_multi_block_diff(cx);
        let (pane, key) = window
            .read_with(cx, |app, _| {
                let pane = app.workspace.active_pane();
                let key = app.workspace.active_item(pane).unwrap().key().to_string();
                (pane, key)
            })
            .unwrap();
        window
            .update(cx, |app, _window, cx| {
                let point = crate::app::diff_selection::DiffPoint { row: 0, column: 0 };
                let selection = crate::app::diff_selection::DiffSelection::caret_at(
                    point,
                    crate::repo::DiffSide::New,
                );
                app.set_diff_selection(pane, &key, selection, cx);
                assert!(app.diff_selection(pane, &key).is_some());
            })
            .unwrap();
        // Close the tab; the selection entry must go with it.
        window
            .update(cx, |app, _window, cx| app.close_active_workspace_tab(cx))
            .unwrap();
        cx.run_until_parked();
        window
            .read_with(cx, |app, _| {
                assert!(app.diff_selection(pane, &key).is_none())
            })
            .unwrap();
    }

    #[gpui::test]
    async fn pane_scroll_state_carries_a_focus_handle_and_content_origins(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_dir, window, _visual) = open_multi_block_diff(cx);
        window
            .update(cx, |app, window, cx| {
                let pane = app.workspace.active_pane();
                let scroll = app.pane_scroll(pane, cx);
                // The handle is live and distinct per pane: focusing it and
                // reading it back round-trips through gpui's focus system.
                scroll.focus.focus(window);
                assert!(scroll.focus.is_focused(window));

                // Task 7's `render_file_diff_side` canvas already wrote real
                // bounds for the sides this diff rendered.
                assert!(
                    !scroll.content_origins.borrow().is_empty(),
                    "a rendered diff should have recorded its sides' bounds"
                );
                let bounds =
                    gpui::Bounds::new(gpui::point(px(0.), px(0.)), gpui::size(px(10.), px(10.)));
                scroll.content_origins.borrow_mut().insert("new", bounds);
                assert_eq!(scroll.content_origins.borrow().get("new"), Some(&bounds));
            })
            .unwrap();
    }

    #[gpui::test]
    async fn diff_drag_field_holds_and_clears_an_in_flight_drag(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_dir, window, _visual) = open_multi_block_diff(cx);
        window
            .update(cx, |app, _window, _cx| {
                let pane = app.workspace.active_pane();
                let key = app.workspace.active_item(pane).unwrap().key().to_string();
                assert!(app.diff_drag.is_none());

                app.diff_drag = Some(DiffDrag {
                    pane,
                    key: key.clone(),
                    mode: DiffDragMode::Character,
                });
                let drag = app.diff_drag.as_ref().expect("drag was just set");
                assert_eq!(drag.pane, pane);
                assert_eq!(drag.key, key);
                assert_eq!(drag.mode, DiffDragMode::Character);

                let origin = (
                    crate::app::diff_selection::DiffPoint { row: 0, column: 0 },
                    crate::app::diff_selection::DiffPoint { row: 0, column: 3 },
                );
                app.diff_drag = Some(DiffDrag {
                    pane,
                    key: key.clone(),
                    mode: DiffDragMode::Word { origin },
                });
                assert_eq!(
                    app.diff_drag.as_ref().map(|drag| drag.mode.clone()),
                    Some(DiffDragMode::Word { origin })
                );

                app.diff_drag = Some(DiffDrag {
                    pane,
                    key,
                    mode: DiffDragMode::Line { origin },
                });
                assert_eq!(
                    app.diff_drag.as_ref().map(|drag| drag.mode.clone()),
                    Some(DiffDragMode::Line { origin })
                );

                app.diff_drag = None;
                assert!(app.diff_drag.is_none());
            })
            .unwrap();
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
    async fn selecting_the_selected_commit_again_keeps_it_selected(cx: &mut TestAppContext) {
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
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: "second-sha".to_string(),
                    },
                    "re-selecting the selected commit must not clear the selection",
                );
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn opening_a_repository_selects_the_checked_out_tip(cx: &mut TestAppContext) {
        let (dir, main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: main_tip.clone()
                    },
                    "opening a repository must select the checked-out tip by default",
                );
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn restoring_a_repository_at_startup_selects_the_checked_out_tip(
        cx: &mut TestAppContext,
    ) {
        let (dir, main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let window = add_app_window_with_recent_and_widths(
            cx,
            vec![RecentRepository::available(dir.path().to_path_buf())],
            SidebarWidths::default(),
        );

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: main_tip.clone()
                    },
                    "restoring the most recent repository at startup must select its checked-out tip",
                );
            })
            .expect("read startup selection");
    }

    #[gpui::test]
    async fn hiding_a_branch_resets_a_selection_it_made_invisible(cx: &mut TestAppContext) {
        let (dir, main_tip, feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(feature_tip.clone(), cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: main_tip.clone()
                    },
                    "hiding the selection's branch must reset the selection to the checked-out tip",
                );
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
        // Three commits loaded (rows 1..4, row 0 is the pending row), one
        // (the feature-exclusive commit) hidden.
        assert!(visual.debug_bounds("commit-row-2").is_some());
        assert!(
            visual.debug_bounds("commit-row-3").is_none(),
            "the feature-exclusive commit must not render"
        );
        // The feature ref label is gone from every remaining row.
        for row in 1..3usize {
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
        // commit rows back (rows 1..4, row 0 is the pending row), the feature
        // label on exactly one of them.
        for row in 1..4 {
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
        let feature_label_rows = (1..4)
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
        // Focusing master must select the row at its *visible* index. Row 0 is
        // always the pending row, so visible commit indices shift by one.
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
        // Master's tip is the default selection on open, so its sidebar row
        // already renders with the selected treatment.
        let master_row = visual
            .debug_bounds("selected-branch-row-heads-master")
            .expect("master branch row renders");
        visual.simulate_click(master_row.center(), Modifiers::none());

        // With feature hidden the visible order is: pending (0), master tip
        // (1), root (2).
        visual
            .debug_bounds("selected-commit-row-1")
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
        // The tip is the checked-out commit, selected by default on open.
        // Row 0 is always the pending row, so the tip sits at row 1.
        let tip_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("tip commit row debug bounds");
        visual.simulate_click(tip_bounds.center(), Modifiers::none());

        let root_bounds = visual
            .debug_bounds("commit-row-3")
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
            .debug_bounds("selected-commit-row-1")
            .expect("selected tip row debug bounds");
        visual
            .debug_bounds("selected-commit-row-2")
            .expect("selected middle row debug bounds");
        visual
            .debug_bounds("selected-commit-row-3")
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
            .debug_bounds("commit-row-2")
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
    async fn double_clicking_the_top_commit_opens_its_changeset_despite_the_bar(
        cx: &mut TestAppContext,
    ) {
        // The selection bar sits above the graph; historically it appeared
        // after the first click and pushed the top row down so the second
        // click landed on the bar itself. The bar now renders from the
        // moment the repository opens, but the gesture must still open the
        // changeset of the commit under the first click.
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
        // The tip is the checked-out commit, selected by default on open.
        let row_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("tip commit row debug bounds");
        simulate_double_click(&mut visual, row_bounds.center());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: shas[0].clone()
                    },
                    "double-click selects the commit under the first click"
                );
                match &app.review_screen {
                    ReviewScreen::Changeset { sha, .. } => assert_eq!(sha, &shas[0]),
                    ReviewScreen::Graph => {
                        panic!("double-clicking the top commit must open its changeset")
                    }
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
        // The tip is the checked-out commit, selected by default on open.
        let tip_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("tip commit row debug bounds");
        visual.simulate_click(tip_bounds.center(), Modifiers::none());
        let root_bounds = visual
            .debug_bounds("commit-row-3")
            .expect("root commit row debug bounds");
        visual.simulate_click(root_bounds.center(), Modifiers::shift());

        // Range tip..root is selected; double-click the middle commit.
        let middle_bounds = visual
            .debug_bounds("selected-commit-row-2")
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
            .debug_bounds("commit-row-2")
            .expect("middle commit row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        let selected_bounds = visual
            .debug_bounds("selected-commit-row-2")
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
    async fn plain_click_gesture_selects_pending(cx: &mut TestAppContext) {
        let (dir, _sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(
                    crate::repo::PENDING_SHA.to_string(),
                    Modifiers::none(),
                    window,
                    cx,
                );

                assert_eq!(app.selection, Selection::Pending);
                assert_eq!(
                    selection_summary(&app.selection).as_deref(),
                    Some("Pending changes selected")
                );
            })
            .expect("select the pending row");
    }

    #[gpui::test]
    async fn range_and_compare_gestures_involving_pending_are_rejected(cx: &mut TestAppContext) {
        use std::cell::RefCell;
        use std::rc::Rc;

        let (dir, sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);
        let app_entity = window.entity(cx).expect("get app entity");

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

                // Shift-click onto the pending row from a commit selection.
                app.selection = Selection::Single { sha: sha.clone() };
                app.select_commit(
                    crate::repo::PENDING_SHA.to_string(),
                    Modifiers::shift(),
                    window,
                    cx,
                );
                assert_eq!(app.selection, Selection::Single { sha: sha.clone() });

                // Shift-click onto a commit while pending is selected.
                app.selection = Selection::Pending;
                app.select_commit(sha.clone(), Modifiers::shift(), window, cx);
                assert_eq!(app.selection, Selection::Pending);

                // Cmd-click both directions.
                app.select_commit(sha.clone(), Modifiers::secondary_key(), window, cx);
                assert_eq!(app.selection, Selection::Pending);

                app.selection = Selection::Single { sha: sha.clone() };
                app.select_commit(
                    crate::repo::PENDING_SHA.to_string(),
                    Modifiers::secondary_key(),
                    window,
                    cx,
                );
                assert_eq!(app.selection, Selection::Single { sha: sha.clone() });
            })
            .expect("attempt gestures involving pending");

        cx.run_until_parked();

        let events = captured.borrow();
        assert_eq!(events.len(), 4, "exactly four rejections emitted");
        assert!(
            events
                .iter()
                .all(|message| message == "Pending changes can only be reviewed on their own."),
            "unexpected rejection message(s): {:?}",
            events
        );
    }

    #[gpui::test]
    async fn pending_row_tops_the_graph_with_an_edge_to_head(cx: &mut TestAppContext) {
        let (dir, _sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        fs::write(dir.path().join("dirty.txt"), "dirt\n").expect("write dirty file");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        let mut visual = VisualTestContext::from_window(*window, cx);

        let pending = visual
            .debug_bounds("pending-row")
            .expect("pending row renders");
        let head_row = visual
            .debug_bounds("selected-commit-row-1")
            .expect("HEAD commit row sits below the pending row");
        assert!(pending.origin.y < head_row.origin.y);
        // The pending summary shows the dirty state.
        assert!(visual.debug_bounds("pending-summary").is_some());
        // The synthetic edge descends from the pending row to HEAD in lane 0:
        // the pending row has an outgoing lane-0 vertical.
        assert!(visual
            .debug_bounds("commit-graph-vertical-0-0-bottom")
            .is_some());
    }

    #[gpui::test]
    async fn clean_tree_pending_row_reads_no_pending_changes(cx: &mut TestAppContext) {
        let (dir, _sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                assert!(!app.pending_summary.is_dirty());
            })
            .expect("open repo");

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("pending-row").is_some());
    }

    #[gpui::test]
    async fn clicking_the_pending_row_selects_it(cx: &mut TestAppContext) {
        let (dir, _sha) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        let mut visual = VisualTestContext::from_window(*window, cx);

        let pending = visual.debug_bounds("pending-row").expect("pending row");
        visual.simulate_click(pending.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.selection, Selection::Pending);
            })
            .expect("read selection");
        assert!(visual.debug_bounds("selected-pending-row").is_some());
    }

    #[gpui::test]
    async fn double_clicking_pending_opens_the_pending_changeset(cx: &mut TestAppContext) {
        let (repo_dir, _sha) = init_repo_with_one_commit();
        fs::write(repo_dir.path().join("dirty.txt"), "dirt\n").expect("write dirty file");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_dir.path().to_path_buf(), window, cx);
            })
            .expect("open repo");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let pending = visual.debug_bounds("pending-row").expect("pending row");
        simulate_double_click(&mut visual, pending.center());

        window
            .read_with(cx, |app, _cx| {
                let ReviewScreen::Changeset { sha, changeset } = &app.review_screen else {
                    panic!("double-clicking the pending row must open the pending changeset");
                };
                assert_eq!(sha, crate::repo::PENDING_SHA);
                assert!(changeset.files.iter().any(|file| file.path == "dirty.txt"));
            })
            .expect("read review screen");
    }

    #[gpui::test]
    async fn pending_changeset_renders_a_worktree_file_diff(cx: &mut TestAppContext) {
        let (repo_dir, _sha) = init_repo_with_one_commit();
        // Modify the fixture's tracked file so the pending diff is
        // side-by-side: old side from HEAD, new side from the worktree.
        fs::write(repo_dir.path().join("hello.txt"), "changed\n").expect("modify tracked file");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_dir.path().to_path_buf(), window, cx);
                app.selection = Selection::Pending;
                app.open_changeset(window, cx);
            })
            .expect("open pending changeset");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let file_row = visual
            .debug_bounds("changed-file-name-hello.txt")
            .expect("changed file row");
        visual.simulate_click(file_row.center(), Modifiers::none());

        assert!(visual.debug_bounds("file-detail-shell").is_some());
        assert!(
            visual.debug_bounds("file-diff-error").is_none(),
            "pending file diff must read from the worktree, not a commit lookup"
        );
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
    async fn cmd_clicking_a_second_commit_creates_a_comparison_anchored_at_the_selection(
        cx: &mut TestAppContext,
    ) {
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");
        cx.run_until_parked();

        let (left_index, right_index) = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                // Row 0 is always the pending row, so a commit's graph row is
                // one past its position in `repo.commits`.
                let position = |sha: &str| {
                    repo.commits
                        .iter()
                        .position(|commit| commit.sha == sha)
                        .expect("commit row")
                        + 1
                };
                (position(&left_sha), position(&right_sha))
            })
            .expect("read commit row indexes");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let left_bounds = visual
            .debug_bounds(test_debug_selector(format!("commit-row-{left_index}")))
            .expect("left commit row debug bounds");
        visual.simulate_click(left_bounds.center(), Modifiers::none());
        let right_bounds = visual
            .debug_bounds(test_debug_selector(format!("commit-row-{right_index}")))
            .expect("right commit row debug bounds");
        visual.simulate_click(right_bounds.center(), Modifiers::secondary_key());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Compare {
                        base_sha: right_sha.clone(),
                        target_sha: left_sha.clone(),
                    },
                    "cmd-click previews merging the anchored commit into the clicked one"
                );
            })
            .expect("read comparison selection");

        for index in [left_index, right_index] {
            assert!(
                visual
                    .debug_bounds(test_debug_selector(format!("selected-commit-row-{index}")))
                    .is_some(),
                "both comparison endpoints must render as selected rows"
            );
        }
    }

    #[gpui::test]
    async fn cmd_clicking_with_a_range_selection_uses_the_anchor_as_the_source(
        cx: &mut TestAppContext,
    ) {
        let (dir, shas) = init_repo_with_three_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(shas[0].clone(), Modifiers::none(), window, cx);
                app.select_commit(shas[2].clone(), Modifiers::shift(), window, cx);
                app.select_commit(shas[1].clone(), Modifiers::secondary_key(), window, cx);

                assert_eq!(
                    app.selection,
                    Selection::Compare {
                        base_sha: shas[1].clone(),
                        target_sha: shas[0].clone(),
                    },
                    "the range's first-clicked endpoint is the merge source"
                );
            })
            .expect("compare from a range selection");
    }

    #[gpui::test]
    async fn cmd_clicking_retargets_an_active_comparison_keeping_the_source(
        cx: &mut TestAppContext,
    ) {
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                let root_sha = match &app.mode {
                    Mode::RepoOpen { repo } => repo
                        .commits
                        .iter()
                        .find(|commit| commit.parent_shas.is_empty())
                        .expect("root commit")
                        .sha
                        .clone(),
                    Mode::NoRepo => panic!("expected repo open mode"),
                };
                app.select_commit(left_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(right_sha, Modifiers::secondary_key(), window, cx);
                app.select_commit(root_sha.clone(), Modifiers::secondary_key(), window, cx);

                assert_eq!(
                    app.selection,
                    Selection::Compare {
                        base_sha: root_sha,
                        target_sha: left_sha.clone(),
                    },
                    "a third cmd-click re-aims the preview while the source stays put"
                );
            })
            .expect("retarget comparison");
    }

    #[gpui::test]
    async fn cmd_clicking_the_base_or_target_leaves_the_comparison_unchanged(
        cx: &mut TestAppContext,
    ) {
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(left_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(right_sha.clone(), Modifiers::secondary_key(), window, cx);
                let comparison = app.selection.clone();

                app.select_commit(left_sha.clone(), Modifiers::secondary_key(), window, cx);
                assert_eq!(
                    app.selection, comparison,
                    "cmd-clicking the base is a no-op"
                );

                app.select_commit(right_sha.clone(), Modifiers::secondary_key(), window, cx);
                assert_eq!(
                    app.selection, comparison,
                    "cmd-clicking the target is a no-op"
                );
            })
            .expect("cmd-click comparison endpoints");
    }

    #[gpui::test]
    async fn cmd_clicking_disjoint_commits_preserves_selection_and_explains_why(
        cx: &mut TestAppContext,
    ) {
        let (dir, master_sha, orphan_sha) = init_repo_with_disjoint_roots();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(master_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(orphan_sha, Modifiers::secondary_key(), window, cx);

                assert_eq!(
                    app.selection,
                    Selection::Single { sha: master_sha },
                    "a comparison without common history is rejected, keeping the selection"
                );
            })
            .expect("attempt disjoint comparison");
        cx.run_until_parked();

        window
            .read_with(cx, |app, cx| {
                assert_eq!(
                    app.notification_count(cx),
                    1,
                    "the rejection surfaces an explanatory message"
                );
            })
            .expect("read notification count");
    }

    #[gpui::test]
    async fn plain_clicking_while_comparing_collapses_to_a_single_selection(
        cx: &mut TestAppContext,
    ) {
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(left_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(right_sha.clone(), Modifiers::secondary_key(), window, cx);
                app.select_commit(right_sha.clone(), Modifiers::none(), window, cx);

                assert_eq!(
                    app.selection,
                    Selection::Single { sha: right_sha },
                    "a plain click replaces the comparison with a single selection"
                );
            })
            .expect("collapse comparison with a plain click");
    }

    #[gpui::test]
    async fn swap_control_reverses_the_comparison_direction(cx: &mut TestAppContext) {
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(left_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(right_sha.clone(), Modifiers::secondary_key(), window, cx);
            })
            .expect("stage a comparison");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let swap_bounds = visual
            .debug_bounds("swap-comparison")
            .expect("swap control must be visible while a comparison is staged");
        visual.simulate_click(swap_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Compare {
                        base_sha: left_sha.clone(),
                        target_sha: right_sha.clone(),
                    },
                    "the swap control reverses the merge-preview direction"
                );
            })
            .expect("read swapped comparison");
    }

    #[gpui::test]
    async fn opening_a_comparison_shows_the_merge_base_diff(cx: &mut TestAppContext) {
        let (dir, left_sha, right_sha) = init_repo_with_diverged_history();
        let path = dir.path().to_path_buf();
        let merge_base = crate::repo::merge_base_sha(dir.path(), &left_sha, &right_sha)
            .expect("merge base lookup")
            .expect("diverged branches share a fork point");
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(left_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(right_sha.clone(), Modifiers::secondary_key(), window, cx);
            })
            .expect("stage a comparison");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                match &app.review_screen {
                    ReviewScreen::Changeset { changeset, .. } => {
                        assert_eq!(
                            changeset.commit_sha, left_sha,
                            "the first-selected commit is the side whose changes are shown"
                        );
                        assert_eq!(changeset.base_sha, Some(merge_base.clone()));
                        let paths = changeset
                            .files
                            .iter()
                            .map(|file| file.path.as_str())
                            .collect::<Vec<_>>();
                        assert_eq!(
                            paths,
                            vec!["left.txt"],
                            "only what merging the first-selected commit would introduce appears"
                        );
                    }
                    ReviewScreen::Graph => panic!("expected changeset review screen"),
                }
                assert_eq!(
                    app.comparison_commit_shas,
                    Some(vec![left_sha.clone()]),
                    "the commits the merge would introduce are captured at open time"
                );
            })
            .expect("read comparison changeset");
    }

    #[gpui::test]
    async fn hiding_a_branch_that_removes_a_comparison_endpoint_resets_to_the_checked_out_tip(
        cx: &mut TestAppContext,
    ) {
        let (dir, main_tip_sha, feature_tip_sha) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_commit(main_tip_sha.clone(), Modifiers::none(), window, cx);
                app.select_commit(
                    feature_tip_sha.clone(),
                    Modifiers::secondary_key(),
                    window,
                    cx,
                );
                assert_eq!(
                    app.selection,
                    Selection::Compare {
                        base_sha: feature_tip_sha.clone(),
                        target_sha: main_tip_sha.clone(),
                    },
                );

                app.toggle_branch_visibility("heads/feature".to_string(), cx);

                assert_eq!(
                    app.selection,
                    Selection::Single { sha: main_tip_sha },
                    "hiding an endpoint's branch resets the selection to the checked-out tip"
                );
            })
            .expect("hide comparison endpoint");
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

        // Row 0 is always the pending row, so a commit's graph row is one past
        // its position in `repo.commits`.
        let (merge_index, main_index, side_index) = window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let merge_index = repo
                    .commits
                    .iter()
                    .position(|commit| commit.sha == merge_sha)
                    .expect("merge commit row")
                    + 1;
                let main_index = repo
                    .commits
                    .iter()
                    .position(|commit| commit.sha == main_sha)
                    .expect("main commit row")
                    + 1;
                let side_index = repo
                    .commits
                    .iter()
                    .position(|commit| commit.sha == side_sha)
                    .expect("side commit row")
                    + 1;

                (merge_index, main_index, side_index)
            })
            .expect("read commit row indexes");

        let mut visual = VisualTestContext::from_window(*window, cx);
        // The merge commit is the checked-out tip, selected by default on open.
        let merge_bounds = visual
            .debug_bounds(test_debug_selector(format!(
                "selected-commit-row-{merge_index}"
            )))
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
                // Simulate a stale popover flag, then open the changeset:
                // opening must dismiss the popover.
                app.context_popover_open = true;
                app.open_changeset(window, cx);
            })
            .expect("open changeset with stale popover flag");

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
        // The tip is the checked-out commit, selected by default on open.
        let tip_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("tip commit row debug bounds");
        visual.simulate_click(tip_bounds.center(), Modifiers::none());

        let root_bounds = visual
            .debug_bounds("commit-row-3")
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
        // The revert commit is the checked-out tip, selected by default on open.
        let revert_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("revert commit row debug bounds");
        visual.simulate_click(revert_bounds.center(), Modifiers::none());

        let change_bounds = visual
            .debug_bounds("commit-row-2")
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
                    PreparedFileDiff::SideBySide { rows, .. } => assert!(!rows.is_empty()),
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
    async fn keyboard_selection_reuses_cached_read_only_cells(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                // The unchanged file opens read-only; keyboard resolution
                // needs a placed caret before it resolves any content.
                app.open_file_preview("context.txt".to_string(), cx);
                let pane = app.workspace.active_pane();
                app.set_diff_selection(
                    pane,
                    "context.txt",
                    crate::app::diff_selection::DiffSelection::caret_at(
                        crate::app::diff_selection::DiffPoint { row: 0, column: 0 },
                        crate::repo::DiffSide::New,
                    ),
                    cx,
                );
            })
            .expect("open read-only file");
        cx.run_until_parked();

        window
            .update(cx, |app, _window, _cx| {
                let first = match app.active_diff_selection_context() {
                    Some((
                        _,
                        _,
                        crate::app::diff_selection::DiffSideContent::ReadOnly { cells },
                        _,
                    )) => cells,
                    other => panic!("expected read-only content, got {:?}", other.is_some()),
                };
                let second = match app.active_diff_selection_context() {
                    Some((
                        _,
                        _,
                        crate::app::diff_selection::DiffSideContent::ReadOnly { cells },
                        _,
                    )) => cells,
                    other => panic!("expected read-only content, got {:?}", other.is_some()),
                };
                assert!(
                    Rc::ptr_eq(&first, &second),
                    "a repeated resolution should return the cached cells, not re-read the blob"
                );
                assert_eq!(
                    app.read_only_cell_cache.borrow().len(),
                    1,
                    "only one entry should be cached for the single open read-only file"
                );
            })
            .expect("inspect read-only cell cache");

        // Closing the changeset must invalidate the cache with the diff-row
        // cache, so a later changeset re-reads against its own commit.
        window
            .update(cx, |app, _window, cx| {
                app.close_changeset(cx);
                assert!(
                    app.read_only_cell_cache.borrow().is_empty(),
                    "closing the changeset should clear the read-only cell cache"
                );
            })
            .expect("verify invalidation");
    }

    #[gpui::test]
    async fn read_only_cells_read_pending_worktree_content(cx: &mut TestAppContext) {
        let (dir, _oid_hex) = init_repo_with_one_commit();
        // Dirty the tree with an unrelated new file so pending is non-empty;
        // hello.txt itself stays unchanged and opens read-only in the
        // all-files view.
        fs::write(dir.path().join("dirty.txt"), "dirty\n").expect("write dirty file");
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.selection = Selection::Pending;
                app.open_changeset(window, cx);
                app.file_list_mode = FileListMode::All;

                let repo = match &app.mode {
                    Mode::RepoOpen { repo } => repo,
                    Mode::NoRepo => panic!("expected an open repository"),
                };
                let changeset = match &app.review_screen {
                    ReviewScreen::Changeset { changeset, .. } => changeset,
                    ReviewScreen::Graph => panic!("expected the changeset screen"),
                };

                let cells = app.read_only_cells(repo, changeset, "hello.txt");
                let cells = cells.expect(
                    "read_only_cells should read the pending worktree content for an \
                     unchanged file, not fail on the pending sentinel sha",
                );
                assert!(!cells.is_empty(), "hello.txt has content to select");
            })
            .expect("resolve read-only cells for an unchanged file in the pending changeset");
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

        // Closing, selecting a different commit, and reopening (the supported
        // way to change the open changeset) must clear the cache so a later
        // render recomputes against the new commit.
        window
            .update(cx, |app, window, cx| {
                app.close_changeset(cx);
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
    async fn clicking_the_selected_commit_row_keeps_the_selection(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        // The checked-out tip is selected on open, so its row renders with
        // the selected treatment straight away. Row 0 is always the pending
        // row, so the tip sits at row 1.
        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("selected-commit-row-1")
            .expect("selected commit row debug bounds");

        visual.simulate_click(row_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: oid_hex.clone(),
                    },
                    "clicking the selected commit must keep it selected",
                );
            })
            .expect("read selected state");
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

    #[test]
    fn selection_summary_describes_single_and_range_counts() {
        assert_eq!(selection_summary(&Selection::None), None);
        assert_eq!(
            selection_summary(&Selection::Single {
                sha: "abc".to_string(),
            }),
            Some("1 commit selected".to_string()),
        );
        assert_eq!(
            selection_summary(&Selection::Range {
                start_sha: "a".to_string(),
                end_sha: "c".to_string(),
                shas: vec!["c".to_string(), "b".to_string(), "a".to_string()],
            }),
            Some("3 commits selected".to_string()),
        );
        assert_eq!(
            selection_summary(&Selection::Compare {
                base_sha: "0123456789abcdef".to_string(),
                target_sha: "fedcba9876543210".to_string(),
            }),
            Some("Merge preview: fedcba9 into 0123456".to_string()),
        );
    }

    #[gpui::test]
    async fn selection_bar_is_absent_only_when_the_graph_has_no_commits(cx: &mut TestAppContext) {
        let dir = init_repo_with_no_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                assert_eq!(
                    app.selection,
                    Selection::None,
                    "an empty graph is the only state without a selection",
                );
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("selection-bar").is_none(),
            "the selection bar renders only while a selection is active",
        );
    }

    #[gpui::test]
    async fn selection_bar_renders_on_open_for_the_default_selection(cx: &mut TestAppContext) {
        let (dir, _oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("selection-bar")
            .expect("the selection bar renders on open for the default checked-out-tip selection");
    }

    #[gpui::test]
    async fn selecting_a_commit_shows_the_selection_bar_with_the_open_action(
        cx: &mut TestAppContext,
    ) {
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
        visual
            .debug_bounds("selection-bar")
            .expect("selection bar renders while a selection is active");
        visual
            .debug_bounds("open-changeset")
            .expect("open-changeset affordance lives in the selection bar");
        assert!(
            visual.debug_bounds("reset-selection").is_none(),
            "the bar carries no reset affordance: a selection always exists, so \
             there is nothing to reset to",
        );
    }

    #[gpui::test]
    async fn selection_bar_renders_above_the_commit_history(cx: &mut TestAppContext) {
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
        let bar_bounds = visual
            .debug_bounds("selection-bar")
            .expect("selection-bar debug bounds");
        let history_bounds = visual
            .debug_bounds("commit-history")
            .expect("commit-history debug bounds");
        assert!(
            bar_bounds.bottom() <= history_bounds.top(),
            "the selection bar docks to the top of the history panel, pushing the \
             graph down (bar bottom {:?} vs history top {:?})",
            bar_bounds.bottom(),
            history_bounds.top(),
        );
    }

    #[gpui::test]
    async fn pressing_enter_opens_the_changeset_for_the_selection(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
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
        visual.simulate_keystrokes("enter");

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { sha, .. } => assert_eq!(sha, &oid_hex),
                ReviewScreen::Graph => panic!("enter should open the selected changeset"),
            })
            .expect("read review screen after enter");
    }

    #[gpui::test]
    async fn pressing_escape_leaves_the_selection_untouched(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (dir, _main_tip, feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(feature_tip.clone(), cx);
            })
            .expect("open repo and select the feature tip");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_keystrokes("escape");

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: feature_tip.clone()
                    },
                    "escape is not bound to any selection action",
                );
            })
            .expect("read selection after escape");
    }

    #[gpui::test]
    async fn pressing_enter_while_a_changeset_is_open_keeps_the_workspace_intact(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::app::bind_app_keys);
        let (dir, oid_hex) = init_repo_with_one_commit();
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
        visual.simulate_keystrokes("cmd-k right");
        window
            .read_with(cx, |app, _cx| {
                assert_eq!(app.workspace.pane_ids().len(), 2, "split created");
            })
            .expect("read split workspace");

        visual.simulate_keystrokes("enter");

        window
            .read_with(cx, |app, _cx| {
                assert!(
                    matches!(app.review_screen, ReviewScreen::Changeset { .. }),
                    "enter must not leave changeset mode",
                );
                assert_eq!(
                    app.workspace.pane_ids().len(),
                    2,
                    "enter must not rebuild the workspace while a changeset is open",
                );
            })
            .expect("read workspace after enter in changeset mode");
    }

    #[gpui::test]
    async fn pressing_enter_in_the_branch_filter_does_not_open_a_changeset(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(crate::app::bind_app_keys);
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();

        // The filter input reaches for `gpui_component::Root` when focused, so
        // this test mirrors production (`lib.rs`) and wraps the app in one;
        // `add_app_window` skips the wrapper because nothing else needs it.
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
                    app.open_repository_at(path, window, cx);
                    app.select_single_commit(oid_hex, cx);
                    let filter_focus = gpui::Focusable::focus_handle(&app.filter_input, cx);
                    window.focus(&filter_focus);
                });
            })
            .expect("open repo, select commit, focus filter");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_keystrokes("enter");

        app_entity.read_with(cx, |app, _cx| {
            assert_eq!(
                app.review_screen,
                ReviewScreen::Graph,
                "enter while typing in the branch filter must not open a changeset",
            );
        });
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

    #[gpui::test]
    async fn setting_an_identical_selection_does_not_touch_the_blink_epoch(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let (_dir, window, _visual) = open_multi_block_diff(cx);
        window
            .update(cx, |app, _window, cx| {
                let pane = app.workspace.active_pane();
                let key = app.workspace.active_item(pane).unwrap().key().to_string();
                let point = crate::app::diff_selection::DiffPoint { row: 0, column: 0 };
                let selection = crate::app::diff_selection::DiffSelection::caret_at(
                    point,
                    crate::repo::DiffSide::New,
                );

                // Placing the first caret is a real change: it must pause
                // (and thereby arm) the blink loop.
                app.set_diff_selection(pane, &key, selection, cx);
                let epoch_after_first_set = app.caret_blink_epoch;
                assert_ne!(
                    epoch_after_first_set, 0,
                    "placing a caret bumps the blink epoch"
                );

                // Setting the exact same selection again must be a no-op:
                // no insert, no notify, and critically no blink-epoch bump,
                // so a stationary drag's repeated mouse-move ticks don't
                // each spawn a fresh pause/timer task.
                app.set_diff_selection(pane, &key, selection, cx);
                assert_eq!(
                    app.caret_blink_epoch, epoch_after_first_set,
                    "an identical selection must not bump the blink epoch"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    async fn boundary_noop_motion_still_snaps_the_caret_solid(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_dir, window, _visual) = open_multi_block_diff(cx);
        window
            .update(cx, |app, window, cx| {
                let pane = app.workspace.active_pane();
                let key = app.workspace.active_item(pane).unwrap().key().to_string();
                // Row 0 of the multi-block fixture is unchanged text, so
                // (0, 0) is the document start and `move_left` from it is a
                // boundary no-op that `set_diff_selection` skips.
                let point = crate::app::diff_selection::DiffPoint { row: 0, column: 0 };
                app.set_diff_selection(
                    pane,
                    &key,
                    crate::app::diff_selection::DiffSelection::caret_at(
                        point,
                        crate::repo::DiffSide::New,
                    ),
                    cx,
                );

                app.caret_blink_visible = false;
                let epoch_before = app.caret_blink_epoch;
                app.diff_motion(false, window, cx, crate::app::diff_selection::move_left);
                assert!(
                    app.caret_blink_visible,
                    "a boundary no-op keypress must still snap the caret solid"
                );
                assert_ne!(
                    app.caret_blink_epoch, epoch_before,
                    "the no-op motion must re-arm the blink chain"
                );

                // The same holds for a vertical step off the document's top.
                app.caret_blink_visible = false;
                let epoch_before = app.caret_blink_epoch;
                app.diff_vertical_motion(false, false, window, cx);
                assert!(
                    app.caret_blink_visible,
                    "a vertical boundary no-op must also snap the caret solid"
                );
                assert_ne!(app.caret_blink_epoch, epoch_before);
            })
            .unwrap();
    }

    #[gpui::test]
    async fn stop_caret_blink_orphans_the_live_chain(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_dir, window, _visual) = open_multi_block_diff(cx);
        window
            .update(cx, |app, _window, cx| {
                let pane = app.workspace.active_pane();
                let key = app.workspace.active_item(pane).unwrap().key().to_string();
                let point = crate::app::diff_selection::DiffPoint { row: 0, column: 0 };
                let selection = crate::app::diff_selection::DiffSelection::caret_at(
                    point,
                    crate::repo::DiffSide::New,
                );

                // Arm the blink chain via a real caret placement.
                app.set_diff_selection(pane, &key, selection, cx);
                let epoch_before_stop = app.caret_blink_epoch;
                assert_ne!(epoch_before_stop, 0, "the blink chain is armed");

                // `stop_caret_blink` must orphan the in-flight timer by
                // bumping the epoch again, and leave the caret visible.
                app.caret_blink_visible = false;
                app.stop_caret_blink();
                assert_ne!(
                    app.caret_blink_epoch, epoch_before_stop,
                    "stop_caret_blink must bump the epoch to orphan the live chain"
                );
                assert!(
                    app.caret_blink_visible,
                    "stop_caret_blink leaves the caret visible"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    async fn closing_a_changeset_refreshes_the_pending_summary(cx: &mut TestAppContext) {
        let window = add_app_window(cx);
        let (repo_dir, sha) = init_repo_with_one_commit();
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_dir.path().to_path_buf(), window, cx);
                assert!(!app.pending_summary.is_dirty());
                app.selection = Selection::Single { sha };
                app.open_changeset(window, cx);
            })
            .unwrap();

        // Dirty the tree while the changeset is open.
        std::fs::write(repo_dir.path().join("late.txt"), "late\n").unwrap();

        window
            .update(cx, |app, _, cx| {
                app.close_changeset(cx);
                assert!(app.pending_summary.is_dirty());
            })
            .unwrap();
    }

    #[gpui::test]
    async fn window_activation_refreshes_the_pending_summary(cx: &mut TestAppContext) {
        let window = add_app_window(cx);
        let (repo_dir, _sha) = init_repo_with_one_commit();
        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(repo_dir.path().to_path_buf(), window, cx);
                assert!(!app.pending_summary.is_dirty());
            })
            .unwrap();

        // Dirty the tree while the window is inactive (as if edited from
        // another app), then reactivate the window.
        std::fs::write(repo_dir.path().join("late.txt"), "late\n").unwrap();

        window
            .update(cx, |_app, window, _cx| {
                window.activate_window();
            })
            .unwrap();
        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| {
                assert!(app.pending_summary.is_dirty());
            })
            .unwrap();
    }
}
