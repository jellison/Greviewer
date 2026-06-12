# Nested Branch Folders Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Nest slash-named branches (`features/some-feature`) under collapsible folders in the graph branch sidebar, with a per-folder visibility toggle that hides/shows every branch beneath it.

**Architecture:** Mirror the file-tree pattern already in `src/app.rs`: a pure builder groups branches by `/`-separated name segments into a `BTreeMap`-backed tree and flattens it into depth-tagged `BranchTreeRow`s; a session-only `BTreeSet<String>` on `App` stores collapsed folder paths; folder visibility derives from descendant membership in the existing `hidden_branches` set. All state keeps keying on full branch names — only display changes. Spec: `docs/superpowers/specs/2026-06-12-nested-branch-folders-design.md`.

**Tech Stack:** Rust, gpui, gpui-component, git2 (tests only). Single-crate layout per ADR-0002; tests per ADR-0003 (unit + `#[gpui::test]` view tests, all inside `src/app.rs`'s `mod tests`).

**Verification:** `bin/check` (fmt + clippy `-D warnings` + all tests) must pass at the end of every task, before its commit. Run `cargo fmt` before `bin/check` — some plan snippets exceed the line width and rustfmt will rewrap them. Each task leaves no dead code: new items are wired into the render path in the same task they are introduced.

**Conventions you must know:**
- `local_branches` arrives from the repo layer already sorted alphabetically by full name.
- Debug selectors come from `debug_ref_label_fragment` (`src/app.rs:5165`), which lowercases and replaces every non-alphanumeric char with `-`; `features/alpha` → `features-alpha`.
- Existing branch-row selectors (`branch-row-{name}`, `branch-visibility-{name}`) must NOT change — existing view tests depend on them.
- Existing tests set `app.hovered_branch_row = Some(index)` directly to simulate hover; rows with no `/` in their names keep their flat indices, so existing tests stay valid.

---

### Task 1: Vendor Lucide chevron icons

Folder rows need a chevron: right when collapsed, down when expanded. Neither is vendored yet.

**Files:**
- Create: `assets/icons/chevron-right.svg`
- Create: `assets/icons/chevron-down.svg`
- Modify: `src/icons.rs`

- [ ] **Step 1: Write the two SVG assets**

Create `assets/icons/chevron-right.svg` (Lucide chevron-right, reformatted to match the existing vendored style — compare `assets/icons/eye.svg`):

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
  <path d="m9 18 6-6-6-6" />
</svg>
```

Create `assets/icons/chevron-down.svg`:

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
  <path d="m6 9 6 6 6-6" />
</svg>
```

- [ ] **Step 2: Add the enum variants in `src/icons.rs`**

The file's doc comment says: add a variant, map it in `path`, add it to `ALL`. Variants are alphabetical; insert after `Check`:

In the `LucideIcon` enum:

```rust
    /// `chevron-down.svg`
    ChevronDown,
    /// `chevron-right.svg`
    ChevronRight,
```

In `ALL` (after `LucideIcon::Check,`):

```rust
        LucideIcon::ChevronDown,
        LucideIcon::ChevronRight,
```

In the `path` match (after the `Check` arm):

```rust
            LucideIcon::ChevronDown => "icons/chevron-down.svg",
            LucideIcon::ChevronRight => "icons/chevron-right.svg",
```

- [ ] **Step 3: Run the icon asset test**

Run: `cargo test every_variant_resolves_to_a_vendored_asset`
Expected: PASS (this existing test loads every `ALL` variant's asset, so it covers the new icons automatically).

- [ ] **Step 4: Run `bin/check`**

Expected: clean. The new variants are constructed in `ALL`, so no dead-code warning.

- [ ] **Step 5: Commit**

```bash
git add assets/icons/chevron-right.svg assets/icons/chevron-down.svg src/icons.rs
git commit -m "feat(ui): vendor lucide chevron-right and chevron-down icons"
```

---

### Task 2: Branch tree model, collapse state, and nested sidebar rendering

One cohesive change: the row model + builder (unit-tested), the collapse state on `App`, and the sidebar rewired to render the tree. Doing these together keeps `bin/check` green — the builder is consumed by the render path in the same commit.

**Files:**
- Modify: `src/app.rs` — types near `FileTreeLeaf` (~line 252), `App` fields (~line 113) and `App::new` (~line 399), repo-open reset (~line 501), methods near `toggle_branch_visibility` (~line 560), builder functions after `append_file_tree_rows` (~line 5104), `render_branch_sidebar` (~line 1475), `render_branch_row` (~line 1549), new `render_branch_folder_row`, tests in `mod tests`.

- [ ] **Step 1: Write the failing unit tests for the builder**

In `mod tests`, next to the existing `visible_commit_shas` tests (~line 5293). The `local_branch` helper already exists at `src/app.rs:5281`.

```rust
    #[test]
    fn flat_branch_names_produce_flat_rows() {
        let branches = vec![local_branch("feature", "f"), local_branch("master", "m")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("feature", "f"),
                    display_name: "feature".to_string(),
                    depth: 0,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("master", "m"),
                    display_name: "master".to_string(),
                    depth: 0,
                }),
            ]
        );
    }

    #[test]
    fn slash_named_branch_nests_under_a_folder_even_when_alone() {
        let branches = vec![local_branch("features/some-feature", "tip")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "features".to_string(),
                    depth: 0,
                    collapsed: false,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("features/some-feature", "tip"),
                    display_name: "some-feature".to_string(),
                    depth: 1,
                }),
            ]
        );
    }

    #[test]
    fn multi_level_names_nest_one_folder_per_segment() {
        let branches = vec![local_branch("team/alice/feature-x", "tip")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "team".to_string(),
                    path: "team".to_string(),
                    depth: 0,
                    collapsed: false,
                }),
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "alice".to_string(),
                    path: "team/alice".to_string(),
                    depth: 1,
                    collapsed: false,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("team/alice/feature-x", "tip"),
                    display_name: "feature-x".to_string(),
                    depth: 2,
                }),
            ]
        );
    }

    #[test]
    fn folders_sort_before_branches_at_each_level() {
        // Input order is alphabetical by full name, as the repo layer
        // provides it: alpha, features/x, zeta.
        let branches = vec![
            local_branch("alpha", "a"),
            local_branch("features/x", "x"),
            local_branch("zeta", "z"),
        ];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &BTreeSet::new());

        let order = rows
            .iter()
            .map(|row| match row {
                BranchTreeRow::Folder(folder) => format!("folder:{}", folder.path),
                BranchTreeRow::Branch(branch_row) => {
                    format!("branch:{}", branch_row.branch.name)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            vec!["folder:features", "branch:features/x", "branch:alpha", "branch:zeta"]
        );
    }

    #[test]
    fn collapsed_folder_emits_no_descendant_rows() {
        let branches = vec![
            local_branch("features/inner/deep", "d"),
            local_branch("features/x", "x"),
            local_branch("master", "m"),
        ];
        let collapsed = ["features"]
            .iter()
            .map(|path| path.to_string())
            .collect::<BTreeSet<_>>();

        let rows = build_branch_tree_rows(&branches, &collapsed, &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "features".to_string(),
                    depth: 0,
                    collapsed: true,
                }),
                BranchTreeRow::Branch(BranchRow {
                    branch: local_branch("master", "m"),
                    display_name: "master".to_string(),
                    depth: 0,
                }),
            ]
        );
    }
```

Add `build_branch_tree_rows, BranchFolderRow, BranchRow, BranchTreeRow` to the `use super::{...}` list at the top of `mod tests` (~line 5186), keeping it alphabetical.

(The `hidden_branches` builder parameter exists from the start so the signature doesn't churn in Task 3, where folder visibility is derived from it. Until then the builder ignores it; passing `&BTreeSet::new()` everywhere is correct.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test branch_tree 2>&1 | tail -20` — expected: compile error, `build_branch_tree_rows` not found.

- [ ] **Step 3: Add the row types**

After `FileTreeLeaf` (~line 252):

```rust
/// Render model for the graph branch sidebar: branches grouped under
/// collapsible folders derived from `/`-separated name segments. Branch rows
/// carry the full `LocalBranch` — selection, hiding, and debug selectors all
/// key on the full name; only `display_name` is shortened.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BranchTreeRow {
    Folder(BranchFolderRow),
    Branch(BranchRow),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchFolderRow {
    /// Final path segment, e.g. "alice" for `team/alice`.
    name: String,
    /// Full prefix path, e.g. "team/alice". Keys collapse state.
    path: String,
    depth: usize,
    collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchRow {
    branch: repo::LocalBranch,
    /// Final name segment, e.g. "some-feature" for `features/some-feature`.
    display_name: String,
    depth: usize,
}
```

- [ ] **Step 4: Add the builder functions**

After `append_file_tree_rows` (~line 5104):

```rust
/// Tree node used while grouping branches by slash-separated name segments.
/// The BTreeMap keeps sibling folders alphabetical; branches within a node
/// stay in input order, which is alphabetical because they share a prefix
/// and `local_branches` arrives sorted by full name.
#[derive(Debug, Default)]
struct BranchTreeNode {
    folders: BTreeMap<String, BranchTreeNode>,
    branches: Vec<repo::LocalBranch>,
}

/// Group branches into folders by `/`-separated name segments and flatten
/// the tree into depth-tagged sidebar rows. Folders list before branches at
/// each level. A collapsed folder contributes its own row and skips every
/// descendant. Git rejects ref names with empty segments, so segments are
/// always non-empty.
fn build_branch_tree_rows(
    local_branches: &[repo::LocalBranch],
    collapsed_folders: &BTreeSet<String>,
    _hidden_branches: &BTreeSet<String>,
) -> Vec<BranchTreeRow> {
    let mut root = BranchTreeNode::default();
    for branch in local_branches {
        let mut segments = branch.name.split('/').collect::<Vec<_>>();
        segments.pop();
        let mut node = &mut root;
        for segment in segments {
            node = node.folders.entry(segment.to_string()).or_default();
        }
        node.branches.push(branch.clone());
    }

    let mut rows = Vec::new();
    append_branch_tree_rows(&root, 0, "", collapsed_folders, &mut rows);
    rows
}

fn append_branch_tree_rows(
    node: &BranchTreeNode,
    depth: usize,
    prefix: &str,
    collapsed_folders: &BTreeSet<String>,
    rows: &mut Vec<BranchTreeRow>,
) {
    for (name, child) in &node.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let collapsed = collapsed_folders.contains(&path);
        rows.push(BranchTreeRow::Folder(BranchFolderRow {
            name: name.clone(),
            path: path.clone(),
            depth,
            collapsed,
        }));
        if !collapsed {
            append_branch_tree_rows(child, depth + 1, &path, collapsed_folders, rows);
        }
    }

    for branch in &node.branches {
        let display_name = branch
            .name
            .rsplit('/')
            .next()
            .unwrap_or(&branch.name)
            .to_string();
        rows.push(BranchTreeRow::Branch(BranchRow {
            branch: branch.clone(),
            display_name,
            depth,
        }));
    }
}
```

Note the `_hidden_branches` underscore: it silences unused-parameter warnings until Task 3 starts reading it.

- [ ] **Step 5: Run the unit tests**

Run: `cargo test branch_tree` and `cargo test folders_sort` and `cargo test slash_named` and `cargo test multi_level` and `cargo test collapsed_folder` (or just `cargo test` for the lot).
Expected: the five new tests PASS.

- [ ] **Step 6: Add collapse state to `App`**

Field, after `hidden_branches` (~line 113):

```rust
    /// Sidebar folder paths the user has collapsed. Session-only: cleared
    /// whenever a repository is opened. Folders default to expanded, so an
    /// empty set means every folder shows its contents.
    collapsed_branch_folders: BTreeSet<String>,
```

Initialization in `App::new`, next to `hidden_branches: BTreeSet::new(),` (~line 399):

```rust
            collapsed_branch_folders: BTreeSet::new(),
```

Reset on repo open, immediately after `self.hidden_branches.clear();` (~line 501):

```rust
        self.collapsed_branch_folders.clear();
```

Toggle method, after `toggle_branch_visibility` (~line 560):

```rust
    /// Collapse or expand a sidebar branch folder. Purely visual: removes the
    /// folder's descendant rows from the sidebar without touching graph
    /// visibility.
    pub(crate) fn toggle_branch_folder(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.collapsed_branch_folders.insert(path.clone()) {
            self.collapsed_branch_folders.remove(&path);
        }
        cx.notify();
    }
```

- [ ] **Step 7: Rewire `render_branch_sidebar` to the tree rows**

In `render_branch_sidebar` (~line 1493), replace:

```rust
            let rows = repo
                .local_branches
                .iter()
                .enumerate()
                .map(|(index, branch)| self.render_branch_row(index, branch, cx))
                .collect::<Vec<_>>();
```

with:

```rust
            let rows = build_branch_tree_rows(
                &repo.local_branches,
                &self.collapsed_branch_folders,
                &self.hidden_branches,
            );
            let rows = rows
                .iter()
                .enumerate()
                .map(|(index, row)| match row {
                    BranchTreeRow::Folder(folder) => self
                        .render_branch_folder_row(index, folder, cx)
                        .into_any_element(),
                    BranchTreeRow::Branch(branch_row) => self
                        .render_branch_row(index, branch_row, cx)
                        .into_any_element(),
                })
                .collect::<Vec<_>>();
```

- [ ] **Step 8: Update `render_branch_row` for display name and depth**

Change the signature (~line 1549) from:

```rust
    fn render_branch_row(
        &self,
        index: usize,
        branch: &repo::LocalBranch,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
```

to:

```rust
    fn render_branch_row(
        &self,
        index: usize,
        row: &BranchRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let branch = &row.branch;
```

The body then compiles unchanged except for two edits:

1. Indent spacer — insert immediately before the existing first `.child(` call (the branch-name div), so the spacer is the row's first child:

```rust
            .when(row.depth > 0, |el| {
                el.child(
                    div()
                        .flex_none()
                        .w(px(row.depth as f32 * FILE_TREE_INDENT_WIDTH)),
                )
            })
```

2. Display name — in the branch-name child, replace `.child(branch.name.clone())` with:

```rust
                    .child(row.display_name.clone()),
```

Everything else (selectors from `branch.name`, `tip_sha`, hover, toggle) stays exactly as is.

- [ ] **Step 9: Add `render_branch_folder_row`**

After `render_branch_row` (~line 1662). Folder name color matches the file tree's folder color `0x8aa6bd`; row metrics match branch rows. No visibility toggle yet — Task 3 adds it.

```rust
    fn render_branch_folder_row(
        &self,
        index: usize,
        folder: &BranchFolderRow,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path_fragment = debug_ref_label_fragment(&folder.path);
        let row_selector = format!("branch-folder-{path_fragment}");
        let collapse_path = folder.path.clone();
        let depth = folder.depth;

        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(FILE_TREE_ROW_HEIGHT))
            .gap_2()
            .px_3()
            .bg(rgb(0x171717))
            .cursor_pointer()
            .id(("branch-folder", index))
            .debug_selector(move || row_selector.clone())
            .hover(|style| style.bg(rgb(0x1f2733)))
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
            .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                app.toggle_branch_folder(collapse_path.clone(), cx);
            }))
            .when(depth > 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(depth as f32 * FILE_TREE_INDENT_WIDTH)),
                )
            })
            .child(
                Icon::new(if folder.collapsed {
                    LucideIcon::ChevronRight
                } else {
                    LucideIcon::ChevronDown
                })
                .text_color(rgb(0x8aa6bd))
                .size(px(FILE_TREE_STATUS_ICON_SIZE)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(rgb(0x8aa6bd))
                    .text_size(px(FILE_TREE_TEXT_SIZE))
                    .truncate()
                    .child(folder.name.clone()),
            )
    }
```

(The `on_hover` handler exists now, rather than in Task 3, so folder rows participate in the sidebar's single hover-index model from the start; Task 3's hover-revealed toggle reads it.)

- [ ] **Step 10: Add the test repository helper**

In `mod tests`, next to `init_repo_with_unmerged_branch_commit` (~line 5430):

```rust
    /// Two commits on master (HEAD at the tip) plus `features/alpha` carrying
    /// one exclusive commit and `features/beta` pointing at the root.
    /// Exercises sidebar folder nesting. Timestamps are fixed so the loaded
    /// order is deterministic. Returns (dir, master_tip_sha, alpha_tip_sha).
    fn init_repo_with_slash_named_branches() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let root_oid = commit_all_to_ref_at_time(&repo, Some("HEAD"), "Root", &[], 10);

        fs::write(dir.path().join("alpha.txt"), "alpha\n").expect("write alpha file");
        let alpha_tip = commit_all_to_ref_at_time(
            &repo,
            Some("refs/heads/features/alpha"),
            "Alpha work",
            &[root_oid],
            20,
        );

        let root_commit = repo.find_commit(root_oid).expect("find root commit");
        repo.branch("features/beta", &root_commit, false)
            .expect("create features/beta");
        drop(root_commit);

        fs::remove_file(dir.path().join("alpha.txt")).expect("remove alpha file");
        fs::write(dir.path().join("hello.txt"), "main\n").expect("update file");
        let main_tip =
            commit_all_to_ref_at_time(&repo, Some("HEAD"), "Main tip", &[root_oid], 30);

        drop(repo);
        (dir, main_tip.to_string(), alpha_tip.to_string())
    }
```

- [ ] **Step 11: Write the view tests**

Next to the existing branch-sidebar view tests (the `clicking_the_eye_icon_hides_the_branch` group, ~line 10306):

```rust
    #[gpui::test]
    async fn slash_named_branches_render_under_a_folder_row(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder = visual
            .debug_bounds("branch-folder-features")
            .expect("slash-named branches render a folder row");
        let alpha = visual
            .debug_bounds("branch-row-features-alpha")
            .expect("nested branch row renders, keyed by full name");
        let beta = visual
            .debug_bounds("branch-row-features-beta")
            .expect("sibling nested branch row renders");
        let master = visual
            .debug_bounds("branch-row-master")
            .expect("flat branch row renders");
        assert!(
            folder.origin.y < alpha.origin.y
                && alpha.origin.y < beta.origin.y
                && beta.origin.y < master.origin.y,
            "row order must be: folder, alpha, beta, master"
        );
    }

    #[gpui::test]
    async fn clicking_a_folder_row_collapses_and_expands_it(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder = visual
            .debug_bounds("branch-folder-features")
            .expect("folder row renders");
        visual.simulate_click(folder.center(), Modifiers::none());

        assert!(
            visual.debug_bounds("branch-row-features-alpha").is_none(),
            "collapsed folder must hide its descendant rows"
        );
        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.is_empty(),
                    "collapsing is visual only; no branch becomes hidden"
                );
                assert!(app.collapsed_branch_folders.contains("features"));
            })
            .expect("read state");

        let folder = visual
            .debug_bounds("branch-folder-features")
            .expect("collapsed folder row still renders");
        visual.simulate_click(folder.center(), Modifiers::none());
        assert!(
            visual.debug_bounds("branch-row-features-alpha").is_some(),
            "expanding must restore descendant rows"
        );
    }

    #[gpui::test]
    async fn reopening_a_repository_expands_all_folders(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.toggle_branch_folder("features".to_string(), cx);
                assert!(app.collapsed_branch_folders.contains("features"));

                app.open_repository_at(path, window, cx);
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "reopening must reset collapse state"
                );
            })
            .expect("open, collapse, reopen");
    }
```

- [ ] **Step 12: Run the view tests**

Run: `cargo test slash_named_branches_render` and `cargo test clicking_a_folder_row` and `cargo test reopening_a_repository_expands`
Expected: PASS.

- [ ] **Step 13: Run `bin/check`**

Expected: clean — fmt, clippy `-D warnings`, full test suite (including all pre-existing sidebar tests, which use flat branch names and are unaffected).

- [ ] **Step 14: Commit**

```bash
git add src/app.rs
git commit -m "feat(graph): nest slash-named branches under collapsible sidebar folders"
```

---

### Task 3: Folder visibility toggles

Adds `FolderVisibility` derivation to the builder, the batched `toggle_folder_visibility` App method, and the hover-revealed eye toggle on folder rows.

**Files:**
- Modify: `src/app.rs` — `BranchFolderRow` + builder, App method near `toggle_branch_visibility`, `render_branch_folder_row`, tests.

- [ ] **Step 1: Write the failing unit tests for visibility derivation**

The `hidden(&[...])` helper already exists at `src/app.rs:5289`. Add next to the Task 2 builder tests:

```rust
    fn head_branch(name: &str, tip_sha: &str) -> repo::LocalBranch {
        repo::LocalBranch {
            name: name.to_string(),
            tip_sha: tip_sha.to_string(),
            is_head: true,
        }
    }

    /// Extract (path, visibility) for every folder row.
    fn folder_visibilities(rows: &[BranchTreeRow]) -> Vec<(String, FolderVisibility)> {
        rows.iter()
            .filter_map(|row| match row {
                BranchTreeRow::Folder(folder) => {
                    Some((folder.path.clone(), folder.visibility))
                }
                BranchTreeRow::Branch(_) => None,
            })
            .collect()
    }

    #[test]
    fn folder_visibility_derives_from_descendants() {
        let branches = vec![
            local_branch("features/a", "a"),
            local_branch("features/b", "b"),
        ];

        let none_hidden = build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&[]));
        let some_hidden =
            build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&["features/a"]));
        let all_hidden = build_branch_tree_rows(
            &branches,
            &BTreeSet::new(),
            &hidden(&["features/a", "features/b"]),
        );

        assert_eq!(
            folder_visibilities(&none_hidden),
            vec![("features".to_string(), FolderVisibility::Visible)]
        );
        assert_eq!(
            folder_visibilities(&some_hidden),
            vec![("features".to_string(), FolderVisibility::Mixed)]
        );
        assert_eq!(
            folder_visibilities(&all_hidden),
            vec![("features".to_string(), FolderVisibility::Hidden)]
        );
    }

    #[test]
    fn folder_visibility_spans_nested_subfolders() {
        // Hiding the only branch in a deep subfolder marks every ancestor
        // folder Hidden, because each ancestor's full descendant set is hidden.
        let branches = vec![local_branch("team/alice/feature-x", "tip")];

        let rows = build_branch_tree_rows(
            &branches,
            &BTreeSet::new(),
            &hidden(&["team/alice/feature-x"]),
        );

        assert_eq!(
            folder_visibilities(&rows),
            vec![
                ("team".to_string(), FolderVisibility::Hidden),
                ("team/alice".to_string(), FolderVisibility::Hidden),
            ]
        );
    }

    #[test]
    fn folder_visibility_ignores_the_head_branch() {
        let branches = vec![
            head_branch("features/current", "c"),
            local_branch("features/other", "o"),
        ];

        let nothing_hidden = build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&[]));
        let other_hidden =
            build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&["features/other"]));

        // HEAD never counts: with the only hideable branch hidden, the folder
        // reads fully hidden even though HEAD inside it stays visible.
        assert_eq!(
            folder_visibilities(&nothing_hidden),
            vec![("features".to_string(), FolderVisibility::Visible)]
        );
        assert_eq!(
            folder_visibilities(&other_hidden),
            vec![("features".to_string(), FolderVisibility::Hidden)]
        );
    }

    #[test]
    fn folder_containing_only_the_head_branch_is_visible() {
        let branches = vec![head_branch("features/current", "c")];

        let rows = build_branch_tree_rows(&branches, &BTreeSet::new(), &hidden(&[]));

        assert_eq!(
            folder_visibilities(&rows),
            vec![("features".to_string(), FolderVisibility::Visible)]
        );
    }
```

Add `FolderVisibility` to the `use super::{...}` list in `mod tests`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test folder_visibility 2>&1 | tail -20` — expected: compile error, `FolderVisibility` not found.

- [ ] **Step 3: Add `FolderVisibility` and derive it in the builder**

Add above `BranchTreeRow`:

```rust
/// A folder's aggregate graph-visibility, derived from its descendant
/// branches' membership in `hidden_branches`. The HEAD branch cannot be
/// hidden and never counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderVisibility {
    /// No hideable descendant is hidden (or there are none).
    Visible,
    /// Every hideable descendant is hidden, and there is at least one.
    Hidden,
    /// Some hideable descendants are hidden, some visible.
    Mixed,
}
```

Add the field to `BranchFolderRow`:

```rust
    visibility: FolderVisibility,
```

In `build_branch_tree_rows`, rename the parameter `_hidden_branches` back to `hidden_branches` and pass it through to `append_branch_tree_rows`, whose signature gains `hidden_branches: &BTreeSet<String>` (before `rows`) and whose folder loop derives the field:

```rust
        rows.push(BranchTreeRow::Folder(BranchFolderRow {
            name: name.clone(),
            path: path.clone(),
            depth,
            collapsed,
            visibility: folder_visibility(child, hidden_branches),
        }));
        if !collapsed {
            append_branch_tree_rows(child, depth + 1, &path, collapsed_folders, hidden_branches, rows);
        }
```

Add the derivation helpers after `append_branch_tree_rows`:

```rust
fn folder_visibility(
    node: &BranchTreeNode,
    hidden_branches: &BTreeSet<String>,
) -> FolderVisibility {
    let mut any_hidden = false;
    let mut any_visible = false;
    collect_folder_visibility(node, hidden_branches, &mut any_hidden, &mut any_visible);
    match (any_hidden, any_visible) {
        (true, false) => FolderVisibility::Hidden,
        (true, true) => FolderVisibility::Mixed,
        _ => FolderVisibility::Visible,
    }
}

fn collect_folder_visibility(
    node: &BranchTreeNode,
    hidden_branches: &BTreeSet<String>,
    any_hidden: &mut bool,
    any_visible: &mut bool,
) {
    for child in node.folders.values() {
        collect_folder_visibility(child, hidden_branches, any_hidden, any_visible);
    }
    for branch in &node.branches {
        if branch.is_head {
            continue;
        }
        if hidden_branches.contains(&branch.name) {
            *any_hidden = true;
        } else {
            *any_visible = true;
        }
    }
}
```

Update the four `BranchFolderRow { ... }` literals in the Task 2 unit tests to include the new field — each gains `visibility: FolderVisibility::Visible,` (none of those tests hide branches). Example, from `slash_named_branch_nests_under_a_folder_even_when_alone`:

```rust
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "features".to_string(),
                    depth: 0,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
                }),
