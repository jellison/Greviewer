# Branch Visibility Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-branch visibility toggles in the graph sidebar: a hidden branch loses its ref label and its exclusive commits disappear from the graph.

**Architecture:** Session-only `hidden_branches: BTreeSet<String>` on `App`. A pure function computes the visible-commit set by walking `parent_shas` from HEAD plus every visible branch tip over the loaded commit list. `render_graph_screen` filters commits through that set before graph layout; ref-label rendering skips hidden branch names; the sidebar grows a hover-revealed eye toggle per non-HEAD branch row. See `docs/superpowers/specs/2026-06-10-branch-visibility-toggle-design.md`.

**Tech Stack:** Rust, gpui, gpui-component, git2 (tests only). Verification via `bin/check`.

**Design deviation (intentional):** The spec sketches `hidden_branches: HashSet<String>`; we use `BTreeSet<String>` to match the codebase's existing per-repo view state (`collapsed_file_tree_paths: BTreeSet<String>`). Behavior is identical.

---

### Task 1: Vendor Lucide eye / eye-off icons

`LucideIcon` (src/icons.rs) has no eye variants, and the sidebar toggle needs them. Lucide is ISC-licensed (permissive, fine per ADR-0001). Follow the module's own doc comment: drop SVGs in `assets/icons/`, add variants, map in `path()`, add to `ALL`.

**Files:**
- Modify: `src/icons.rs`
- Create: `assets/icons/eye.svg`
- Create: `assets/icons/eye-off.svg`

- [ ] **Step 1: Add the enum variants (test will fail until assets exist)**

In `src/icons.rs`, add to the `LucideIcon` enum (alphabetical position, after `Columns2`):

```rust
    /// `eye.svg`
    Eye,
    /// `eye-off.svg`
    EyeOff,
```

Add to `ALL` (after `LucideIcon::Columns2`):

```rust
        LucideIcon::Eye,
        LucideIcon::EyeOff,
```

Add to the `path()` match (after the `Columns2` arm):

```rust
            LucideIcon::Eye => "icons/eye.svg",
            LucideIcon::EyeOff => "icons/eye-off.svg",
```

- [ ] **Step 2: Run the icons test to verify it fails**

Run: `cargo test --lib icons::tests::every_variant_resolves_to_a_vendored_asset`
Expected: FAIL with `no vendored asset for icons/eye.svg`

- [ ] **Step 3: Vendor the SVGs**

These are the current Lucide `eye` and `eye-off` sources, reformatted to match the attribute layout of the existing vendored files (compare `assets/icons/check.svg`).

Create `assets/icons/eye.svg`:

```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" />
  <circle cx="12" cy="12" r="3" />
</svg>
```

Create `assets/icons/eye-off.svg`:

```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49" />
  <path d="M14.084 14.158a3 3 0 0 1-4.242-4.242" />
  <path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143" />
  <path d="m2 2 20 20" />
</svg>
```

- [ ] **Step 4: Run the icons tests to verify they pass**

Run: `cargo test --lib icons::`
Expected: PASS (both `every_variant_resolves_to_a_vendored_asset` and `icon_widget_renders_without_panicking`)

- [ ] **Step 5: Commit**

```bash
git add src/icons.rs assets/icons/eye.svg assets/icons/eye-off.svg
git commit -m "feat(icons): vendor lucide eye and eye-off icons"
```

---

### Task 2: `visible_commit_shas` and the `hidden_branches` field

The pure reachability function plus the state it reads. No rendering changes yet.

**Files:**
- Modify: `src/app.rs` (use block ~line 27, `App` struct ~line 90, constructor ~line 370, `apply_open_repository` ~line 468, free functions near `render_commit_ref_labels` ~line 2408, tests module)

- [ ] **Step 1: Write failing unit tests**

In the `#[cfg(test)] mod tests` block of `src/app.rs`, near the existing `graph_commit` helper (~line 5077), add builders and tests:

```rust
    fn commit_info(sha: &str, parent_shas: &[&str]) -> repo::CommitInfo {
        repo::CommitInfo {
            sha: sha.to_string(),
            short_sha: sha.chars().take(7).collect(),
            summary: format!("commit {sha}"),
            author: "Tester".to_string(),
            authored_timestamp: 0,
            authored_date: "2026-01-01".to_string(),
            parent_shas: parent_shas.iter().map(|sha| sha.to_string()).collect(),
            branch_names: Vec::new(),
            parent_count: parent_shas.len(),
            is_head: false,
        }
    }

    fn local_branch(name: &str, tip_sha: &str) -> repo::LocalBranch {
        repo::LocalBranch {
            name: name.to_string(),
            tip_sha: tip_sha.to_string(),
            is_head: false,
        }
    }

    fn hidden(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }
```

