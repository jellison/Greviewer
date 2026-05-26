# File Diff Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user select a changed file in review mode and see a file-detail placeholder shell for that file.

**Architecture:** Keep `selected_changed_file_path: Option<String>` on `App` so selection can survive closing and reopening review mode. Use the current `ChangeSet` snapshot to validate and render the selected file. Keep textual diff computation and line rendering for the next slice.

**Tech Stack:** Rust 2021, gpui `0.2`, gpui-component `0.5`, git2 `0.20`, `#[gpui::test]`, Cargo integration tests.

---

## File Structure

- Modify `src/app.rs`: add selected changed-file state, row click handling, selected row styling, two-pane review layout, file-detail placeholder rendering, and gpui tests.
- Modify `tests/smoke.rs`: extend the golden path to click the changed-file row and assert the file-detail shell renders.
- No required change to `src/repo/mod.rs`: the existing `ChangeSet` and `ChangedFile` snapshots provide all data needed for this shell.
- No required change to `docs/specs/review/workflow.md`: selecting a file and opening its diff are already covered by the review workflow spec.

### Task 1: Selected File State

**Files:**
- Modify: `src/app.rs`

- [x] **Step 1: Write failing state tests**

Add gpui tests that:

```rust
assert_eq!(app.selected_changed_file_path, Some("hello.txt".to_string()));
```

after selecting a changed file, prove closing and reopening the same changeset preserves that selection, and prove opening a changeset clears a stale file path that is not present in the new `ChangeSet`.

- [x] **Step 2: Run state tests to verify they fail**

Run:

```bash
cargo test app::tests::selecting_changed_file_records_path
cargo test app::tests::reopening_changeset_preserves_valid_changed_file_selection
cargo test app::tests::opening_changeset_clears_stale_changed_file_selection
```

Expected: FAIL to compile because `selected_changed_file_path` and changed-file selection helpers do not exist.

- [x] **Step 3: Implement selected-file state**

Add `pub selected_changed_file_path: Option<String>` to `App`, initialize and reset it on repository open, add `select_changed_file`, and update `open_changeset` to clear the selected path when the computed changeset does not contain it.

- [x] **Step 4: Run state tests**

Run:

```bash
cargo test app::tests::selecting_changed_file_records_path
cargo test app::tests::reopening_changeset_preserves_valid_changed_file_selection
cargo test app::tests::opening_changeset_clears_stale_changed_file_selection
```

Expected: PASS.

### Task 2: Rendered File Detail Shell

**Files:**
- Modify: `src/app.rs`

- [x] **Step 1: Write failing rendered interaction tests**

Add a gpui test that opens a repo, opens the changeset, clicks `changed-file-row-0`, and asserts debug selectors `file-detail-shell` and `selected-changed-file-row-0` exist.

- [x] **Step 2: Run rendered test to verify it fails**

Run:

```bash
cargo test app::tests::clicking_changed_file_renders_detail_shell
```

Expected: FAIL because changed-file rows are not clickable and the detail pane does not render.

- [x] **Step 3: Implement clickable rows and detail shell**

Update changed-file row rendering to take selection state and `Context<Self>`, add click handlers, add selected row styling, render a two-pane body for non-empty changesets, and add a detail pane with a selected-file summary or empty state.

- [x] **Step 4: Run app tests**

Run:

```bash
cargo test app::tests::clicking_changed_file_renders_detail_shell
cargo test app::tests
```

Expected: PASS.

### Task 3: Smoke Coverage And Full Verification

**Files:**
- Modify: `tests/smoke.rs`

- [x] **Step 1: Extend smoke coverage**

After the smoke test opens the fixture changeset, click `changed-file-row-0`, assert `file-detail-shell` exists, and assert `selected_changed_file_path == Some("hello.txt")`.

- [x] **Step 2: Run smoke test**

Run:

```bash
cargo test --test smoke
```

Expected: PASS after implementation.

- [x] **Step 3: Format and verify**

Run:

```bash
cargo fmt
bin/check
```

Expected: PASS.
