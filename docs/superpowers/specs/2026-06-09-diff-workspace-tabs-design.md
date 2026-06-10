# Diff Workspace Tabs Design

This design replaces the single-diff detail pane in the changeset review screen with a Zed-style workspace: tabbed panes that support preview and pinned tabs, splits, drag and drop, and persisted layout. It covers the full workspace scope in one document; implementation is expected to land in phased slices, each verified against this design.

## Problem

Reviewing a changeset today means clicking one file at a time. The detail pane shows exactly one diff, and selecting another file discards the previous one. Reviewers routinely need to hold several files open at once — a header and its implementation, a test and the code under test — and to compare two diffs side by side. The current `selected_changed_file_path` model cannot express any of that.

## Requirements and Constraints

- Single-clicking a file in the file tree opens its diff in a preview tab; double-clicking opens it pinned. This matches Zed's and VS Code's preview-tab convention.
- Tabs can be closed, reordered, moved between panes, and dragged to create splits.
- The visual design of the tab bar follows Zed's tab bar as closely as our feature set allows.
- Workspace content (open tabs) is scoped to the current changeset and cleared when leaving it. Structural layout (split arrangement) persists per repository.
- **License (ADR-0001):** Zed's `workspace`, `editor`, and `ui` crates are GPL-3.0. We model behavior and appearance only. No code in this feature may be copied from or derived from Zed source; Zed is read for idioms and observed for UX, nothing more.
- **Layout (ADR-0002):** the feature lives in a by-feature module `src/workspace/` inside the single binary crate.
- **Testing (ADR-0003):** the module ships unit tests for all state transitions and `#[gpui::test]` view tests for the rendered tab bar, panes, and drag interactions.

## Alternatives Considered

Three architectures were weighed. Keeping tab state as flat fields on `App` (a `Vec<DiffTab>` plus active index) is the smallest change but cannot represent splits, so every later phase would rework it. A separate gpui child-view entity for the tab bar adds event plumbing without isolation benefits, and would be the first child entity in a codebase that deliberately renders everything from the root `App` view. The chosen design — a workspace/pane abstraction held as plain state on `App` and rendered by module-level helpers — costs more up front but makes splits, drag and drop, and persistence additive rather than rework, and keeps all logic unit-testable without gpui.

## Placement and Scope

The workspace occupies the changeset review screen's detail area, to the right of the file tree. The file tree remains a sidebar outside the workspace. The graph screen is unchanged, and the title bar's changeset pills continue to switch review contexts above the workspace level.

Entering a changeset starts with the persisted pane layout and no open tabs. Leaving the changeset (returning to the graph or opening a different changeset) closes all tabs. `App.selected_changed_file_path` is removed; the workspace is the sole source of truth for what the detail area shows.

## Tab Semantics

Single-clicking a file in the tree opens its diff in the **preview tab** of the active pane. Each pane has at most one preview tab; a subsequent single-click replaces the preview tab's content in place (same tab-strip position, scroll reset). The preview tab's title renders in italic.

Double-clicking a file in the tree opens it **pinned**. If the file currently occupies the preview tab, that tab is promoted in place rather than reopened. Double-clicking a preview tab in the tab bar also promotes it. Pinned tabs are never auto-replaced; they close only by explicit user action.

Opening a file that is already open in the active pane — preview or pinned — activates the existing tab instead of duplicating it. A file may be open in multiple panes simultaneously, but at most once per pane.

New tabs append to the end of the pane's tab strip. Tab order is stable except under explicit drag-reorder. Closing the active tab activates its right neighbor, or the left neighbor when no right neighbor exists. Tabs close via a hover-revealed close button (always visible on the active tab) or middle-click.

A tab's title is the file name, colored by change kind using the same palette as the file-tree rows. When two open tabs in the same pane share a file name, each appends a muted parent-directory hint for disambiguation.

The file tree's selection highlight is click-driven only: it reflects the last tree click and does not follow tab activation. When every tab in every pane is closed, panes render the existing "select a file" empty state.

## Panes and Splits

The workspace layout is a tree whose internal nodes are axes (horizontal or vertical) holding ratio-weighted children, and whose leaves are panes. A single pane is the degenerate tree. Dividers between siblings are draggable, reusing the existing resizable-panel machinery where practical.

Exactly one pane is active. Tree clicks open files in the active pane, and its tabs render at full visual strength while inactive panes render dimmed, following Zed. Clicking anywhere within a pane — tab bar or content — activates it.

