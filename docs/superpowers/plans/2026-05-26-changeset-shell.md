# Changeset Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user open a placeholder changeset review shell for a selected commit, then close back to graph mode with selection preserved.

**Architecture:** Add root app review-screen state because this slice only controls navigation between graph mode and review mode. Use gpui actions and rendered affordances so both dispatch and click paths are testable. Keep changed-file computation, file tree, and diff rendering for later slices.

**Tech Stack:** Rust 2021, gpui `0.2`, gpui-component `0.5`, git2 `0.20`, `#[gpui::test]`, Cargo integration tests.

---

## File Structure

- Modify `src/app.rs`: add `OpenChangeset` and `CloseChangeset` actions, `ReviewScreen` state, open/close helpers, graph affordance, review shell rendering, and gpui tests.
- Modify `tests/smoke.rs`: assert opened repositories start in graph mode.
- No changes to `src/repo/mod.rs`: commit snapshots already provide selected SHAs.
- No changes to `docs/specs/review/workflow.md`: existing opening-changeset contract covers this behavior.

### Task 1: Review Screen State

**Files:**
- Modify: `src/app.rs`
- Modify: `tests/smoke.rs`

- [x] **Step 1: Write failing state tests**

Add tests that assert:

```rust
app.review_screen == ReviewScreen::Graph
```

after opening a repository, opening changeset without selection leaves `ReviewScreen::Graph`, opening with a selected commit switches to `ReviewScreen::Changeset { sha }`, and closing returns to graph mode while keeping `Selection::Single`.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test app::tests::opening_changeset_requires_a_selection
cargo test app::tests::closing_changeset_returns_to_graph_and_preserves_selection
cargo test --test smoke
```

Expected: FAIL to compile because `ReviewScreen`, `App::review_screen`, and open/close helpers do not exist.

- [x] **Step 3: Implement minimal review-screen state**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewScreen {
    Graph,
    Changeset { sha: String },
}
```

Add `review_screen: ReviewScreen` to `App`, initialize to `Graph`, reset to `Graph` on repository open, add `open_changeset` and `close_changeset` helpers, and make no-selection open a no-op.

- [x] **Step 4: Run targeted state tests**

Run:

```bash
cargo test app::tests::opening_changeset_requires_a_selection
cargo test app::tests::closing_changeset_returns_to_graph_and_preserves_selection
cargo test --test smoke
```

Expected: PASS.

### Task 2: Actions And Rendered Affordances

**Files:**
- Modify: `src/app.rs`

- [x] **Step 1: Write failing interaction tests**

Add tests that dispatch `OpenChangeset` and `CloseChangeset` through gpui actions, and a test that clicks the rendered `open-changeset` and `close-changeset` debug selectors.

- [x] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test app::tests::dispatching_open_and_close_changeset_actions_updates_review_screen
cargo test app::tests::clicking_changeset_affordances_enters_and_exits_review_mode
```

Expected: FAIL because actions, handlers, debug selectors, and rendered affordances do not exist.

- [x] **Step 3: Implement actions and shell rendering**

Add `OpenChangeset` and `CloseChangeset` to the app actions. Register root `on_action` handlers. In graph mode, render an open-changeset affordance only when `Selection::Single` exists. In review mode, render a placeholder shell and close affordance.

- [x] **Step 4: Run targeted interaction tests**

Run:

```bash
cargo test app::tests::dispatching_open_and_close_changeset_actions_updates_review_screen
cargo test app::tests::clicking_changeset_affordances_enters_and_exits_review_mode
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
