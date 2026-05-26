# Changeset File List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the net changed-file list for a selected single commit when review mode opens.

**Architecture:** Add a small repo-layer changeset snapshot computed on demand from `git2` tree diffs. Store the computed snapshot in `ReviewScreen::Changeset` so the UI renders stable data and does not hold libgit2 handles. Keep file selection and diff rendering out of this slice.

**Tech Stack:** Rust 2021, gpui `0.2`, gpui-component `0.5`, git2 `0.20`, `#[gpui::test]`, Cargo integration tests.

---

## File Structure

- Modify `src/repo/mod.rs`: add `ChangeSet`, `ChangedFile`, `ChangeKind`, a user-facing changeset error, and `changeset_for_single_commit`.
- Modify `src/app.rs`: store `ChangeSet` in review-screen state, open changesets through the repo layer, render changed-file rows, and add gpui tests.
- Modify `tests/repo.rs`: add integration coverage for the checked-in two-commit fixture changeset.
- Modify `tests/smoke.rs`: extend the golden path from repository open to selected HEAD changeset.
- No required change to `docs/specs/review/workflow.md`: the existing review workflow spec already describes this behavior.

### Task 1: Repo Changeset Model

**Files:**
- Modify: `src/repo/mod.rs`
- Modify: `tests/repo.rs`

- [x] **Step 1: Write failing repo tests**

Add tests that call `changeset_for_single_commit(repo_path, sha)` and assert:

```rust
assert_eq!(changeset.files[0].path, "hello.txt");
assert_eq!(changeset.files[0].kind, ChangeKind::Modified);
```

for the fixture HEAD, plus inline unit tests for root-added, deleted, and renamed files.

- [x] **Step 2: Run repo tests to verify they fail**

Run:

```bash
cargo test repo::tests::changeset_for_root_commit_lists_added_files
cargo test repo::tests::changeset_for_commit_lists_deleted_files
cargo test repo::tests::changeset_for_commit_lists_renamed_files
cargo test --test repo open_at_reads_the_two_commits_fixture
```

Expected: FAIL to compile because the changeset API and types do not exist.

- [x] **Step 3: Implement minimal repo changeset API**

Use `git2::Repository::open`, find the target commit by SHA, diff the target tree against the first parent tree or `None` for root commits, and convert each delta into sorted `ChangedFile` values with `ChangeKind::{Added, Modified, Deleted, Renamed}`.

- [x] **Step 4: Run repo tests**

Run:

```bash
cargo test repo::tests::changeset_for_root_commit_lists_added_files
cargo test repo::tests::changeset_for_commit_lists_deleted_files
cargo test repo::tests::changeset_for_commit_lists_renamed_files
cargo test --test repo
```

Expected: PASS.

### Task 2: Review State And UI Rendering

**Files:**
- Modify: `src/app.rs`

- [x] **Step 1: Write failing view/state tests**

Add gpui tests that open a real repo, select the HEAD commit, call the open-changeset path, and assert `ReviewScreen::Changeset` carries a `ChangeSet` containing `hello.txt`. Add a rendered interaction test that clicks `open-changeset` and finds a `changed-file-row-0` debug selector.

- [x] **Step 2: Run view tests to verify they fail**

Run:

```bash
cargo test app::tests::opening_changeset_loads_changed_files
cargo test app::tests::clicking_open_changeset_renders_changed_files
```

Expected: FAIL because `ReviewScreen::Changeset` does not carry `ChangeSet` and changed-file rows do not render.

- [x] **Step 3: Implement review-state and rendering changes**

Change `ReviewScreen::Changeset` to hold `{ sha, changeset }`. Update `open_changeset` to compute the changeset through `repo::changeset_for_single_commit`, preserve graph mode and notify on errors, and render a file-list body with debug selectors.

- [x] **Step 4: Run app tests**

Run:

```bash
cargo test app::tests::opening_changeset_loads_changed_files
cargo test app::tests::clicking_open_changeset_renders_changed_files
cargo test app::tests
```

Expected: PASS.

### Task 3: Smoke Coverage And Full Verification

**Files:**
- Modify: `tests/smoke.rs`

- [x] **Step 1: Write failing smoke assertion**

Extend the existing open-repo smoke path to select the fixture HEAD, open the changeset, and assert the changeset contains `hello.txt` as modified.

- [x] **Step 2: Run smoke test to verify failure or coverage gap**

Run:

```bash
cargo test --test smoke
```

Expected before implementation: FAIL because review state does not carry the file list. Expected after implementation: PASS.

- [x] **Step 3: Format and verify**

Run:

```bash
cargo fmt
bin/check
```

Expected: PASS.
