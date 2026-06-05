# Review Workflow

This contract defines the review experience: opening a Git repository, navigating its commit graph, selecting commits to review, browsing the rollup change set, and inspecting individual file diffs.

## Opening a repository

The user opens a local Git repository through a folder picker. The application reads the repository's history and presents the commit graph for that repository. Recently opened repositories persist between application launches and are surfaced on launch so the user can return to a repository without re-picking it. Only one repository is open per window in v1.

**Triggering conditions**

- The user picks a folder via the open-repository affordance on launch or from within an open window.
- The user activates a recently opened repository entry on launch.

**Observable outcomes**

- The window displays the commit graph of the chosen repository.
- The native titlebar identifies the open repository by repository name only.
- The chosen repository is added to (or moved to the top of) the recently opened list, and that ordering is retained for future launches.

**Edge cases**

- A chosen folder that is not a Git repository surfaces a clear error and the previous window state is preserved, including the native titlebar text. If no repository was open, the native titlebar remains blank.
- A repository with zero commits opens to an empty graph with a message explaining there is nothing to review.
- A recently opened repository whose folder has been moved or deleted is shown as unavailable; activating it surfaces a clear error and offers to remove it from the list.

## Quitting the application

The user can quit Greviewer with the standard quit keyboard shortcut.

**Triggering conditions**

- The user presses Cmd-Q while the Greviewer window is focused.

**Observable outcomes**

- The application begins its normal quit flow.

## Viewing the commit graph

The user sees a graphical history of the repository's commits with branch lanes and merge connectors. Branch and merge lines use smooth rounded bends where they turn between lanes, including the final turn into a branch commit. Each commit is presented as a single-row entry whose reading order is graph, short identifier, summary line, author, authored date, and any local branch names that point at it. The graph is scrollable; older commits load progressively as the user scrolls. The currently checked-out tip is visually marked, but selection is independent of checkout state — reviewing never moves HEAD.

**Triggering conditions**

- A repository is open.

**Observable outcomes**

- Commits appear in graphical order with branch lanes and merge connectors.
- Each visible commit shows its short identifier, summary line, author, and authored date.
- Local branch names are shown on the commits they point to.
- The currently checked-out tip carries a visual marker.

**Edge cases**

- Very large histories load progressively rather than blocking the UI.
- Histories with several active branches remain legible: the visible first-parent history of the top commit anchors the left-most lane, active branch lanes keep a stable horizontal position until they end, branch lanes render as continuous vertical lines through and between rows without row separators or rounded branch/merge joins interrupting them, new branch lanes use the nearest available lane to the right and do not reuse occupied lanes, multi-lane connectors stay continuous and horizontally aligned across intermediate lanes even when those lanes are occupied by other active branches, and branch labels do not obscure the graph.
- Detached-HEAD repositories render normally with no checked-out branch marker.

## Selecting commits to review

The user stages a review by selecting either a single commit or a contiguous sequential range from the graph. Selection is tentative: no review activity begins until the user explicitly opens the changeset (described below). Clicking a commit selects it. Shift-clicking a second commit extends the selection to a range, provided the two commits lie on a single ancestry path (one is an ancestor of the other). The selection is the inclusive set of commits between the two endpoints along that path. Clicking the selected commit again clears the selection.

**Triggering conditions**

- The user clicks a commit in the graph.
- The user shift-clicks a second commit while a single-commit selection is active.
- The user clicks the selected commit again.

**Observable outcomes**

- The selected commit or range is visually distinct in the graph.

**Edge cases**

- If the two endpoints do not share a linear ancestry — they lie on diverged branches — the second click is rejected with a message explaining why, and the original selection is preserved.
- Merge commits inside a selected range are included in the range and contribute to the rollup.

## Opening the changeset

With a valid selection in place, the user opens the changeset to begin reviewing. Opening transitions the window from graph mode to review mode, where the file tree and diff viewer are the primary content. The graph remains the basis for the open selection but is not the active surface. The user closes the open changeset to return to graph mode; the prior selection is preserved so it can be adjusted and re-opened without re-selecting from scratch. Modifying the selection while a changeset is open is not supported in v1 — the user closes the changeset, adjusts, and reopens.

**Triggering conditions**

- The user activates the open-changeset affordance while a valid selection is active.
- The user closes the open changeset.

**Observable outcomes**

- Activating the open-changeset affordance transitions the window into review mode and renders the change set for the current selection.
- The open-changeset affordance is unavailable when no selection is active or when the selection is not a valid contiguous range.
- Closing the changeset returns the window to graph mode with the prior selection preserved.
- Review mode does not move HEAD or modify the repository.

**Edge cases**

- Opening a changeset whose net effect is no change still transitions to review mode; the change set view shows the empty-state message described in the next section.

## Reviewing the change set

With a changeset open, the user sees the rollup change set: a file tree containing every file whose content differs between the state immediately before the oldest selected commit and the state at the newest selected commit. The tree shows each file's path and an indicator of how it changed (added, modified, deleted, renamed). Selecting a file in the tree opens that file's diff (described below).

**Triggering conditions**

- A changeset is open.

**Observable outcomes**

- The change set lists every net-changed file across the selection.
- Each entry shows its path and its change indicator (added / modified / deleted / renamed).
- Selecting a file opens it for inspection.

**Edge cases**

- A selection whose net effect is no change (e.g., a commit and its revert within the same range) shows an empty change set with a message explaining the net result.
- Binary files appear in the tree with an indicator that no textual diff is available; selecting one opens an explanatory placeholder.
- Renamed files appear once with their new path and an indicator of the rename; the diff displays the rename's content delta.

## Seeing all files for context

The user can toggle a view that shows every file in the repository at the newest selected commit, not just the files in the change set. Files that are part of the change set remain marked so they are distinguishable from unchanged files. Selecting an unchanged file opens it for read-only viewing rather than as a diff.

**Triggering conditions**

- The user toggles the all-files view while a changeset is open.

**Observable outcomes**

- The file tree expands to show every file at the newest selected commit.
- Files in the change set retain their change indicator; unchanged files render without one.
- Selecting an unchanged file opens it as a read-only view of its contents at the newest selected commit.

**Edge cases**

- The toggle preserves the user's current expanded-folder state in the tree where possible.

## Inspecting a file's diff

Opening a file in the change set presents its diff. For files that exist on both sides of the selection (modified or renamed files), the diff is shown side-by-side: the "before" state on the left, the "after" state on the right, with corresponding lines aligned. Added, removed, and modified lines are visually distinguished. The user can scroll both sides; the alignment is preserved as they scroll. Line numbers are shown for each side.

For files that exist on only one side of the selection — added or deleted files — the diff is shown full-width as a single pane. An added file shows its post-add content full-width; a deleted file shows its pre-delete content full-width. The full-width treatment exists because an empty pane wastes screen space without conveying additional information.

**Triggering conditions**

- The user selects a changed file in the change set.

**Observable outcomes**

- Modified and renamed files render as a side-by-side diff with aligned, scroll-synchronized panes.
- Added files render full-width showing the post-add content.
- Deleted files render full-width showing the pre-delete content.
- Added, removed, and modified lines are visually distinguished.
- Line numbers appear for each side of a side-by-side diff and for the single side of a full-width view.

**Edge cases**

- Renamed files diff old-path content against new-path content; the file's pre-rename path is surfaced so the user can see what was renamed.
- Binary files render an explanatory placeholder rather than attempting a textual diff.