```

(The literals to update are in `slash_named_branch_nests_under_a_folder_even_when_alone`, `multi_level_names_nest_one_folder_per_segment` — two literals — and `collapsed_folder_emits_no_descendant_rows`.)

- [ ] **Step 4: Run the unit tests**

Run: `cargo test folder_visibility` and `cargo test branch_tree` (plus the Task 2 test names).
Expected: all PASS.

- [ ] **Step 5: Write the failing tests for `toggle_folder_visibility`**

The HEAD-in-folder case needs a repo whose checked-out branch is slash-named:

```rust
    /// Like `init_repo_with_slash_named_branches`, but HEAD is moved onto
    /// `features/alpha`, so a sidebar folder contains the checked-out branch.
    fn init_repo_with_head_inside_folder() -> (tempfile::TempDir, String) {
        let (dir, _master_tip, alpha_tip) = init_repo_with_slash_named_branches();
        let repo = Repository::open(dir.path()).expect("open repo");
        repo.set_head("refs/heads/features/alpha").expect("set HEAD");
        drop(repo);
        (dir, alpha_tip)
    }

    #[gpui::test]
    async fn folder_visibility_toggle_hides_then_shows_all_descendants(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);

                app.toggle_folder_visibility("features", cx);
                assert!(app.hidden_branches.contains("features/alpha"));
                assert!(app.hidden_branches.contains("features/beta"));

                app.toggle_folder_visibility("features", cx);
                assert!(
                    app.hidden_branches.is_empty(),
                    "toggling a fully hidden folder must show every descendant"
                );
            })
            .expect("toggle folder visibility twice");
    }

    #[gpui::test]
    async fn folder_visibility_toggle_hides_the_remainder_when_mixed(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("features/alpha".to_string(), cx);

                app.toggle_folder_visibility("features", cx);
                assert!(
                    app.hidden_branches.contains("features/alpha")
                        && app.hidden_branches.contains("features/beta"),
                    "a mixed folder toggle must hide the remaining visible branches"
                );
            })
            .expect("toggle mixed folder");
    }

    #[gpui::test]
    async fn folder_visibility_toggle_skips_the_head_branch(cx: &mut TestAppContext) {
        let (dir, _alpha_tip) = init_repo_with_head_inside_folder();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);

                app.toggle_folder_visibility("features", cx);
                assert!(
                    !app.hidden_branches.contains("features/alpha"),
                    "the checked-out branch must never be hidden"
                );
                assert!(app.hidden_branches.contains("features/beta"));

                app.toggle_folder_visibility("features", cx);
                assert!(app.hidden_branches.is_empty());
            })
            .expect("toggle folder containing HEAD");
    }

    #[gpui::test]
    async fn hiding_a_folder_clears_a_selection_inside_it(cx: &mut TestAppContext) {
        let (dir, _master_tip, alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.selection = Selection::Single {
                    sha: alpha_tip.clone(),
                };

                app.toggle_folder_visibility("features", cx);
                assert_eq!(
                    app.selection,
                    Selection::None,
                    "hiding the folder removed the selected commit, so the selection clears"
                );
            })
            .expect("hide folder containing the selection");
    }
