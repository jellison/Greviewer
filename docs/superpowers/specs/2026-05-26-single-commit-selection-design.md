# Single Commit Selection Design

This design defines the next implementation slice for `docs/specs/review/workflow.md`: after the commit history renders, clicking a commit should stage that commit as the tentative review selection. The slice intentionally implements only single-commit selection. Range selection, shift-click ancestry validation, clear-selection controls, opening changesets, and diff/file-tree surfaces remain later slices.

## Scope

The user can click a commit row in graph mode to select that commit. The selected row becomes visually distinct from unselected rows. Clicking the same selected commit again clears the selection. Clicking a different commit moves the single-commit selection to that commit. Opening a different repository clears the prior selection because selection belongs to the currently open repository.

The selection is tentative and does not start review activity. It does not move `HEAD`, modify the repository, or open a changeset.

## Recommended Approach

Keep single-commit selection in the root app state for this slice. The app already owns graph-mode rendering, and there is not yet a dedicated graph module or review workflow model. A small `Selection` enum on `App` gives later slices a natural place to add range selection or move selection into a focused module once the behavior grows.

Two alternatives were rejected. Creating `src/selection/` now would add module ceremony before the behavior needs it. Encoding selection inside `repo::OpenRepository` would mix user interaction state into a Git snapshot, making repository data harder to reason about and reuse.

## Data Flow

`App` gains a selection field whose initial state is empty. When a repository opens successfully, the app stores the new repository snapshot and clears selection. Each commit row is rendered with a click handler that passes the commit SHA back to the app. The app toggles selection: if the clicked SHA is already selected, selection clears; otherwise the clicked SHA becomes the selected single commit.

Rendering derives selected styling by comparing each row's commit SHA to the active single selection. The visual treatment should be deliberately simple in this slice: a selected row background/border change is enough to satisfy the spec.

## Testing

The slice needs both focused state tests and a view interaction test. Focused tests can call the app selection helper directly to prove toggle semantics. A gpui view test should open the fixture repository, locate a rendered commit row through a debug selector, simulate a mouse click on the row, and assert that the app's public state now contains the clicked commit SHA. Clicking the same row again should clear the selection.

The test should not depend on pixel-perfect layout. It should use gpui's rendered debug bounds only to find a point inside the row and then assert on app state.

## Definition of Done

The slice is complete when single commit selection works through row clicks, selected rows render distinctly, selection clears on repeat click and repository changes, targeted tests pass, and `bin/check` passes.
