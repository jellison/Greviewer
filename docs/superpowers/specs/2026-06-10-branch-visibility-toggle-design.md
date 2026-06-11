# Design: Branch Visibility Toggles in the Graph Sidebar

## Context and Problem

The commit graph shows every unmerged local branch (`b69fce2`), and the branch
sidebar (`2026-06-09-graph-branch-sidebar-design.md`) lists them all. That is
the right default, but it gives the user no way to reduce noise: a repository
with several stale work-in-progress branches forces their commits and lanes
into the graph permanently. The user cannot say "I don't care about this
branch right now."

This design adds a per-branch visibility toggle to the sidebar. A branch that
is toggled off loses its ref label in the graph, and any commit reachable
*only* from hidden branches disappears entirely — the graph re-flows as if
those commits did not exist. Commits shared with a visible branch (merged
work, common ancestry) stay.

## Requirements and Non-Goals

Requirements:

1. Each non-HEAD branch row in the sidebar carries a visibility toggle,
   revealed on row hover, following the sidebar's existing hover-reveal
   pattern. A hidden branch's toggle is always visible.
2. The checked-out (HEAD) branch cannot be hidden and shows no toggle. The
   graph therefore always has its lane-0 trunk anchor.
3. Hiding a branch removes its name from commit ref labels and removes every
   commit not reachable from a visible branch tip or HEAD.
4. Toggling is instant and purely in-memory: no Git re-read, no async work.
5. If hiding a branch removes the currently selected commit (or any commit in
   a range selection), the selection is cleared.
6. Visibility state is session-only and resets when a repository is opened or
   reopened.

Non-goals: persistence across restarts, hide-all/show-all bulk actions,
remote branches and tags (the sidebar does not list them), and any change to
how commits are paged from Git. These can come later without reworking this
design.

## Alternatives Considered

**Filter at the revwalk.** Re-run `read_commit_page` pushing only visible
branch tips, letting Git compute reachability exactly. Rejected: every toggle
becomes a repository re-read with async plumbing, and the paged commit list
would need invalidating and re-fetching on each flip. Visibility is a view
concern; it should not reach into the data layer.

**Hide labels only, dim exclusive commits.** Cheapest to build, but it fails
the core requirement: a noisy branch's commits would still occupy rows and
lanes, so the graph would not actually get quieter.

**Chosen: in-memory reachability filter.** The loaded `CommitInfo` list
already carries `parent_shas`, so the set of visible commits is a graph walk
the app can run itself: seed from HEAD plus every visible branch tip, follow
parents, stop at the loaded-page boundary. The repo module stays untouched,
toggles are O(loaded commits), and flipping a branch back on needs no reload.

## Design

### State (`src/app.rs`)

`App` gains `hidden_branches: HashSet<String>`, holding branch names the user
has toggled off. It is cleared wherever a repository is opened, alongside the
other per-repo view state. Nothing is persisted.

### Visible-commit computation

A pure function in `src/app.rs` (next to the other selection/graph helpers):

```rust
fn visible_commit_shas(
    commits: &[CommitInfo],
    local_branches: &[LocalBranch],
    head_sha: Option<&str>,
    hidden_branches: &HashSet<String>,
) -> HashSet<String>
```

It seeds a worklist with `head_sha` and the `tip_sha` of every branch not in
`hidden_branches`, then walks `parent_shas` over a sha→commit index built from
the loaded list. Parents that are not loaded simply terminate the walk — a
commit beyond the paging boundary cannot be on screen anyway, and paging in
more history re-runs the computation over the larger list. When
`hidden_branches` is empty the function is the identity over loaded commits
(every loaded commit is reachable from some pushed tip, by construction of the
revwalk), so the default render is unchanged.

`render_graph_screen` filters `repo.commits` through this set before building
the `GraphCommit` list, so `layout_graph_anchored` never sees hidden commits
and lanes re-flow naturally. The computation runs per render, matching the
existing pattern of recomputing graph layout per render; if profiling ever
objects, the set can be cached on (commit count, hidden set) without changing
this design.

### Label filtering

`render_commit_ref_labels` skips branch names present in `hidden_branches`.
This matters for the case where a hidden branch's tip is still visible because
a visible branch reaches it (a fast-forward ancestor, for example): the commit
row stays, but the hidden branch's label badge does not. The HEAD label is
unaffected.

### Sidebar toggle UI

Each non-HEAD branch row gains a trailing eye icon (Lucide `eye` /
`eye-off`, consistent with `2026-06-07-lucide-icons-design.md`):

1. Visible branch, row not hovered: no icon.
2. Visible branch, row hovered: `eye` icon; clicking it hides the branch.
3. Hidden branch: `eye-off` icon always shown, branch name in the muted
   `0x999999` color; clicking the icon shows the branch again.

The icon gets its own `debug_selector` (`branch-visibility-{name}`) and its
click handler stops propagation so it never triggers the row's focus
behavior. The row body of a *hidden* branch does not focus on click — its tip
may not exist in the graph — so the click is a no-op until the branch is shown
again. The HEAD branch row renders no icon in any state.

### Selection clearing

The toggle-off handler recomputes the visible set and, if the current
`Selection` references any sha no longer visible (the single sha, or any sha
in a range), resets it to `Selection::None` along with the dependent diff
state, using the same path the app already uses when a selection becomes
invalid.

## Verification

Per ADR-0003, the change ships with tests at two levels:

1. Unit tests for `visible_commit_shas`: hiding a branch removes its
   exclusive commits; shared ancestry survives; the HEAD chain is always
   present; an empty hidden set returns all loaded commits; a parent beyond
   the loaded boundary does not panic.
2. `#[gpui::test]` view tests: toggling a branch off removes its exclusive
   commit rows and its ref label, and the eye-off icon renders; toggling back
   on restores them; the HEAD branch row renders no toggle; hiding the branch
   containing the selected commit clears the selection; reopening a
   repository resets hidden state.

`docs/specs/review/workflow.md`'s branch-visibility material gains a section
covering toggles in the same change, and `bin/check` must pass before the work
is declared done.