```

- [ ] **Step 6: Run them to verify they fail**

Run: `cargo test folder_visibility_toggle 2>&1 | tail -20` — expected: compile error, no method `toggle_folder_visibility`.

- [ ] **Step 7: Implement `toggle_folder_visibility`**

After `toggle_branch_folder`:

```rust
    /// Flip graph visibility for every branch under a sidebar folder as one
    /// batched change: if any hideable descendant is visible, hide them all;
    /// otherwise show them all. The HEAD branch cannot be hidden and is
    /// skipped, so a folder containing it hides everything else inside.
    pub(crate) fn toggle_folder_visibility(&mut self, path: &str, cx: &mut Context<Self>) {
        let Mode::RepoOpen { repo } = &self.mode else {
            return;
        };
        let prefix = format!("{path}/");
        let descendants = repo
            .local_branches
            .iter()
            .filter(|branch| !branch.is_head && branch.name.starts_with(&prefix))
            .map(|branch| branch.name.clone())
            .collect::<Vec<_>>();
        if descendants.is_empty() {
            return;
        }

        let any_visible = descendants
            .iter()
            .any(|name| !self.hidden_branches.contains(name));
        if any_visible {
            self.hidden_branches.extend(descendants);
            self.clear_selection_if_hidden();
        } else {
            for name in &descendants {
                self.hidden_branches.remove(name);
            }
        }
        cx.notify();
    }