And the tests (same module):

```rust
    #[test]
    fn hiding_a_branch_removes_its_exclusive_commits() {
        // feature-tip -> root <- main-tip; hiding feature drops feature-tip only.
        let commits = vec![
            commit_info("feature-tip", &["root"]),
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "feature-tip"),
            local_branch("master", "main-tip"),
        ];

        let visible =
            visible_commit_shas(&commits, &branches, Some("main-tip"), &hidden(&["feature"]));

        assert!(!visible.contains("feature-tip"));
        assert!(visible.contains("main-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn shared_ancestry_survives_hiding_a_branch() {
        // feature points at root, which master also reaches: root stays.
        let commits = vec![
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "root"),
            local_branch("master", "main-tip"),
        ];

        let visible =
            visible_commit_shas(&commits, &branches, Some("main-tip"), &hidden(&["feature"]));

        assert!(visible.contains("root"));
        assert!(visible.contains("main-tip"));
    }

    #[test]
    fn head_chain_is_visible_even_with_no_visible_branches() {
        let commits = vec![
            commit_info("head-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![local_branch("feature", "head-tip")];

        let visible =
            visible_commit_shas(&commits, &branches, Some("head-tip"), &hidden(&["feature"]));

        assert!(visible.contains("head-tip"));
        assert!(visible.contains("root"));
    }

    #[test]
    fn empty_hidden_set_keeps_every_loaded_commit() {
        let commits = vec![
            commit_info("feature-tip", &["root"]),
            commit_info("main-tip", &["root"]),
            commit_info("root", &[]),
        ];
        let branches = vec![
            local_branch("feature", "feature-tip"),
            local_branch("master", "main-tip"),
        ];

        let visible =
            visible_commit_shas(&commits, &branches, Some("main-tip"), &BTreeSet::new());

        assert_eq!(visible.len(), commits.len());
    }

    #[test]
    fn parents_beyond_the_loaded_boundary_are_ignored() {
        // root's parent is not loaded; the walk must terminate, not panic.
        let commits = vec![commit_info("root", &["unloaded-parent"])];
        let branches = vec![local_branch("master", "root")];

        let visible = visible_commit_shas(&commits, &branches, Some("root"), &BTreeSet::new());

        assert!(visible.contains("root"));
        assert!(!visible.contains("unloaded-parent"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib hiding_a_branch_removes_its_exclusive_commits`
Expected: FAIL to compile — `visible_commit_shas` not found.

- [ ] **Step 3: Implement the function and the state**

Add `HashSet` to the std imports at the top of `src/app.rs`:

```rust
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};
```

