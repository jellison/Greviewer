# Zed's Git and Diff Architecture: A Portability Assessment for Greviewer

## TL;DR Ranking

1. **`git_graph` (TIGHT COUPLING, GPL-3.0)** — Renders a polished commit graph with branch lanes and merge connectors using gpui primitives. However, it depends heavily on Zed's workspace/project/editor stack (`editor`, `workspace`, `project`, `settings`, `ui` crates). Porting would require significant refactoring; building from scratch on gpui may be faster for Greviewer.

2. **`buffer_diff` (LOOSE COUPLING, GPL-3.0)** — Excellent reusable layer for computing diff hunks between two text buffers. Minimal Zed dependencies (only `text`, `language`, `gpui`). This is the most portable piece and could be adapted for diff computation, though Greviewer would need its own rendering layer.

3. **`git` (TIGHT COUPLING, GPL-3.0)** — Core abstraction over libgit2 with excellent domain objects (`CommitData`, `CommitDiff`, `Branch`). Highly useful APIs but deeply integrated with Zed's async/task system, gpui, and workspace concepts. Reusable for low-level git operations if decoupled from workspace/project.

**Verdict**: GPL-3.0 license cost is real. Build commit graph rendering from scratch on gpui + use `buffer_diff` logic as inspiration for hunk computation. Consider git2-rs directly for git operations to avoid licensing entanglement.

---

## 1. Inventory of Git-Related Crates

### 1.1 `crates/git`
- **License**: GPL-3.0-or-later
- **Size**: 8 files, ~3,500 lines (estimated from module count)
- **Purpose**: Low-level abstraction over libgit2, exposing commits, branches, status, blame, and repository operations.
- **Role in Zed**: Core git integration layer; used by git_ui, git_hosting_providers, git_graph, and project's git_store.
- **Direct Zed dependencies**: 
  - Workspace crates: `collections`, `text`, `util`, `rope`, `sum_tree`, `smallvec`
  - UI: `gpui`, `ui` (minimal)
  - Async: `async-channel`, `smol`, `async-trait`, `futures`
  - External: `git2` (libgit2 wrapper), `regex`, `url`, `serde`
- **Coupling rating**: **TIGHT**. Depends on Zed's text/rope/async primitives; tight to gpui via `Task`. Hard to lift without rewriting async and memory-model adapters.

---

### 1.2 `crates/git_graph` ⭐ CRITICAL FOR GREVIEWER
- **License**: GPL-3.0-or-later
- **Size**: 1 file (~2,400 lines in single file), highly dense
- **Purpose**: Renders an interactive commit graph UI with branch lanes, merge connectors, and per-commit detail panel.
- **Role in Zed**: Primary UI for browsing repository history; integrates with workspace tabs, search, and commit detail view.
- **Direct Zed dependencies**:
  - **Essential graph logic**: `git` (commit data), `project` (GitStore, Repository entity)
  - **UI rendering**: `gpui` (primitives: PathBuilder, window.paint_path, shapes), `ui` (Table, ListItem, buttons, styling)
  - **Editor integration**: `editor`, `workspace`, `settings`, `theme`, `menu`, `picker`, `search`, `task`
  - **Workspace structure**: `project_panel`, `db`, `language`, `language_model`, `release_channel`
- **Key public API**:
  - `GitGraph` — main UI component (implements `Render`, `Item`, `SearchableItemHandle`)
  - Renders a `Table` with columns: graph canvas (commit lanes), description, date, author, commit hash
  - Graph data structure: internal `GraphData` with `commits`, `lines` (lane connectors), `max_lanes`
  - Subscribes to `GitStoreEvent` for live repository updates
  - Wraps `CommitView` for detailed diff display
- **Does it render a graph?** **YES**, fully. Uses gpui's `PathBuilder` and `window.paint_path()` to draw:
  - Commit circles at lane intersections (constants: `COMMIT_CIRCLE_RADIUS`, `LANE_WIDTH`)
  - Curved lines for branch merges (`CommitLineSegment` with `Curve { to_column, on_row, curve_kind }`)
  - Straight vertical lines for linear history
  - Row-based layout synchronized with uniform list scrolling
