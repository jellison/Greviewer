# Changeset File List Design

This design defines the next implementation slice for `docs/specs/review/workflow.md`: when a single selected commit is opened in review mode, the app lists the files changed by that commit. The slice intentionally stops before selecting a file or rendering its diff.

## Scope

Review mode replaces the placeholder body with a changed-file list for the selected commit. Each row shows the file path and a concise change kind: added, modified, deleted, or renamed. Root commits diff against an empty tree. Non-root commits diff against their first parent, which matches the current single-commit selection model.

The list is a net file summary only. It does not render hunks, line counts, binary placeholders, range selections, all-files mode, or file-selection state.

## Recommended Approach

Keep Git diff computation in `src/repo/mod.rs` beside the existing repository snapshot code. Add small snapshot structs that the UI can render without holding libgit2 handles: `ChangeSet`, `ChangedFile`, and `ChangeKind`. Compute the file list on demand when the app opens a changeset, then store it in review-screen state.

Two alternatives are deferred. Computing diffs during `open_at` would do unnecessary work for every visible commit before the user starts a review. Creating a separate changeset module is likely soon, but this slice only has one consumer and can stay with the existing repo boundary until file selection and diff content arrive.

## Data Flow

When `open_changeset` sees a single selected commit, it asks the repo layer for the selected commit's changed files. On success, the app switches to review mode with the selected SHA and computed `ChangeSet`. On failure, it preserves graph mode and surfaces the error through the existing notification path.

Review mode renders the repository header, a close affordance, and the changed-file list. An empty change set renders a clear empty state. Closing the changeset returns to graph mode and preserves the existing selection.

## Testing

Repo tests should prove single-commit changeset computation for modified files, root commits, deleted files, and renamed files. View tests should prove opening a real selected commit stores a `ChangeSet` and renders a changed-file row through a debug selector. Smoke coverage should extend the open-repo path through selecting the fixture HEAD and opening its changeset.

## Definition of Done

The slice is complete when a selected single commit opens review mode with a file list populated from Git, empty changesets have a visible empty state, failure preserves the previous state, targeted tests pass, and `bin/check` passes.