Add the free function near `render_commit_ref_labels` (after `render_commit_row`'s `impl` block ends, ~line 2390):

```rust
/// The set of loaded commits reachable from HEAD or from any branch whose
/// name is not in `hidden_branches`. Parents beyond the loaded page simply
/// terminate the walk: a commit that is not loaded cannot be rendered anyway,
/// and paging in more history re-runs this computation over the larger list.
fn visible_commit_shas(
    commits: &[repo::CommitInfo],
    local_branches: &[repo::LocalBranch],
    head_sha: Option<&str>,
    hidden_branches: &BTreeSet<String>,
) -> HashSet<String> {
    let commits_by_sha: HashMap<&str, &repo::CommitInfo> = commits
        .iter()
        .map(|commit| (commit.sha.as_str(), commit))
        .collect();

    let mut worklist: Vec<&str> = Vec::new();
    worklist.extend(head_sha);
    worklist.extend(
        local_branches
            .iter()
            .filter(|branch| !hidden_branches.contains(&branch.name))
            .map(|branch| branch.tip_sha.as_str()),
    );

    let mut visible = HashSet::new();
    while let Some(sha) = worklist.pop() {
        let Some(commit) = commits_by_sha.get(sha) else {
            continue;
        };
        if !visible.insert(commit.sha.clone()) {
            continue;
        }
        worklist.extend(commit.parent_shas.iter().map(String::as_str));
    }
    visible
}
```

Add the field to the `App` struct, after `branch_sidebar_hovered: bool` (~line 109):

```rust
    /// Branch names the user has toggled off in the sidebar. Session-only:
    /// cleared whenever a repository is opened. The checked-out branch is
    /// never in this set (its row renders no toggle).
    hidden_branches: BTreeSet<String>,
```

Initialize it in `new_with_picker_settings_and_store_path` (~line 391, next to `branch_sidebar_hovered: false`):

```rust
            hidden_branches: BTreeSet::new(),
```

Reset it in `apply_open_repository` (~line 491, next to `self.branch_sidebar_hovered = false;`):

```rust
        self.hidden_branches.clear();
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib visible -- --nocapture` then the named tests:
`cargo test --lib hiding_a_branch_removes_its_exclusive_commits shared_ancestry_survives_hiding_a_branch head_chain_is_visible_even_with_no_visible_branches empty_hidden_set_keeps_every_loaded_commit parents_beyond_the_loaded_boundary_are_ignored`
Expected: PASS (5 tests). Note: until Task 3 uses the field, `hidden_branches` may trigger a dead-code warning under clippy — if so, proceed to Task 3 before running `bin/check`; do not add `#[allow]`.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(graph): compute visible commits from branch visibility state"
```

---

### Task 3: Toggle action with selection clearing

The state mutation path: `toggle_branch_visibility` flips a branch, and hiding clears the selection if any selected commit became invisible. Also adds the multi-branch test fixture later tasks reuse.

**Files:**
- Modify: `src/app.rs` (methods near `select_single_commit` ~line 530, test fixtures ~line 5100, tests module)

- [ ] **Step 1: Add the test fixture**

In the tests module, after `init_repo_with_feature_branch` (~line 5119), add:

```rust
    /// Two commits on master plus a `feature` branch carrying one exclusive
    /// commit branched from the root. Returns (dir, master_tip_sha,
    /// feature_tip_sha). HEAD stays on master.
    fn init_repo_with_unmerged_branch_commit() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let root_oid = commit_all(&repo, "Root", &[]);

        fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
        let main_tip = commit_all(&repo, "Main tip", &[root_oid]);

        let root_commit = repo.find_commit(root_oid).expect("find root commit");
        repo.branch("feature", &root_commit, false)
            .expect("create feature branch");
        repo.set_head("refs/heads/feature")
            .expect("point HEAD at feature");
        fs::write(dir.path().join("feature.txt"), "feature\n").expect("write feature file");
        let feature_tip = commit_all(&repo, "Feature work", &[root_oid]);
        repo.set_head("refs/heads/master")
            .expect("point HEAD back at master");

        drop(root_commit);
        drop(repo);
        (dir, main_tip.to_string(), feature_tip.to_string())
    }