- **Coupling rating**: **VERY TIGHT**. Depends on:
  - `project::GitStore` and `Repository` entities (cannot be instantiated standalone)
  - Zed's workspace/item/tab system for lifecycle
  - Editor and settings for view configuration
  - Heavy use of Zed-specific async/event patterns via gpui

**Verdict for Greviewer**: Porting would require:
1. Extracting graph layout logic (lanes, line segments) into standalone `graph_layout` module
2. Reimplementing rendering against gpui (doable — graph uses basic primitives)
3. Replacing `GitStore` subscription with custom git-data source
4. Rebuilding table/column management (non-trivial but doable)

**Estimated effort**: 3-4 weeks of refactoring. **Building from scratch** on gpui + a layout library like `gitoxide` might be 2-3 weeks.

---

### 1.3 `crates/git_ui`
- **License**: GPL-3.0-or-later
- **Size**: 24 files, ~8,000 lines
- **Purpose**: UI components for git workflows: blame, commit modal, file diff view, branch picker, stash, clone, etc.
- **Role in Zed**: Implements all git UI except the graph; handles user-facing git operations.
- **Direct Zed dependencies**:
  - **Data**: `git`, `buffer_diff`
  - **UI/rendering**: `gpui`, `ui`, `editor`, `multi_buffer`, `component`
  - **Workflow**: `workspace`, `project`, `picker`, `notifications`, `language_model`, `prompt_store`
  - **Presentation**: `markdown`, `linkify`, `file_icons`, `theme`, `settings`
- **Key exports**:
  - `CommitView` — diff viewer for a single commit (opens files and renders per-file diffs)
  - `FileDiffView` — side-by-side diff between two buffers (uses `BufferDiff` and `SplittableEditor`)
  - `TextDiffView` — diff clipboard content vs. selected text
  - `GitPanel` — main git status panel with staging controls
  - `BlameUI` — inline blame annotations
- **Diff rendering path**:
  - `FileDiffView::open()` creates two buffers (old/new), builds `BufferDiff`, wraps in `SplittableEditor`
  - `SplittableEditor` renders side-by-side via editor's built-in diff mode
  - Diff hunks from `BufferDiff` are rendered as colored line backgrounds and gutter decorations
- **Coupling rating**: **VERY TIGHT**. Depends on editor multi-buffer, workspace lifecycle, UI framework.

---