```

(The `{path}/` prefix can't false-match a sibling like `features2/x` against `features`, because the prefix includes the slash. Matching on name prefix rather than the tree gives the full descendant set even for nested subfolders.)

- [ ] **Step 8: Run the toggle tests**

Run: `cargo test folder_visibility_toggle` and `cargo test hiding_a_folder_clears`
Expected: PASS.

- [ ] **Step 9: Write the failing view tests for the folder eye toggle**

```rust
    #[gpui::test]
    async fn clicking_a_folder_eye_hides_every_descendant_branch(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // The toggle is hover-revealed; tests drive the hover state
                // directly. The features folder is row 0.
                app.hovered_branch_row = Some(0);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-folder-visibility-features")
            .expect("hovered folder row reveals its visibility toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.contains("features/alpha")
                        && app.hidden_branches.contains("features/beta"),
                    "folder toggle click must hide every descendant branch"
                );
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "the toggle click must not also collapse the folder"
                );
            })
            .expect("verify post-click state");
    }

    #[gpui::test]
    async fn fully_hidden_folder_keeps_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_folder_visibility("features", cx);
            })
            .expect("open repository and hide the folder");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-visibility-features")
            .expect("a fully hidden folder keeps its toggle visible without hover");
    }

    #[gpui::test]
    async fn partially_hidden_folder_keeps_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("features/alpha".to_string(), cx);
            })
            .expect("open repository and hide one branch");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-visibility-features")
            .expect("a mixed folder keeps its toggle visible without hover");
    }
