# File Diff Shell Design

This design defines the next implementation slice for `docs/specs/review/workflow.md`: selecting a file in an open changeset opens a file-detail surface. The slice intentionally stops before textual diff computation or rendering.

## Scope

When review mode contains changed files, each changed-file row is selectable. Selecting a file makes that file visually distinct in the file list and renders a detail pane with the file path, change kind, and rename source path when present. The detail pane contains a placeholder where the file diff will render in a later slice.

If no changed file is selected, the detail pane shows a neutral empty state. Empty changesets continue to show the existing empty-state message and do not render a selectable file list.

## Recommended Approach

Keep the selected changed-file path as app state separate from `ReviewScreen`. This preserves the file selection when the user closes review mode and reopens the same changeset, while still letting graph mode remain the active surface outside review mode. Reset the selected changed-file path when opening a different repository.

Two alternatives are deferred. Storing file selection inside `ReviewScreen::Changeset` would drop selection on close, which does not match the desired reopen behavior. Computing real file contents in this slice would conflate interaction state with diff rendering and make the change too broad.

## Data Flow

Clicking a changed-file row records the row's path as the selected changed-file path. Review rendering looks up that path in the current `ChangeSet`. If the path is present, the right-side pane renders the file summary. If the path is absent, the pane renders an empty state. Opening a changeset preserves the existing selected changed-file path only if that file exists in the newly computed changeset; otherwise it clears the file selection.

## Testing

State tests should prove selecting a changed file records the path, opening a changeset clears stale file selections, and close/reopen preserves a valid selection. View tests should click a rendered changed-file row and assert that a file-detail pane appears through a debug selector. Smoke coverage should extend the existing golden path by selecting `hello.txt` after opening the fixture changeset.

## Definition of Done

The slice is complete when changed-file rows can be selected, the review screen renders a file-detail shell for the selected file, stale selections are cleared when they no longer apply, targeted gpui tests pass, and `bin/check` passes.