### 1.4 `crates/buffer_diff` ⭐ MOST PORTABLE
- **License**: GPL-3.0-or-later
- **Size**: 1 file, ~800 lines
- **Purpose**: Compute and track diff hunks between two text buffers; track staging state transitions.
- **Role in Zed**: Powers all diff views (file diff, text diff, inline hunks in editor).
- **Direct Zed dependencies**:
  - **Text model**: `text` (Buffer, Anchor, Point, Edit), `rope` (Rope)
  - **Diff algorithm**: `language::word_diff_ranges` (language crate's integration with git2)
  - **UI integration**: `gpui` (Task, App)
  - **Async**: `futures`
  - **Minimalist**: no workspace, project, or editor dependencies
  - **External**: `git2` for libgit2 diff computation
- **Key public API**:
  - `BufferDiff::new(base_text)` — construct from base (HEAD) text
  - `BufferDiff::update_diff(new_buffer_snapshot)` — recompute hunks
  - `BufferDiff::hunks()` — fetch computed hunks as `DiffHunk` (buffer range + base range)
  - `DiffHunk` — `buffer_range: Range<Anchor>`, `diff_base_byte_range: Range<usize>`, `buffer_word_diffs`, `base_word_diffs`
  - Tracks secondary diff (working copy vs. index, for staged/unstaged distinction)
- **Diff algorithm**: Uses `language::word_diff_ranges` (character-level refinement on top of git2's line diff)
- **Coupling rating**: **LOOSE**. Could be extracted with minimal refactoring. Main coupling points:
  - Uses Zed's `text::Buffer` model; would need adapter for external text model
  - Uses `gpui::Task` for async; could be replaced with standard `Future`
  - `language` dependency only for word diff; could stub or replace

**Verdict for Greviewer**: **EXCELLENT CANDIDATE** for porting or adaptation. Logic is sound and isolated; refactoring effort ~1 week.

---

### 1.5 `crates/streaming_diff`
- **License**: GPL-3.0-or-later
- **Size**: 1 file, ~300 lines
- **Purpose**: Character-level diff algorithm for highlighting word-level changes within lines.
- **Role in Zed**: Called by `buffer_diff` for fine-grained diff display.
- **Direct Zed dependencies**:
  - Minimal: `rope`, `ordered_float`
  - No gpui, no workspace, no editor
- **Key public API**:
  - `StreamingDiff` — computes optimal character insertions/deletions between two strings
  - Returns `Vec<CharOperation>` (Insert/Delete/Keep)
  - Uses matrix-based algorithm (dynamic programming, similar to Myers' diff)
- **Coupling rating**: **VERY LOOSE**. Self-contained diff algorithm; could be extracted as-is with only rope dependency.

**Verdict for Greviewer**: **TRIVIAL TO REUSE**. This is pure algorithm; could be copied into Greviewer with no adaptation.

---

### 1.6 `crates/git_hosting_providers`
- **License**: GPL-3.0-or-later
- **Size**: 12 files, ~2,000 lines
- **Purpose**: Abstraction for GitHub, GitLab, Gitea, Azure DevOps, Bitbucket, etc.; maps commits to permalinks and detects PRs in commit messages.
- **Role in Zed**: Used by git_graph to show clickable links to remote commits/PRs.
- **Direct Zed dependencies**:
  - `git` (for URL parsing, remote info)
  - Minimal UI: `gpui` (SharedString, Task)
  - No workspace, editor, or settings
  - External: `regex`, `serde`, `url`
- **Key exports**:
  - `GitHostingProviderRegistry` — registry of hosting provider implementations
  - `extract_pull_request()`, `build_commit_permalink()` — per-provider implementations
- **Coupling rating**: **LOOSE**. Could be extracted as a standalone crate with minor edits (gpui → standard types).

---

## 2. Commit Graph Deep-Dive

### Architecture: Rendering vs. Layout

**git_graph DOES RENDER**. The crate contains both layout computation and full gpui rendering:

#### Layout Computation (`git_graph.rs` lines ~440–600)
- **Data structure**: `LaneState` enum tracks active branch lanes
  - Each lane: child OID, parent OID, color index, row range, destination column
  - Segments: straight lines and curves (Bézier-like with `CurveKind::Merge`, `CurveKind::Checkout`)
- **Algorithm**: Single-pass traversal of commits in topological order
  - Maintains `Vec<LaneState>` for active lanes
  - When processing a commit: find/create lanes for parents, update destination columns
  - Curves computed on-the-fly when lanes merge or diverge
- **Output**: `CommitLine` struct per commit
  ```rust
  struct CommitLine {
      child_column: usize,
      full_interval: Range<usize>,  // row range
      color_idx: usize,
      segments: SmallVec<[CommitLineSegment; 1]>,
  }
  enum CommitLineSegment {
      Straight { to_row: usize },
      Curve { to_column: usize, on_row: usize, curve_kind: CurveKind },
  }
  ```

#### Rendering (`git_graph.rs` lines ~1,055–1,083)
- **Primitives**: gpui's `PathBuilder` and `window.paint_path()`
  - Draw circles: `PathBuilder::fill()` with `arc_to()` for circle geometry
  - Draw lines: implicit via path rendering (strokes computed by gpui layer)
- **Visual constants**:
  ```rust
  COMMIT_CIRCLE_RADIUS: 3.5px
  LANE_WIDTH: 16px
  LINE_WIDTH: 1.5px
  LEFT_PADDING: 12px
  ```
- **Coordinate system**: 
  - X: `lane_center_x = left_padding + lane_idx * lane_width + lane_width / 2`
  - Y: `to_row_center = bounds.y + row_idx * row_height + row_height / 2 - scroll_offset`

#### Visual Output
Produces a **gitk-like** graph:
- Colored dots at commit positions (one per lane)
- Vertical lines connecting commits on same lane
- Curved connectors when lanes merge/branch
- Row-aligned with table rows showing commit message, author, date
- Horizontally scrollable (graph expands with merge complexity)

### Public API & Consumer Contract

```rust
impl GitGraph {
    pub fn new(repo_id, git_store, workspace, log_source, window, cx) -> Self;
    pub fn select_commit_by_sha(sha, cx);
    pub fn invalidate_state(cx);  // refresh on repo changes
}

// GraphData (internal but illustrative)
struct GraphData {
    commits: Vec<GraphCommit>,
    lines: Vec<CommitLine>,
    max_lanes: usize,
}

// Consumed from GitStore
struct GraphDataResponse {
    commits: Vec<InitialGraphCommitData>,
    update_count: u64,
}
```

### Dependencies on Zed's Ecosystem

| Dependency | Why | Extractability |
|------------|-----|-----------------|
| `project::GitStore` | Source of truth for commit data; emits `GitStoreEvent::RepositoryUpdated` | Must replace with custom git source |
| `editor::Editor` | Search box implementation | Reimplementable with gpui primitives |
| `workspace::Item` | Tab lifecycle and item management | Reimplementable as simple UI state |
| `settings::Settings` | Theme colors and UI font size | Replaceable with config struct |
| `ui::Table` | Column layout and resizing | Could use simpler list/flex layout |
| `gpui::*` | Rendering and event loop | Core; non-negotiable |

### Verdict: Build vs. Port

**Porting `git_graph` to Greviewer**:
- **Pros**: Lane layout logic is well-factored; rendering uses gpui primitives (compatible)
- **Cons**: Massive dependency on Zed's workspace/project/entity system; refactoring would require:
  - Extract graph layout into standalone module (doable, ~1 week)
  - Replace `GitStore` subscription with custom event source (easy)
  - Reimplement table layout with simpler flex/list (1–2 days)
  - Adapt async/event patterns to Greviewer's architecture (1 week)
  - Total: ~3 weeks

**Building from scratch**:
- Use `gitoxide` (gix) or git2 for commit graph traversal
- Implement lane layout algorithm (2–3 days, ~200 lines)
- Render with gpui primitives (~1 week)
- Total: ~2 weeks

**Recommendation**: **BUILD FROM SCRATCH**. The extra time is offset by avoiding GPL licensing and tight coupling.

---

## 3. Diff Display

### Data Path: From Commit to Rendered Diff

```
Git commit data (CommitDiff with CommitFile array)
    ↓
CommitView::open(commit_sha, ...)
    ↓
For each file: CommitFile { old_text, new_text, path, is_binary }
    ↓
BufferDiff::new(old_text) + BufferDiff::update_diff(new_text_snapshot)
    ↓
Hunks: Vec<DiffHunk> { buffer_range, diff_base_byte_range, buffer_word_diffs, base_word_diffs }
    ↓
SplittableEditor with diff mode enabled
    ↓
Rendered: colored line backgrounds (added/removed/modified), gutter decorations, word highlights
```

### Diff Algorithm

1. **Line-level**: git2's `Patch::diff_stats()` or internal libgit2 line-diff algorithm
   - Identifies hunks (contiguous ranges of added/removed/modified lines)
2. **Word-level** (refinement): `language::word_diff_ranges(old_text, new_text)`
   - Calls `streaming_diff::StreamingDiff` for character-level LCS
   - Returns ranges of inserted/deleted runs within lines

### Side-by-Side vs. Inline

**Zed renders SIDE-BY-SIDE** via `SplittableEditor`:
- Left pane: base (old) buffer with diff hunks as gutter decorations
- Right pane: current (new) buffer with hunks highlighted
- Synchronized scrolling
- Word diffs highlighted inline (different color from line hunk color)

### Reusable Components

| Component | Portability | Recommendation |
|-----------|-------------|-----------------|
| `buffer_diff` hunk computation | HIGH | Extract and adapt |
| `streaming_diff` word diff | VERY HIGH | Copy as-is |
| `SplittableEditor` rendering | VERY LOW | Rebuild for Greviewer |
| Hunks UI (gutter, line coloring) | MEDIUM | Greviewer's editor will define this |

---

## 4. Git Operations Layer (`crates/git`)

### Level of Abstraction

Zed's `git` crate wraps **libgit2 (git2-rs)** and shells out to **git binary** for advanced operations:
- **Libgit2**: low-level repo access (objects, status, blame, diffs)
- **Git binary**: high-level workflows (fetch, push, rebase, worktrees, hooks)
- **Hybrid**: CommitDataReader async reads via `git cat-file` for performance

### Primitives Exposed

#### Commits
```rust
pub struct CommitData {
    pub sha: Oid,
    pub parents: SmallVec<[Oid; 1]>,  // most have 1; merges have 2+
    pub author_name, author_email: SharedString,
    pub commit_timestamp: i64,
    pub subject, message: SharedString,
}

pub struct CommitDetails {
    pub sha: String,
    pub summary: CommitSummary,
    pub diff: CommitDiff,
}

pub struct CommitDiff {
    pub files: Vec<CommitFile>,
    pub stats: DiffStats,
}

pub struct CommitFile {
    pub path: RepoPath,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub is_binary: bool,
}
```

#### Branches & Refs
```rust
pub struct Branch {
    pub is_head: bool,
    pub ref_name: SharedString,  // "refs/heads/main"
    pub upstream: Option<Upstream>,
    pub most_recent_commit: Option<CommitSummary>,
}
```

#### Graph Traversal
```rust
pub async fn log_commits(
    log_source: LogSource,  // All, Branch(name), Path(path)
    log_order: LogOrder,    // DateOrder, TopoOrder, AuthorDateOrder
    chunk_size: usize,
) -> Result<Vec<InitialGraphCommitData>>

pub struct InitialGraphCommitData {
    pub sha: Oid,
    pub parents: SmallVec<[Oid; 1]>,
    pub ref_names: Vec<SharedString>,  // tags, branches pointing here
}
```

#### File Tree at Commit
```rust
pub struct CommitDetails {
    pub files: Vec<CommitFile>,  // all files modified in this commit
}
// For tree at a commit, can use:
pub async fn get_file_content(sha: Oid, path: RepoPath) -> Result<String>
```

### Greviewer's Requirements Check

| Requirement | Zed Exposes | Notes |
|-------------|-------------|-------|
| List commits with parents | ✅ YES | `InitialGraphCommitData` |
| Topological graph traversal | ✅ YES | `LogOrder::TopoOrder` + `log_commits` |
| Diff between two commits | ✅ YES | `CommitDetails::diff` |
| List files in tree at commit | ✅ YES | `CommitFile` array in `CommitDiff` |
| Get file content at commit | ✅ YES | `CommitFile::old_text` / `new_text` |
| Blame information | ✅ YES | Separate `Blame` module |

**All requirements met.**

### Coupling & Extractability

- **Strong coupling to**:
  - `gpui::Task`, `AsyncApp` (async runtime)
  - Zed's `util`, `text`, `collections` (memory models)
  - Zed's `settings` (for git config overrides)
  - Zed's workspace/project for path resolution
- **Easy to extract**:
  - Core libgit2 wrapping (GitRepository trait)
  - CommitData/CommitDiff types
  - Git binary invocation (uses standard smol/futures)

**Recommendation for Greviewer**: Use **git2-rs** directly instead of porting Zed's wrapper. Zed's abstractions add little value for a code-review tool.

---

## 5. Recommendations: Build vs. Port

### (A) Commit Graph

| Option | Cost | Pros | Cons |
|--------|------|------|------|
| **Port git_graph** | 3 weeks refactor + GPL-3.0 risk | Battle-tested logic | Deep Zed coupling; licensing complexity |
| **Build from scratch** on gpui + gix | 2 weeks implementation | Clean architecture; own code; license-free | Must validate lane algorithm |
| **Hybrid: extract layout, rewrite rendering** | 2.5 weeks | Balance | Still GPL-3.0 for layout |

**Verdict**: **BUILD FROM SCRATCH** using gitoxide for graph data and gpui for rendering.

### (B) Diff Computation

| Option | Cost | Pros | Cons |
|--------|------|------|------|
| **Port buffer_diff** | 1 week refactor + GPL-3.0 | Proven algorithm | Minor Zed dependencies |
| **Use similar-rs crate** | 3 days + integration | Standard Rust lib; permissive license | Less feature-complete (no word diff) |
| **Write custom** | 1.5 weeks | Total control | Reinvent the wheel |

**Verdict**: **PORT `buffer_diff` or use `similar-rs`**. Either is low-cost. If GPL-3.0 is a blocker, use `similar-rs`.

### (C) Diff Rendering

| Option | Cost | Pros | Cons |
|--------|------|------|------|
| **Extract SplittableEditor logic** | 3+ weeks | Exact Zed UX | Massive Zed coupling |
| **Build custom side-by-side with gpui** | 1.5 weeks | Clean; Greviewer-native | Must implement column sync, resizing |

**Verdict**: **BUILD CUSTOM**. Zed's editor is too intertwined.

### (D) Git Operations

| Option | Cost | Pros | Cons |
|--------|------|------|------|
| **Port crates/git** | 2 weeks + GPL-3.0 + must unbind from workspace | Complete abstraction | Heavy refactor |
| **Use git2-rs directly** | 1 week (learning) | Simple; permissive (MIT); standard | Lower-level; handle more edge cases |
| **Use gitoxide (gix)** | 1.5 weeks (learning) | Modern; pure Rust; MIT | Steeper learning curve; less mature |

**Verdict**: **USE GIT2-RS DIRECTLY**. Simpler abstraction, proven, permissive license.

---

## 6. Hidden Gems & Related Crates

### 6.1 `crates/project` → `git_store.rs`
- **Relevance**: Central coordinator of all git state in Zed; defines `Repository` entity and `GitStore` event stream.
- **For Greviewer**: Architectural inspiration. Don't port; design your own `GitState` coordinator.

### 6.2 `crates/editor` → Multi-buffer diff rendering
- **Relevance**: Editor renders diff hunks as background colors and inline decorations.
- **For Greviewer**: Can adapt diff hunk visualization patterns, but don't extract; gpui rendering is simpler.

### 6.3 `crates/ui` → Table, ContextMenu, Picker
- **Relevance**: High-quality gpui UI components used by git_graph.
- **For Greviewer**: Can reuse these crates if they're published; check Zed's workspace structure. If not published, inspiration for gpui layouts.

### 6.4 `crates/language` → `word_diff_ranges()`
- **Relevance**: Character-level diff refinement.
- **For Greviewer**: Dependency for `buffer_diff`. Could use `streaming_diff` standalone or integrate into custom diff logic.

### 6.5 `crates/theme` & `crates/settings`
- **Relevance**: Color schemes and UI configuration.
- **For Greviewer**: Not directly reusable; design Greviewer's own theme system.

---

## Summary Table: Portability & License

| Crate | License | Lines | Coupling | Portability | Recommendation |
|-------|---------|-------|----------|-------------|-----------------|
| `git` | GPL-3.0 | ~3.5K | TIGHT | Medium | Use git2-rs instead |
| `git_graph` | GPL-3.0 | ~2.4K | VERY TIGHT | Low | Build from scratch |
| `git_ui` | GPL-3.0 | ~8K | VERY TIGHT | Low | Analyze; rebuild UI |
| `buffer_diff` | GPL-3.0 | ~800 | LOOSE | High | PORT or use `similar-rs` |
| `streaming_diff` | GPL-3.0 | ~300 | VERY LOOSE | Very High | COPY as-is or reference |
| `git_hosting_providers` | GPL-3.0 | ~2K | LOOSE | High | Extract if needed; MIT rewrite easier |

---

## Conclusion

**For Greviewer's MVP commit-graph and diff-display surfaces**, the cost of porting from Zed is higher than building fresh:

1. **Commit Graph**: Build on gpui + gitoxide. Effort: 2 weeks. Benefit: Clean architecture, no GPL entanglement.
2. **Diff Computation**: Adapt `buffer_diff` logic or use `similar-rs`. Effort: 1 week. Benefit: Proven algorithm, low risk.
3. **Diff Rendering**: Custom gpui-based side-by-side. Effort: 1.5 weeks. Benefit: Greviewer-native; simpler than adapting SplittableEditor.
4. **Git Operations**: Use git2-rs with thin Greviewer wrapper. Effort: 1 week. Benefit: MIT license, focused scope.

**Total estimated effort: 5-6 weeks to MVP, with zero GPL licensing risk and a codebase designed specifically for Greviewer's needs.**

GPL-3.0 remains a high barrier for commercial reuse, even if Greviewer itself is open-source. Avoid it where feasible.