Each pane's tab bar carries split-right and split-down controls at its right corner. Splitting inserts a new empty pane (showing the placeholder) adjacent to the source pane and makes it active. Closing a pane's last tab does not close the pane; the pane shows the placeholder. A pane closes only via the explicit close-pane action or by having its last tab dragged out, at which point the layout tree collapses the empty slot and returns its space to siblings.

Keyboard bindings, registered through the existing menu module: `Cmd+W` closes the active tab, `Ctrl+Tab` and `Ctrl+Shift+Tab` cycle tabs within the active pane, `Cmd+K` followed by an arrow key splits in that direction, and `Cmd+K W` closes the active pane.

## Drag and Drop

Dragging a tab horizontally within its own tab bar reorders it, with a drop indicator marking the insertion point. Reordering does not change preview status.

Dropping a tab onto another pane's tab bar moves it there at the drop position. A moved preview tab becomes pinned — the preview slot is per-pane and the move is a deliberate act. If the target pane already holds the same file, the drag merges: the existing tab activates and the dragged tab closes rather than duplicating.

Dragging a tab over the left, right, top, or bottom edge zone of a pane's content area highlights the corresponding half of the pane; dropping there creates a split in that direction with the dragged tab as the new pane's only (pinned) tab. Dragging the last tab out of a pane closes that pane and collapses the layout as described above.

## Layout Persistence

The settings store persists, per repository, the pane-tree shape — split axes, ratios, and which pane was active. Tabs, items, and preview state are never persisted; they are per-changeset by definition. Entering any changeset review in a repository restores that repository's pane arrangement with all panes empty. If a saved layout fails to deserialize, the workspace falls back silently to a single pane and overwrites the stored layout on next save.

## Visual Design

The tab bar is modeled on Zed's: a compact strip (about 32px) above each pane's content, tabs separated by hairline borders, the active tab using the editor background with a top accent border, inactive tabs using the tab-bar background with muted text. The close button occupies the tab's right edge on hover. Preview titles are italic; titles are colored by change kind. The strip scrolls horizontally when overfull — tabs never shrink to fit — responds to the mouse wheel, and auto-scrolls the active tab into view. The right corner holds only the split controls; Zed's navigation-history arrows and other corner controls are out of scope.

## Architecture

A new `src/workspace/` module owns all state types and their render helpers. The root `App` view holds a `Workspace` value and composes the module's render functions, the same pattern `graph/` and `app/title_bar.rs` use today. No child view entities are introduced.

- `Workspace` owns the pane tree, the active pane id, and layout (de)serialization. Its public API expresses every user-level operation: `open_preview`, `open_pinned`, `activate_tab`, `promote_preview`, `close_tab`, `split`, `close_pane`, `activate_pane`, `move_tab`, and `resize`. All are pure state transitions.
- `PaneGroup` is the layout tree: axis nodes with ratio-weighted children, pane leaves.
- `Pane` holds an ordered tab list, the active tab index, and an optional preview-tab index.
- `WorkspaceItem` is the trait for anything openable in a tab: an identity key (driving activate-instead-of-duplicate and drag-merge), a title with change-kind styling, and content rendering. The sole initial implementor is `FileDiffItem`, holding a file path plus changeset context and delegating content rendering to the existing diff code paths. Future item kinds (graph, images) slot in without touching pane logic.

## Testing

Unit tests cover every `Workspace` transition without gpui: preview replacement and promotion, dedupe activation, close-neighbor selection, split insertion and collapse, move-with-merge, drag-reorder ordering, serialize/deserialize round-trips, and corrupt-layout fallback.

View tests under `#[gpui::test]` attach debug selectors to tabs, close buttons, split controls, dividers, and drop zones, then drive the UX contract with simulated input: single-clicking two files yields one italic preview tab showing the second file; double-click pins; middle-click closes; mouse-down/move/up sequences exercise reorder, cross-pane move, and drag-to-split. Smoke coverage extends the golden path to open two files in tabs and split once.

`docs/specs/review/workflow.md` (or a new `docs/specs/review/workspace.md` referenced from it) gains the normative tab and workspace behavior in the same change that implements it.

## Phasing

Implementation lands in four slices, each independently shippable and `bin/check`-clean: (1) workspace core with a single tabbed pane and the full preview/pinned UX; (2) splits, pane focus, and keyboard bindings; (3) drag and drop; (4) layout persistence. The architecture above is fixed across all slices; later slices add operations, not rework.

## Definition of Done

The feature is complete when all four slices have landed, the review workflow spec documents the workspace contract, every behavior in this design is covered by a unit or view test, and `bin/check` passes.