```

- [ ] **Step 10: Run them to verify they fail**

Run: `cargo test folder_eye 2>&1 | tail -5` and `cargo test hidden_folder_keeps 2>&1 | tail -5`
Expected: FAIL — `debug_bounds("branch-folder-visibility-features")` returns `None` (the selector doesn't exist yet).

- [ ] **Step 11: Add the toggle to `render_branch_folder_row`**

Update the function's locals (after `let row_selector = ...`):

```rust
        let toggle_selector = format!("branch-folder-visibility-{path_fragment}");
        let toggle_path = folder.path.clone();
        let hidden = folder.visibility == FolderVisibility::Hidden;
        let show_toggle = folder.visibility != FolderVisibility::Visible
            || self.hovered_branch_row == Some(index);
        let name_color = if hidden { rgb(0x999999) } else { rgb(0x8aa6bd) };
```

Replace both `rgb(0x8aa6bd)` usages in the chevron and name children with `name_color` (a fully hidden folder mutes like a hidden branch; Mixed keeps the normal color). Then append after the name child, mirroring the branch row's toggle exactly:

```rust
            .when(show_toggle, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .cursor_pointer()
                        .id(("branch-folder-visibility", index))
                        .debug_selector(move || toggle_selector.clone())
                        .on_click(cx.listener(move |app, _event: &ClickEvent, _window, cx| {
                            cx.stop_propagation();
                            app.toggle_folder_visibility(&toggle_path, cx);
                        }))
                        .child(
                            Icon::new(if folder.visibility == FolderVisibility::Visible {
                                LucideIcon::Eye
                            } else {
                                LucideIcon::EyeOff
                            })
                            .text_color(rgb(0x999999))
                            .size(px(FILE_TREE_STATUS_ICON_SIZE)),
                        ),
                )
            })
