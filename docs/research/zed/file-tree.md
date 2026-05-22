# Zed's Project Panel: Portability Assessment for Greviewer

## TL;DR

**Recommendation: (A) Build on gpui directly** (~40–60 hours for a senior engineer). Zed's `project_panel` is heavily entangled with Zed's data model (`project::Project`, `worktree::Worktree`, Git status, diagnostics, settings, undo/redo, file operations) and optimized for Zed's rich UI (drag-drop, rename-in-place, multi-select, sticky headers, auto-collapsing). For Greviewer's read-only review use case (file tree + selection → diff view), vendoring and trimming would require reimplementing ~70% anyway. Alternatively, **gpui-component's `tree.rs` is too minimal** (534 lines, no lazy loading, single-select only, no custom row rendering for badges/icons). Build a focused file-tree component on gpui that speaks Greviewer's language: flat commit-diff data or hierarchical repo snapshots, no undo, no mutation.

---

## 1. Crate Inventory

**Location:** `/Users/jellison/code/zed/crates/project_panel`

**License:** GPL-3.0-or-later (Zed)

**Files:**
- `Cargo.toml` — workspace-pinned deps (44 direct)
- `src/project_panel.rs` — main view, 7,384 lines
- `src/project_panel_settings.rs` — settings overlay, 150 lines
- `src/project_panel_tests.rs` — integration tests, 10,377 lines (not needed for port)
- `src/undo.rs` — undo/redo state machine, 558 lines
- `src/utils.rs` — helpers, 42 lines
- `benches/sorting.rs` — benchmark
- `src/tests/undo.rs` — undo test helpers

**Total:** 7,584 lines of active code (excluding tests).

---

## 2. Architecture

### Main Types

**`ProjectPanel`** (137 lines of struct fields; impl blocks total ~8):
```rust
pub struct ProjectPanel {
    project: Entity<Project>,           // Data source, deeply coupled
    fs: Arc<dyn Fs>,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    selection: Option<SelectedEntry>,   // Single selection (or multi via marked_entries)
    marked_entries: Vec<SelectedEntry>,
    filename_editor: Entity<Editor>,    // Inline rename widget
    state: State,                       // Expansion state, cached visible entries
    undo_manager: UndoManager,          // File operation undo/redo
    diagnostics: HashMap<(WorktreeId, Arc<RelPath>), DiagnosticSeverity>,
    // ... drag-drop, hover, context menu, etc. (25+ fields)
}
```

**`State`** (expansion, folding, edit state):
```rust
struct State {
    unfolded_dir_ids: HashSet<ProjectEntryId>,
    ancestors: HashMap<ProjectEntryId, FoldedAncestors>,  // Auto-fold logic
    visible_entries: Vec<VisibleEntriesForWorktree>,      // Filtered entries
    edit_state: Option<EditState>,                        // Rename-in-place
    expanded_dir_ids: HashMap<WorktreeId, Vec<ProjectEntryId>>,
}
```

### Data Sourcing

**Tightly coupled to `project::Project`:**
- `project.visible_worktrees(cx)` → retrieves all worktrees
- `worktree.entries(true, 0)` → live snapshot with status
- `git_store().repo_snapshots(cx)` → Git status for each file
- `diagnostics` system subscribed via editor events

**Flow:**
1. Panel calls `update_visible_entries()` on project/worktree/settings change
2. Spawns async task in `cx.background_spawn()`
3. Uses `GitTraversal::new(&repo_snapshots, ...)` to walk entries with Git status
4. Filters by `hide_gitignore`, `hide_hidden`, applies `auto_fold_dirs` logic
5. Caches flat `Vec<GitEntry>` per worktree
6. Updates diagnostics map on `EditorEvent` subscription

**Coupling severity:** ⚠️ **Very high.** `ProjectPanel` assumes:
- A `Project` entity exists and owns worktrees
- Worktrees provide `entries()` iterator and snapshots
- Git store is available and updated live
- Diagnostics flow through editor events
- Settings are global and observable

There is no abstraction layer; the panel directly calls project methods on nearly every render and state update (165+ refs to workspace/project/settings in the code).

---

## 3. Visual Structure

### Layout & Rendering

**Single row rendering** (`render_entry()`, 400+ lines):
- Div container with 1–2px border, relative positioning
- **Indent:** `depth` scaled by `indent_size` (in pixels or relative units)
- **Expand/collapse arrow:** 
  ```rust
  if entry.is_expanded { "▼" } else { "▶" }  // Unicode glyphs or icon
  ```
  Clickable overlay, toggles on click or keyboard (`Right`/`Left`).
