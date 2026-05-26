# Changeset Shell Design

This design defines the next implementation slice for `docs/specs/review/workflow.md`: with a single commit selected, the user can enter review mode. The slice intentionally creates only the review-mode shell. It does not compute changed files, render the file tree, or display diffs.

## Scope

When graph mode has no selection, the open-changeset affordance is unavailable. When a single commit is selected, the user can open the changeset. The app switches from graph mode to review mode and shows a placeholder that identifies the selected commit. Closing the changeset returns to graph mode and preserves the existing selection so the user can adjust it or reopen the same changeset.

Review mode does not move `HEAD`, modify the repository, or alter selection. Changing selection while review mode is open remains unsupported in this slice.

## Recommended Approach

Keep review-screen state in the root app for now. The root app already owns repository-open mode, commit selection, and graph rendering. A small `ReviewScreen` enum gives this slice the minimum state needed to distinguish graph mode from review mode without creating a premature `changeset` module.

Two alternatives were rejected. Computing the changed-file list in the same slice would conflate navigation state with Git diff behavior and make the change too broad. Building a dedicated changeset module now would mostly contain placeholder rendering, not real changeset logic.

## Data Flow

`App` gains a `review_screen` field. Opening a repository resets it to graph mode. The open-changeset action checks for a single active selection. If present, it records that selection as the active review target and switches to review mode. If no selection is active, the action is a no-op.

Graph mode renders the existing commit list plus an open-changeset affordance only when a selection exists. Review mode renders the repository header, a placeholder for the active changeset target, and a close affordance. Closing sets the screen back to graph mode while leaving selection unchanged.

## Testing

State tests should prove that open-changeset is a no-op without selection, opens with a selected commit, and close returns to graph mode while preserving selection. View tests should drive the rendered open and close affordances through gpui clicks, not only call helper methods directly. Existing smoke tests should continue to prove that opening a repository starts in graph mode with no selection.

## Definition of Done

The slice is complete when a selected commit can enter and leave review mode through the UI, no-selection opens are unavailable or no-op, selection is preserved after close, targeted tests pass, and `bin/check` passes.