```

- [ ] **Step 12: Run the view tests**

Run: `cargo test clicking_a_folder_eye` and `cargo test fully_hidden_folder` and `cargo test partially_hidden_folder`
Expected: PASS.

- [ ] **Step 13: Run `bin/check`**

Expected: clean. The existing branch-visibility view tests (`clicking_the_eye_icon_hides_the_branch`, etc.) use flat branch names and must still pass untouched.

- [ ] **Step 14: Commit**

```bash
git add src/app.rs
git commit -m "feat(graph): folder visibility toggles in the branch sidebar"
```

---

### Task 4: Spec update and final verification

Behavior changed, so `docs/specs/review/workflow.md` must change in the same body of work (CLAUDE.md rule). Read `docs/specs/README.md` for spec voice before editing.

**Files:**
- Modify: `docs/specs/review/workflow.md` (sections at lines 109–160)

- [ ] **Step 1: Point the existing sidebar section at nesting**

In the "Navigating branches from the sidebar" intro paragraph (line 111), after the first sentence ("Graph mode includes a sidebar ... in alphabetical order."), insert:

```
Branch names containing `/` nest under collapsible folders (see "Nesting branches in sidebar folders" below).
```

And in its **Observable outcomes** list, change:

```
- The sidebar lists every local branch by name, alphabetically, with the checked-out branch visually marked.
```

to:

```
- The sidebar lists every local branch, alphabetically within its folder level, with the checked-out branch visually marked.
```

- [ ] **Step 2: Add the nesting section**

Insert a new section between "Navigating branches from the sidebar" and "Hiding branches from the graph" (i.e., before line 134):

```markdown
## Nesting branches in sidebar folders