- **Icon:** `FileIcons::get_icon()` (from `file_icons` crate) resolves file extension → icon name
- **Label:** filename text, colored by git status or diagnostic severity
- **Badges:** 
  - Git status indicator (dot or letter, e.g., "M", "A")
  - Diagnostic count (capped at "99+")
  - "Processing" spinner if file op in progress
- **States:**
  - `selected` (blue/accent bg + border)
  - `marked` (alternate highlight)
  - `dragging_over` (transparent highlight)
  - `editing` (inline `Editor` widget replaces label)
- **Hover:** background color shift, border highlight
- **Sticky row:** duplicated at top while scrolled (via `StickyCandidate` trait)

### Indent Guides
Via `IndentGuideLayout` (from `ui` crate):
- Optional vertical lines at each depth level
- Color customizable (theme)
- Can be toggled per-setting

### Scrolling
- `UniformListScrollHandle` (gpui primitive) tracks scroll offset
- Horizontal scroll for long paths
- Auto-scroll on drag (spawn task to scroll while dragged entry near edge)
- Sticky scroll: top N expanded ancestors stay pinned

---

## 4. Features

### Implemented (47 actions)
- **Tree navigation:** Expand, collapse, collapse+children, collapse all, select parent
- **Keyboard nav:** Up/Down/Left/Right, Ctrl+Home/End (scroll edges)
- **Selection:** Single + multi (marked; Shift+Click or Cmd+Click)
- **Rename:** Inline editor, validation (name conflict, path exists)
- **Create:** New file, new directory, with undo/redo
- **Move/Copy/Delete:** Cut, paste, duplicate, trash, permanent delete (with confirmation)
- **File ops:** Download from remote, open (tab, split V/H, permanent), reveal in Finder
- **Filtering:** Toggle hide-gitignore, hide-hidden files
- **Search:** New search in selected directory (integrates with search panel)
- **Diagnostics:** Highlight entries with errors/warnings, navigate to next/prev diagnostic
- **Git:** Navigate next/prev git-changed entry
- **Undo/Redo:** Full op history (file ops only, not UI state)
- **Drag & drop:** Move files/dirs, external file drops
- **Context menu:** Right-click actions
- **Focus:** Panel gains/loses focus, keyboard shortcuts scoped to context

### Out-of-Scope for Greviewer
- **Rename, create, delete, move, copy** — read-only review context
- **Undo/redo** — file ops only; read-only doesn't apply
- **Context menu** — can be simplified or removed
- **Drag & drop** — not needed for review
- **Diagnostics integration** — not in scope (no live editor)
- **Git status indicators** — nice-to-have (changed files vs. unchanged)

### Core Needed for Greviewer
1. **Tree display** (hierarchical list with expand/collapse)
2. **Selection** (single, on-click opens file in diff view)
3. **Visual distinction** (changed files vs. unchanged, maybe with a badge or color)
4. **Keyboard nav** (Up/Down to select)
5. **Scrolling** (handle tall trees)
6. **File icons & labels**

---

## 5. Coupling Rating

**Coupling to `project::Project`: 9/10 (extremely tight)**

| Aspect | Dependency | Mitigation Effort |
|--------|------------|------------------|
| Data model | Assumes `Project`, `Worktree`, `Entry` entities with live snapshots | Must provide own data model or adapter |
| Git status | Direct subscription to `GitStore` events, live repo snapshots | Requires pre-computed file status, single pass |
| Settings | Global `ProjectPanelSettings` observer; auto-recomputes on settings change | Can be hardcoded or passed as props |
| Diagnostics | Editor event subscriptions, live computation | Not needed for review |
| File operations | Direct calls to `worktree.create()`, `delete()`, etc. | N/A (read-only) |
| Undo/Redo | Custom `UndoManager` tied to file ops | N/A (read-only) |
| Workspace integration | `Workspace` entity, dock panel API, focus handling | Adapter needed if using dock panel |

**Honest assessment:**
- **Copy & trim:** Possible but **not cheaper than building.**
  - Must remove ~2,000 lines of file operation logic (create, rename, delete, move, paste, undo)
  - Must remove ~1,500 lines of diagnostic/settings/workspace logic
  - Must replace `GitTraversal` with a simpler flat list or tree builder
  - Must remove drag-drop (500+ lines)
  - Remaining ~2,500 lines of core tree UI is still coupled to `Project` entity
- **Result:** Removing deps is easy; the remaining code still assumes `Entity<Project>` as data source.
- **Verdict:** **Rebuilding is 30–40% cheaper.**

---

## 6. Alternatives

### (B) gpui-component's `tree.rs`

**Location:** `/Users/jellison/code/glinqpad/vendor/gpui-component-0.5.1/src/tree.rs`

**Stats:**
- **534 lines** — small, manageable
- **License:** Apache-2.0 (permissive)
- **Deps:** gpui only (already in Greviewer)