```

- [ ] **Step 2: Write failing state tests**

In the tests module, near `selecting_commits_toggles_single_selection` (~line 6268):

```rust
    #[gpui::test]
    async fn hiding_a_branch_clears_a_selection_it_made_invisible(cx: &mut TestAppContext) {
        let (dir, _main_tip, feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(feature_tip.clone(), cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
                assert_eq!(app.selection, Selection::None);
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn hiding_a_branch_keeps_a_still_visible_selection(cx: &mut TestAppContext) {
        let (dir, main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(main_tip.clone(), cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
                assert_eq!(app.selection, Selection::Single { sha: main_tip.clone() });
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn toggling_a_branch_twice_restores_visibility(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
                assert!(app.hidden_branches.contains("feature"));
                app.toggle_branch_visibility("feature".to_string(), cx);
                assert!(app.hidden_branches.is_empty());
            })
            .expect("update window");
    }

    #[gpui::test]
    async fn opening_a_repository_resets_hidden_branches(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
                assert!(!app.hidden_branches.is_empty());
                app.open_repository_at(path, window, cx);
                assert!(app.hidden_branches.is_empty());
            })
            .expect("update window");
    }
```

Note: if `open_repository_at`'s real signature in the existing tests differs (some call sites may not pass `window`), match whatever the neighboring tests in this file do.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib hiding_a_branch_clears_a_selection_it_made_invisible`
Expected: FAIL to compile — `toggle_branch_visibility` not found.

- [ ] **Step 4: Implement the toggle**

In `impl App`, after `select_single_commit` (~line 536):

```rust
    /// Flip a branch's graph visibility. Hiding may make the selected
    /// commit(s) invisible, in which case the selection is cleared. The HEAD
    /// branch never reaches this path: its sidebar row renders no toggle.
    pub(crate) fn toggle_branch_visibility(&mut self, name: String, cx: &mut Context<Self>) {
        if self.hidden_branches.remove(&name) {
            cx.notify();
            return;
        }
        self.hidden_branches.insert(name);
        self.clear_selection_if_hidden();
        cx.notify();
    }

    fn clear_selection_if_hidden(&mut self) {
        let Mode::RepoOpen { repo } = &self.mode else {
            return;
        };
        let head_sha = repo
            .commits
            .iter()
            .find(|commit| commit.is_head)
            .map(|commit| commit.sha.as_str());
        let visible = visible_commit_shas(
            &repo.commits,
            &repo.local_branches,
            head_sha,
            &self.hidden_branches,
        );
        let selection_hidden = match &self.selection {
            Selection::None => false,
            Selection::Single { sha } => !visible.contains(sha),
            Selection::Range { shas, .. } => shas.iter().any(|sha| !visible.contains(sha)),
        };
        if selection_hidden {
            self.selection = Selection::None;
        }
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib hiding_a_branch_clears_a_selection_it_made_invisible hiding_a_branch_keeps_a_still_visible_selection toggling_a_branch_twice_restores_visibility opening_a_repository_resets_hidden_branches`
Expected: PASS (4 tests)

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(graph): toggle branch visibility with selection clearing"
```

---

### Task 4: Filter the graph and ref labels by visibility

Hidden-exclusive commits stop rendering and lanes re-flow; hidden branch names drop out of ref labels; `focus_branch` indexes into the *visible* list so scroll targeting stays correct when hidden rows are removed above the target.

**Files:**
- Modify: `src/app.rs` (`render_graph_screen` ~line 1282, `focus_branch` ~line 542, `render_commit_row` ~line 2386, `render_commit_ref_labels` ~line 2408, tests module)

- [ ] **Step 1: Write failing view tests**

In the tests module, near `clicking_a_branch_selects_and_reveals_its_tip_commit` (~line 5809). State is arranged through `window.update` first, then a `VisualTestContext` makes the assertions, matching the existing sidebar tests:

```rust
    #[gpui::test]
    async fn hiding_a_branch_removes_its_exclusive_commits_and_label(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        // Three commits loaded, one (the feature-exclusive commit) hidden.
        assert!(visual.debug_bounds("commit-row-1").is_some());
        assert!(
            visual.debug_bounds("commit-row-2").is_none(),
            "the feature-exclusive commit must not render"
        );
        // The feature ref label is gone from every remaining row.
        for row in 0..2 {
            assert!(
                visual
                    .debug_bounds(&format!("commit-ref-label-{row}-feature"))
                    .is_none(),
                "hidden branch label must not render on row {row}"
            );
        }
    }

    #[gpui::test]
    async fn showing_a_branch_again_restores_its_commits_and_label(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("hide and re-show feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(visual.debug_bounds("commit-row-2").is_some());
        let feature_label_rows = (0..3)
            .filter(|row| {
                visual
                    .debug_bounds(&format!("commit-ref-label-{row}-feature"))
                    .is_some()
            })
            .count();
        assert_eq!(feature_label_rows, 1, "the feature label renders on its tip");
    }

    #[gpui::test]
    async fn focusing_a_branch_targets_its_visible_row_index(cx: &mut TestAppContext) {
        // Hiding `feature` removes a row, shifting indices of the rows below.
        // Focusing master must select the row at its *visible* index.
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let master_row = visual
            .debug_bounds("branch-row-master")
            .expect("master branch row renders");
        visual.simulate_click(master_row.center(), Modifiers::none());

        // With feature hidden the visible order is: master tip (0), root (1).
        visual
            .debug_bounds("selected-commit-row-0")
            .expect("master tip is the selected visible row");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib hiding_a_branch_removes_its_exclusive_commits_and_label`
Expected: FAIL — `commit-row-2` still renders (assertion failure, not a compile error).

- [ ] **Step 3: Implement the filtering**

Add a free function next to `visible_commit_shas`:

```rust
/// The loaded commits that survive branch-visibility filtering, in history
/// order. Render and focus paths must both use this so row indices agree.
fn visible_commits<'a>(
    repo: &'a repo::OpenRepository,
    hidden_branches: &BTreeSet<String>,
) -> Vec<&'a repo::CommitInfo> {
    let head_sha = repo
        .commits
        .iter()
        .find(|commit| commit.is_head)
        .map(|commit| commit.sha.as_str());
    let visible = visible_commit_shas(
        &repo.commits,
        &repo.local_branches,
        head_sha,
        hidden_branches,
    );
    repo.commits
        .iter()
        .filter(|commit| visible.contains(&commit.sha))
        .collect()
}
```

In `render_graph_screen` (~line 1304), replace the `graph_commits` / `head_sha` / `commit_rows` construction so everything derives from the filtered list. The `repo.commits.is_empty()` empty-state check above it stays as-is (HEAD is always visible, so a non-empty repo never filters to zero rows):

```rust
            let visible_commits = visible_commits(repo, &self.hidden_branches);
            let graph_commits = visible_commits
                .iter()
                .map(|commit| graph::GraphCommit {
                    sha: commit.sha.clone(),
                    authored_timestamp: commit.authored_timestamp,
                    parent_shas: commit.parent_shas.clone(),
                })
                .collect::<Vec<_>>();
            let head_sha = visible_commits
                .iter()
                .find(|commit| commit.is_head)
                .map(|commit| commit.sha.as_str());
            let graph_rows = graph::layout_graph_anchored(&graph_commits, head_sha);
            let max_graph_lanes = graph_rows
                .iter()
                .map(|row| row.lane_count)
                .max()
                .unwrap_or(1);

            let commit_rows = visible_commits
                .iter()
                .zip(graph_rows.iter())
                .enumerate()
                .map(|(index, (commit, graph_row))| {
                    self.render_commit_row(
                        index,
                        commit,
                        graph_row,
                        max_graph_lanes,
                        self.is_commit_selected(&commit.sha),
                        cx,
                    )
                })
                .collect::<Vec<_>>();
```

In `focus_branch` (~line 543), make the loop index into the visible list (paging checks still use the full loaded count):

```rust
        let (commit_index, commit_count) = loop {
            let (tip_index, can_load_more, loaded_count, visible_count) = match &self.mode {
                Mode::RepoOpen { repo } => {
                    let visible = visible_commits(repo, &self.hidden_branches);
                    (
                        visible.iter().position(|commit| commit.sha == tip_sha),
                        repo.has_more_commits,
                        repo.commits.len(),
                        visible.len(),
                    )
                }
                Mode::NoRepo => return,
            };
            if let Some(index) = tip_index {
                break (index, visible_count);
            }
            if !can_load_more {
                return;
            }

            self.load_older_commits(window, cx);

            let loaded_count_after = match &self.mode {
                Mode::RepoOpen { repo } => repo.commits.len(),
                Mode::NoRepo => return,
            };
            if loaded_count_after == loaded_count {
                // Paging failed; the error is already on the notification
                // list, so stop rather than loop forever.
                return;
            }
        };
```

In `render_commit_ref_labels` (~line 2408), take and apply the hidden set:

```rust
fn render_commit_ref_labels(
    row_index: usize,
    commit: &repo::CommitInfo,
    hidden_branches: &BTreeSet<String>,
) -> gpui::Div {
    let mut labels = Vec::new();
    if commit.is_head {
        labels.push(CommitRefLabel {
            name: "HEAD".to_string(),
            kind: CommitRefLabelKind::Head,
        });
    }
    labels.extend(
        commit
            .branch_names
            .iter()
            .filter(|name| !hidden_branches.contains(*name))
            .cloned()
            .map(|name| CommitRefLabel {
                name,
                kind: CommitRefLabelKind::Branch,
            }),
    );
    // ... rest of the function unchanged
```

Update its one call site in `render_commit_row` (~line 2386):

```rust
            .child(render_commit_ref_labels(index, commit, &self.hidden_branches))
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib hiding_a_branch_removes_its_exclusive_commits_and_label showing_a_branch_again_restores_its_commits_and_label focusing_a_branch_targets_its_visible_row_index`
Expected: PASS (3 tests). Also run `cargo test --lib` to confirm no existing graph/sidebar test regressed.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(graph): hide branch-exclusive commits and labels when toggled off"
```

---

### Task 5: Sidebar eye toggle UI

The hover-revealed eye icon on non-HEAD rows, the always-visible eye-off on hidden rows, muted hidden-row text, and the no-focus rule for hidden rows.

**Files:**
- Modify: `src/app.rs` (`App` struct ~line 109, constructor ~line 391, `apply_open_repository` ~line 491, `render_branch_row` ~line 1495, tests module)

- [ ] **Step 1: Write failing view tests**

```rust
    #[gpui::test]
    async fn clicking_the_eye_icon_hides_the_branch(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // The toggle is hover-revealed; tests drive the hover state
                // directly. Branches sort alphabetically: feature is row 0.
                app.hovered_branch_row = Some(0);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-visibility-feature")
            .expect("hovered branch row reveals its visibility toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        assert!(
            visual.debug_bounds("commit-row-2").is_none(),
            "feature-exclusive commit must disappear after the icon click"
        );
        assert!(
            visual.debug_bounds("selected-commit-row-0").is_none()
                && visual.debug_bounds("selected-commit-row-1").is_none(),
            "the icon click must not focus the branch"
        );
    }

    #[gpui::test]
    async fn hidden_branch_shows_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-visibility-feature")
            .expect("hidden branch keeps its toggle visible without hover");
    }

    #[gpui::test]
    async fn head_branch_row_renders_no_visibility_toggle(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // master is the HEAD branch; alphabetically it is row 1.
                app.hovered_branch_row = Some(1);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("branch-visibility-master").is_none(),
            "the HEAD branch must not offer a visibility toggle"
        );
    }

    #[gpui::test]
    async fn clicking_a_hidden_branch_row_does_not_focus_it(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("branch-row-feature")
            .expect("hidden branch row still renders");
        visual.simulate_click(row.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert_eq!(app.selection, Selection::None);
            })
            .expect("read selection");
    }
```

Note on the hidden-row click test: the row center may overlap the always-visible eye-off icon. If the click lands on the icon (which would *show* the branch rather than no-op), click near the row's left edge instead: `visual.simulate_click(point(row.origin.x + px(8.), row.center().y), Modifiers::none())`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib clicking_the_eye_icon_hides_the_branch`
Expected: FAIL to compile — `hovered_branch_row` not found.

- [ ] **Step 3: Implement the sidebar UI**

Add the hover field to `App` after `hidden_branches`:

```rust
    /// Sidebar row index the cursor is currently over, if any. Gates the
    /// hover-revealed visibility toggle on visible branches.
    hovered_branch_row: Option<usize>,
```

Initialize in the constructor (`hovered_branch_row: None,`) and reset in `apply_open_repository` (`self.hovered_branch_row = None;`).

Rewrite `render_branch_row` (~line 1495):

```rust
    fn render_branch_row(
        &self,
        index: usize,
        branch: &repo::LocalBranch,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = matches!(
            &self.selection,
            Selection::Single { sha } if sha == &branch.tip_sha
        );
        let hidden = self.hidden_branches.contains(&branch.name);
        let show_toggle = !branch.is_head && (hidden || self.hovered_branch_row == Some(index));
        let row_bg = if selected {
            rgb(0x223248)
        } else {
            rgb(0x171717)
        };
        let name_color = if hidden {
            rgb(0x999999)
        } else if branch.is_head {
            rgb(0xa3e635)
        } else {
            rgb(0xe6e6e6)
        };
        let name_fragment = debug_ref_label_fragment(&branch.name);
        let row_selector = if selected {
            format!("selected-branch-row-{name_fragment}")
        } else {
            format!("branch-row-{name_fragment}")
        };
        let marker_selector = format!("branch-head-marker-{name_fragment}");
        let toggle_selector = format!("branch-visibility-{name_fragment}");
        let tip_sha = branch.tip_sha.clone();
        let toggle_branch_name = branch.name.clone();

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_3()
            .bg(row_bg)
            .id(("branch-row", index))
            .debug_selector(move || row_selector.clone())
            .when(!selected, |row| row.hover(|style| style.bg(rgb(0x1f2733))))
            .on_hover(cx.listener(move |app, hovered: &bool, _window, cx| {
                if *hovered {
                    if app.hovered_branch_row != Some(index) {
                        app.hovered_branch_row = Some(index);
                        cx.notify();
                    }
                } else if app.hovered_branch_row == Some(index) {
                    app.hovered_branch_row = None;
                    cx.notify();
                }
            }))
            .when(!hidden, |row| {
                row.cursor_pointer()
                    .on_click(cx.listener(move |app, _event: &ClickEvent, window, cx| {
                        app.focus_branch(tip_sha.clone(), window, cx);
                    }))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(name_color)
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child(branch.name.clone()),
            )
            .when(branch.is_head, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .debug_selector(move || marker_selector.clone())
                        .child(
                            Icon::new(LucideIcon::Check)
                                .text_color(rgb(0xa3e635))
                                .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            })
            .when(show_toggle, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .id(("branch-visibility", index))
                        .debug_selector(move || toggle_selector.clone())
                        .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                            app.toggle_branch_visibility(toggle_branch_name.clone(), cx);
                        }))
                        .child(
                            Icon::new(if hidden {
                                LucideIcon::EyeOff
                            } else {
                                LucideIcon::Eye
                            })
                            .text_color(rgb(0x999999))
                            .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            })
    }
```

Implementation notes:
- `cx.stop_propagation()` keeps the icon click from also firing the row's `focus_branch` handler. If the borrow checker objects to `cx` usage order inside the listener, call `cx.stop_propagation()` first, as shown.
- The row's `on_click`/`cursor_pointer` moved inside `.when(!hidden, ...)` — that is the "hidden row does not focus" rule.
- The eye icon sits after the HEAD marker slot; HEAD rows never render it (`show_toggle` requires `!branch.is_head`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib clicking_the_eye_icon_hides_the_branch hidden_branch_shows_its_toggle_without_hover head_branch_row_renders_no_visibility_toggle clicking_a_hidden_branch_row_does_not_focus_it`
Expected: PASS (4 tests). Then `cargo test --lib` for the full suite — in particular `clicking_a_branch_selects_and_reveals_its_tip_commit` must still pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(ui): branch visibility eye toggle in the graph sidebar"
```

---

### Task 6: Spec update and full verification

Behavior changed, so `docs/specs/review/workflow.md` must change in the same body of work (per CLAUDE.md). Read `docs/specs/README.md` for spec voice before editing.

**Files:**
- Modify: `docs/specs/review/workflow.md` (the "Navigating branches from the sidebar" section, ~line 109)

- [ ] **Step 1: Add the spec section**

After the "Navigating branches from the sidebar" section's observable outcomes, add a new section (adjust wording to match the file's established spec voice after reading `docs/specs/README.md`):

```markdown
## Hiding branches from the graph

Every branch except the checked-out branch can be toggled off from the
sidebar. A hidden branch's name no longer appears as a ref label in the
graph, and commits reachable only from hidden branches are removed: the graph
re-flows as if those commits did not exist. Commits a hidden branch shares
with any visible branch (or with the checked-out branch) remain.

The toggle control on a visible branch is revealed when the pointer is over
its row; a hidden branch's control is always visible and its name renders
muted. Activating a hidden branch's row does not focus it; the branch must be
shown again first. The checked-out branch offers no toggle. If hiding a
branch removes the selected commit — or any commit in a selected range — the
selection clears. Visibility choices are not persisted: opening a repository
shows every branch.

**Observable outcomes**

- A non-checked-out branch row reveals a visibility toggle on hover; the
  checked-out branch row never shows one.
- Toggling a branch off removes its ref labels and its exclusive commits from
  the graph; toggling it back on restores them.
- A hidden branch's row is muted, keeps its toggle visible, and does not
  focus its tip when activated.
- Hiding a branch that makes the current selection invisible clears the
  selection.
- Reopening a repository resets all branches to visible.
```

- [ ] **Step 2: Run the full verification command**

Run: `bin/check`
Expected: PASS — `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` all clean. Fix any findings (formatting drift, clippy lints in new code) before proceeding. No `#[allow]` without explicit user approval.

- [ ] **Step 3: Commit**

```bash
git add docs/specs/review/workflow.md
git commit -m "docs(specs): cover branch visibility toggles in the review workflow spec"
```

---

## Execution notes

- `docs/superpowers/` is gitignored; this plan file itself was committed with `git add -f` per the repo's tracked-plans convention. Source and spec changes under `src/` and `docs/specs/` stage normally.
- Commit ordering note: the fixture commits in `init_repo_with_unmerged_branch_commit` may share a timestamp (`Signature::now` within one second). The view tests avoid asserting on the relative order of the two branch tips; they only rely on root rendering last and on visible-row counts. Keep new assertions order-independent in the same way.
- If any existing test asserts on `render_commit_ref_labels`'s old two-argument signature or on commit row indices in graph mode, update it to the new call shape / visible indices rather than weakening the new behavior.
