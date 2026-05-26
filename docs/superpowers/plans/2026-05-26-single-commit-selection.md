# Single Commit Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users select and clear a single commit by clicking commit rows in graph mode.

**Architecture:** Add a small app-owned selection model because selection is user interaction state, not repository data. Wire commit rows with gpui click handlers and debug selectors so view tests can drive the real row interaction. Keep range selection, changeset opening, and dedicated selection modules out of this slice.

**Tech Stack:** Rust 2021, gpui `0.2`, gpui-component `0.5`, git2 `0.20`, `#[gpui::test]`, Cargo integration tests.

---

## File Structure

- Modify `src/app.rs`: add `Selection`, app selection state, row click handler, selected row styling, and gpui view tests.
- Modify `tests/smoke.rs`: assert the action-driven open path starts with no selection.
- No changes to `src/repo/mod.rs`: repository snapshots already expose stable commit SHAs.
- No changes to `docs/specs/review/workflow.md`: the existing selection contract already covers this behavior.

### Task 1: Selection State

**Files:**
- Modify: `src/app.rs`
- Modify: `tests/smoke.rs`

- [x] **Step 1: Write failing state tests**

In `src/app.rs`, add app view tests that open a real repo, select the first commit SHA, move selection to the second SHA, and clear by selecting the second SHA again. Add a smoke assertion that an opened repository starts with no active selection.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test app::tests::selecting_commits_toggles_single_selection
cargo test --test smoke
```

Expected: FAIL to compile because `Selection`, `App::selection`, and selection helpers do not exist.

- [x] **Step 3: Implement minimal selection state**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    None,
    Single { sha: String },
}
```

Add `selection: Selection` to `App`, initialize it to `Selection::None`, clear it on successful repository open, and add a `select_single_commit(&mut self, sha: String, cx: &mut Context<Self>)` helper that toggles the clicked SHA.

- [x] **Step 4: Run targeted state tests**

Run:

```bash
cargo test app::tests::selecting_commits_toggles_single_selection
cargo test --test smoke
```

Expected: PASS.

### Task 2: Clickable Commit Rows

**Files:**
- Modify: `src/app.rs`

- [x] **Step 1: Write failing view interaction test**

In `src/app.rs`, add a `#[gpui::test]` that opens the two-commit fixture, uses `VisualTestContext::debug_bounds("commit-row-0")`, simulates a click on that row, and asserts `Selection::Single` contains the first commit SHA. Simulate another click on the same bounds and assert `Selection::None`.

- [x] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test app::tests::clicking_a_commit_row_toggles_selection
```

Expected: FAIL because rows have no click handler or debug selector yet.

- [x] **Step 3: Implement row click handling**

Pass the row index and selected state into `render_commit_row`. Add a stable debug selector `commit-row-{index}`, a left-click handler that calls `select_single_commit`, and selected row styling.

- [x] **Step 4: Run targeted view tests**

Run:

```bash
cargo test app::tests::clicking_a_commit_row_toggles_selection
cargo test app::tests
```

Expected: PASS.

### Task 3: Full Verification

**Files:**
- All modified files.

- [x] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: PASS.

- [x] **Step 2: Run project verification**

Run:

```bash
bin/check
```

Expected: PASS.