Branch names that contain `/` nest in the sidebar: every name segment except
the last becomes a collapsible folder, so `features/some-feature` appears as
a `some-feature` row inside a `features` folder, and `team/alice/feature-x`
nests two folders deep. A folder exists even when it holds a single branch —
grouping depends only on the branch's own name, so rows do not reorganize as
siblings appear or disappear. Within a level, folders list before branches,
each alphabetically. A nested branch row shows only its final name segment,
indented under its folders; everywhere else — graph labels, hiding,
focusing — the branch keeps its full name.

Activating a folder row collapses or expands it. Collapsing is purely
visual: descendant rows leave the sidebar, but graph visibility does not
change, and branches hidden from the graph stay hidden while collapsed.
Folders start expanded, and collapse state is not persisted: opening a
repository expands every folder.

Each folder row carries a visibility toggle with the same hover-reveal
behavior as branch rows. Activating it hides every branch under the folder
when at least one is visible, and shows them all otherwise. The checked-out
branch cannot be hidden and is skipped: hiding a folder that contains it
hides everything else inside, and the folder reads as fully hidden once all
its other branches are. A fully hidden folder renders muted with its toggle
always visible; a folder with a mix of hidden and visible branches keeps its
normal color but also keeps its toggle visible.

**Observable outcomes**

- A branch named with `/` renders inside one folder per leading segment,
  showing only its final segment, indented by depth.
- A folder appears even for a single nested branch; multi-segment names nest
  multi-level.
- Folders precede branches at each level; both sort alphabetically.
- Activating a folder row removes its descendant rows from the sidebar;
  activating it again restores them. Graph contents are unchanged either way.
- Activating a folder's visibility toggle hides all its branches from the
  graph, skipping the checked-out branch, or shows them all when none are
  visible. A selection that becomes invisible clears, as with single-branch
  hiding.
- A fully hidden folder renders muted with its toggle always shown; a
  partially hidden folder keeps its toggle shown without muting.
- Reopening a repository expands all folders and shows all branches.
```

- [ ] **Step 3: Run `bin/check`**

Expected: clean (docs-only change; this is the final full-suite gate for the feature).

- [ ] **Step 4: Commit**

```bash
git add docs/specs/review/workflow.md
git commit -m "docs(specs): nested branch folder contract for the graph sidebar"
```
