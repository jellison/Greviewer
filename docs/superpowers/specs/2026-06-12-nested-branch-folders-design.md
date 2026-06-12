# Design: Nested Branch Folders in the Graph Sidebar

## Context and Problem

The branch sidebar lists every unmerged local branch as a flat list
(`2026-06-09-graph-branch-sidebar-design.md`), with per-branch visibility
toggles (`2026-06-10-branch-visibility-toggle-design.md`). Flat listing
ignores the slash-separated naming convention most teams use: a repository
with `features/login`, `features/search`, and `bugfix/crash-on-open` shows
three unrelated-looking rows, and a team with many `features/*` branches gets
a long undifferentiated list. The user cannot collapse a group they do not
care about, and hiding a whole group from the graph means clicking each
branch's eye toggle one at a time.

This design nests branches under collapsible folders derived from their
slash-separated name segments, and gives each folder its own visibility
toggle that hides or shows every branch beneath it.

## Requirements and Non-Goals

1. Any branch whose name contains `/` nests under folders, one folder per
   path segment except the last. Nesting is multi-level:
   `team/alice/feature-x` renders as `team` → `alice` → `feature-x`.
2. A folder exists even when it contains a single branch. Grouping depends
   only on the branch's own name, never on what siblings exist, so rows do
   not reorganize as branches appear and disappear.
3. Folders collapse and expand by clicking the folder row. All folders start
   expanded. Collapse state is session-only and resets when a repository is
   opened, like hidden-branch state.
4. Collapsing a folder is purely visual: it removes descendant rows from the
   sidebar but does not change which branches are hidden from the graph.
5. Each folder row carries the same hover-reveal eye toggle as branch rows.
   Clicking it hides every hideable descendant branch if any is visible,
   otherwise shows them all. The HEAD branch cannot be hidden, so a folder
   toggle skips it.
6. A folder renders as hidden (eye-off always shown, name muted) when every
   non-HEAD descendant branch is hidden, and as mixed (eye-off always shown,
   name in the normal color) when only some are. Individual branch toggles
   continue to work inside folders.
7. All existing state — `hidden_branches`, focus, graph filtering, debug
   selectors — continues to key on the full branch name. Branch rows display
   only the final segment.

Non-goals: persisting collapse state across restarts, collapse-all /
expand-all bulk actions, drag-to-rename or any branch mutation, and remote
branches or tags (the sidebar does not list them).

## Alternatives Considered

**Extract a shared tree component for the file tree and the branch sidebar.**
The file tree already solves collapsible nesting, so a generic component is
tempting. Rejected for now: the two trees differ in row content (status
icons vs. visibility toggles), in default-collapse policy (the file tree
stores deltas against computed defaults; branch folders are uniformly
expanded), and in interaction (file rows open diffs; branch rows focus
commits). Forcing one abstraction before a second real consumer exists would
churn working file-tree code for no behavioral gain. Once both trees have
matured we will understand the genuine overlap; the duplication is accepted
until then.

**Flat list with group headers.** Render non-interactive `features/` headers
above their branches without a real tree. Cheapest, but it cannot express
multi-level nesting, which requirement 1 demands.

**Chosen: mirror the file-tree pattern inside the branch sidebar.** Build a
small tree from branch-name segments, flatten it depth-first into rows
tagged with their indent depth, and store collapsed folder paths in a
session-only set on `App`. This is the same shape as `FileTreeRow` plus
`collapsed_file_tree_paths`, so the implementation follows a proven local
idiom without touching the file tree itself.

## Design

### Row model and tree construction (`src/app.rs`)

A new render model alongside `FileTreeRow`:

```rust
enum BranchTreeRow {
    Folder {
        name: String,        // final segment, e.g. "alice"
        path: String,        // full prefix, e.g. "team/alice"
        depth: usize,
        collapsed: bool,
        visibility: FolderVisibility, // Visible | Hidden | Mixed
    },
    Branch {
        branch: LocalBranch,
        display_name: String, // final segment
        depth: usize,
    },
}
```

