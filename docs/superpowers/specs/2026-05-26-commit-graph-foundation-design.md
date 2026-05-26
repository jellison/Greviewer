# Commit Graph Foundation Design

This design defines the next implementation slice for `docs/specs/review/workflow.md`: after a repository opens, Greviewer should show real commit history instead of only the current HEAD. The full spec calls for graphical lanes, merge connectors, progressive loading, live refresh, and selection. This slice deliberately stops earlier. It builds the data and first visible graph-mode surface that later slices can extend.

## Scope

The slice reads a bounded list of commits from the opened repository and renders each visible commit with the metadata the workflow spec requires: short identifier, summary line, author, and authored date. The currently checked-out tip is marked independently from future review selection. Empty repositories still open successfully and show an empty graph state.

The slice does not implement lane geometry, merge connectors, progressive loading, live repository refresh, commit selection, range selection, changesets, or diffs. Those remain separate implementation slices.

## Recommended Approach

Add commit history to the existing repository snapshot returned by `repo::open_at`. This keeps the UI simple: opening a repository produces one immutable snapshot with the path, optional HEAD information, and the initial commit list. The app renders that snapshot directly in graph mode.

Two alternatives were rejected. Building the full visual graph now would combine data access, lane layout, rendering, scrolling, and selection in one change, which is too large for the next slice. Rendering placeholder rows without real Git history would create UI motion without product progress. A data-backed history list is the smallest useful step toward the spec.

## Module Layout

`src/repo/mod.rs` remains the owner of Git reads. It gains a `CommitInfo` snapshot type and a bounded revwalk helper. `src/app.rs` remains the root view and renders the commit list when `Mode::RepoOpen` is active.

No new `graph` module is introduced yet. The code does not have graph layout logic to isolate; adding a module now would mostly be ceremony. A later lane-layout slice can introduce `src/graph/` once there is a real graph algorithm and view state to own.

## Data Contract

Each commit snapshot contains:

- the full SHA for stable identity;
- the short SHA for display;
- the summary line;
- the author display name;
- the authored timestamp for deterministic ordering and future sorting;
- a formatted authored date for the UI;
- the parent count for future lane and merge handling;
- a marker for whether this commit is the checked-out tip.

The repository snapshot preserves the existing `head` field so current tests and app code keep their simple HEAD contract. The first commit in the returned list should be the current HEAD for a normal repository.

## Testing

Repository tests should prove that `open_at` reads commit history in newest-to-oldest order for the existing two-commit fixture and returns an empty commit list for an unborn repository. A focused unit test should cover authored-date formatting because that logic is local and easy to regress.

App-level gpui tests should continue to drive repository opening through the public action path. Once a fixture repository opens, the root app state should expose two commits with the expected summaries, proving the UI-visible graph mode is backed by real history.

## Definition of Done

The slice is complete when the app opens the existing fixture repository, the repository snapshot contains the expected commit list, graph mode renders commit rows with required metadata, unborn repositories show an empty graph message, and `bin/check` passes.