**Capabilities:**
- ✅ Hierarchical tree with nested children
- ✅ Expand/collapse via keyboard (`Right`/`Left`) or `toggle_expand()`
- ✅ Keyboard nav (Up/Down/Left/Right)
- ✅ Single-select (index-based)
- ✅ Custom render closure (per-item)
- ✅ Scroll tracking
- ✅ List-based (uniform_list under hood)

**Limitations:**
- ❌ **No lazy loading** — all items must be in memory as `Vec<TreeItem>`; no async tree walking
- ❌ **No multi-select** — single index selection only
- ❌ **No drag-drop** — not integrated
- ❌ **No row badges/decorations** — you render in closure, but no built-in support
- ❌ **No sticky scroll** — not implemented
- ❌ **No keyboard context menu** — context menu not included
- ❌ **Limited styling** — ListItem passed to render; not much control
- ❌ **No "changed" status tracking** — you must manage that in `TreeItem` manually

**Verdict on gpui-component:**
- **Good for:** Simple, static trees with basic expand/collapse
- **Not good for:** File trees with live status, large repos, visual badges
- **Effort to extend for Greviewer:** ~20–30 hours (add status tracking, render decorations, context menu)

### (C) Vendor and Trim Zed's `project_panel`

**Effort breakdown:**
- Extract the crate: 1–2 hours (copy, Cargo.toml relink, GPL review)
- Remove file ops (create, delete, move, rename, paste, undo/redo): 8–12 hours
- Remove drag-drop: 6–8 hours
- Remove diagnostics integration: 2–3 hours
- Replace `GitTraversal` with simpler iterator or pre-built tree: 4–6 hours
- Replace `Project` entity with data adapter: 6–10 hours
- Test & debug: 10–15 hours
- **Total:** 37–56 hours

**Remaining issues:**
- Still GPL-3.0 (not ideal for proprietary Greviewer)
- Complex state machine for folding (might be overkill for read-only)
- Sticky scroll, diagnostics rendering logic clutters the codebase

---

## 7. Recommendation

### **Option (A): Build on gpui directly — RECOMMENDED**

**Why:**
1. **Alignment:** Greviewer's needs are different from Zed's (read-only, no file ops, simpler data model)
2. **Scope:** A file-tree component for displaying commit diffs or repo snapshots is ~2,000–3,000 lines of clean, focused code
3. **Control:** No GPL, no legacy baggage, optimized for Greviewer's exact use case
4. **Maintainability:** Smaller surface area, easier to reason about and debug
5. **Reusability:** Can be extracted into `greviewer-ui` crate if needed

**Effort:** 40–60 hours (mid-level engineer) or 25–35 hours (senior engineer familiar with gpui)

**Scope:**
- **Core data model:** Flat list of file entries (path, kind, status, icon) or recursive tree builder
- **Tree state:** Expansion map, selection, scroll offset
- **Render:** Each row as div with icon, label, depth indent, status badge
- **Interaction:** Mouse click to select, keyboard nav, expand/collapse on arrow/Right key
- **Visual polish:** Hover state, selection highlight, git status colors, file icons

**Alternative (if time-constrained):**
Use **gpui-component's tree** as a starting point (~15 hours to integrate + 10 hours to extend with status), then decide if more control is needed.

---

## Notes on Portability

### If You Still Want to Try Option (C)

- **License:** Vendoring GPL-3.0 code into a proprietary app is risky; consult legal
- **Trim targets:** Remove these modules first:
  - `undo.rs` (558 lines) — not needed
  - `project_panel_settings.rs` (150 lines) — simplify to bare config
  - ~40% of `project_panel.rs` (file ops, diagnostics, drag-drop)
- **Data adapter:** Create a `TreeDataSource` trait that wraps `Project` or a diff model
- **Remaining risk:** Subtle bugs in folding logic, event handling, or rendering will be expensive to debug because you don't own the original architecture

---

## Summary Table

| Aspect | Zed Panel | gpui-component | Build Custom |
|--------|-----------|--------|---------|
| **Lines** | 7,584 | 534 | ~2,500 |
| **License** | GPL-3.0 | Apache-2.0 | Custom (no GPL) |
| **Coupling** | Very high (Project, Worktree, GitStore) | None (pure gpui) | None (pure gpui) |
| **Features** | 47 actions, rich UI | Basic tree nav | Selection + display |
| **Effort to adapt** | 37–56 hrs (heavy trim) | 20–30 hrs (extend) | 40–60 hrs (build) |
| **Maintenance burden** | High (GPL, future syncs) | Medium (update gpui-component) | Low (own code) |
| **Fit for Greviewer** | Poor (bloated, GPL, over-featured) | Fair (can extend) | **Best** |