A pure function builds the rows:

```rust
fn build_branch_tree_rows(
    local_branches: &[LocalBranch],
    collapsed_folders: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
) -> Vec<BranchTreeRow>
```

Each branch name splits on `/`; every segment but the last contributes a
folder node keyed by its full prefix path. Nodes live in a `BTreeMap`-backed
tree so siblings sort alphabetically, with folders listed before branches at
each level, matching the file tree. Flattening walks depth-first; a folder
whose path is in `collapsed_folders` emits its own row and skips all
descendants. A branch name with empty segments (leading, trailing, or
doubled slashes) is not a concern: Git rejects such ref names, so the
builder may treat segments as non-empty.

Folder visibility derives during construction: a folder is `Hidden` when
every non-HEAD descendant branch is in `hidden_branches` (and it has at
least one), `Visible` when none are, and `Mixed` otherwise. A folder whose
only descendant is the HEAD branch is `Visible` and its toggle is a no-op.

### Collapse state

`App` gains `collapsed_branch_folders: BTreeSet<String>`, keyed by full
folder path. It starts empty (all folders expanded), mutates by simple
insert/remove on folder-row click, and is cleared wherever a repository is
opened, alongside `hidden_branches`. No delta-against-default machinery is
needed because the default is uniformly "expanded."

### Rendering

`render_branch_sidebar` iterates `BranchTreeRow`s instead of raw branches.
Folder rows render a chevron (right when collapsed, down when expanded), the
folder name, and indent proportional to depth, using the existing sidebar
row height, font, and color constants. Branch rows render exactly as today
except the label is the final segment and the row is indented by depth.
Hover tracking (`hovered_branch_row`) indexes into the new row list.

Folder rows take debug selector `branch-folder-{path}` and their toggle
`branch-folder-visibility-{path}`. Branch-row selectors are unchanged
(`branch-row-{full-name}` and friends), keeping existing view tests valid.

### Folder interaction

Clicking a folder row toggles its presence in `collapsed_branch_folders` and
notifies. Folder rows never focus a commit.

Clicking a folder's eye toggle stops propagation, then: if any non-HEAD
descendant branch is visible, inserts all of them into `hidden_branches`,
runs the existing `clear_selection_if_hidden` pass once, and notifies once;
otherwise removes all descendants from `hidden_branches` and notifies. This
batches the whole folder flip into a single state change rather than
simulating per-branch toggles.

The hover-reveal rules match branch rows: a `Visible` folder shows its eye
only on row hover; `Hidden` and `Mixed` folders always show eye-off, and
only a `Hidden` folder's name uses the muted `0x999999` color.

### Spec update

`docs/specs/review/workflow.md` gains the nesting contract in the same
change: the grouping rule, multi-level nesting, display-name truncation,
folder collapse semantics (visual only, session-only, expanded by default),
folder visibility toggle semantics, and the HEAD exemption.

## Verification

Per ADR-0003, the change ships with tests at two levels:

1. Unit tests for `build_branch_tree_rows` and folder-visibility
   derivation: a slash-named branch nests under its folders; multi-level
   names produce one folder per segment; a lone slash-named branch still
   gets a folder; folders sort before branches and siblings sort
   alphabetically; a collapsed folder emits no descendant rows; visibility
   derives to `Visible`/`Hidden`/`Mixed` correctly, with the HEAD branch
   excluded from the computation.
2. `#[gpui::test]` view tests: a nested branch renders a folder row and an
   indented branch row showing the final segment; clicking the folder
   collapses it and removes descendant rows, clicking again restores them;
   clicking a folder's eye toggle hides all descendant branches from the
   graph and the folder renders hidden; toggling again shows them; a folder
   containing HEAD hides everything except HEAD; reopening a repository
   resets collapse state.

`bin/check` must pass before the work is declared done.
