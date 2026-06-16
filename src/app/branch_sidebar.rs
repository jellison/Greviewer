//! Branch-sidebar model building: grouping local and remote branches into a
//! slash-segmented folder tree, deriving per-folder visibility, and producing
//! the flat row list the sidebar renders. Extracted from `app.rs`; see
//! docs/adr/0002-project-layout.md.

use super::*;

/// Tree node used while grouping branches by slash-separated name segments.
/// The BTreeMap keeps sibling folders alphabetical; branches within a node
/// stay in input order, which is alphabetical because they share a prefix
/// and `branches` arrives sorted by full name.
#[derive(Debug, Default)]
pub(crate) struct BranchTreeNode {
    folders: BTreeMap<String, BranchTreeNode>,
    branches: Vec<repo::Branch>,
}

/// Namespaced identity for a branch, mirroring git's ref layout:
/// `heads/{name}` for local branches, `remotes/{name}` for remote-tracking
/// branches. `hidden_branches`, `collapsed_branch_folders`, and debug
/// selectors key on this so a local branch literally named `origin/main`
/// never collides with the remote-tracking `origin/main`.
pub(crate) fn branch_key(name: &str, kind: &repo::BranchKind) -> String {
    match kind {
        repo::BranchKind::Local => format!("heads/{name}"),
        repo::BranchKind::Remote { .. } => format!("remotes/{name}"),
    }
}

/// The full sidebar row list: a "Local" section with the local branch tree,
/// then a "Remote" section whose top-level folders are the remote names
/// (remote branch names already lead with their remote, so the existing
/// tree builder folders them for free). A section with no branches is
/// omitted entirely.
pub(crate) fn build_branch_sidebar_rows(
    branches: &[repo::Branch],
    collapsed_folders: &BTreeSet<String>,
    collapsed_sections: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
) -> Vec<BranchTreeRow> {
    let (local, remote): (Vec<_>, Vec<_>) = branches
        .iter()
        .cloned()
        .partition(|branch| matches!(branch.kind, repo::BranchKind::Local));

    let mut rows = Vec::new();
    append_branch_section(
        &mut rows,
        "Local",
        "heads",
        &local,
        collapsed_folders,
        collapsed_sections,
        hidden_branches,
    );
    append_branch_section(
        &mut rows,
        "Remote",
        "remotes",
        &remote,
        collapsed_folders,
        collapsed_sections,
        hidden_branches,
    );
    rows
}

/// Append one sidebar section: a header carrying the branch count and collapse
/// state, followed by the branch tree unless the section is collapsed. A
/// section with no branches contributes nothing.
fn append_branch_section(
    rows: &mut Vec<BranchTreeRow>,
    title: &str,
    key: &str,
    branches: &[repo::Branch],
    collapsed_folders: &BTreeSet<String>,
    collapsed_sections: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
) {
    if branches.is_empty() {
        return;
    }

    let collapsed = collapsed_sections.contains(key);
    rows.push(BranchTreeRow::Section(BranchSectionRow {
        title: title.to_string(),
        key: key.to_string(),
        count: branches.len(),
        collapsed,
    }));
    if !collapsed {
        rows.extend(build_branch_tree_rows(
            branches,
            key,
            collapsed_folders,
            hidden_branches,
        ));
    }
}

/// Group branches into folders by `/`-separated name segments and flatten
/// the tree into depth-tagged sidebar rows. Folders list before branches at
/// each level. A collapsed folder contributes its own row and skips every
/// descendant. Git rejects ref names with empty segments, so segments are
/// always non-empty. `key_prefix` seeds the folder key paths (`heads` for
/// local branches or `remotes` for remote-tracking branches) without
/// affecting row indentation.
pub(crate) fn build_branch_tree_rows(
    branches: &[repo::Branch],
    key_prefix: &str,
    collapsed_folders: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
) -> Vec<BranchTreeRow> {
    let mut root = BranchTreeNode::default();
    for branch in branches {
        let mut segments = branch.name.split('/').collect::<Vec<_>>();
        segments.pop();
        let mut node = &mut root;
        for segment in segments {
            node = node.folders.entry(segment.to_string()).or_default();
        }
        node.branches.push(branch.clone());
    }

    let mut rows = Vec::new();
    append_branch_tree_rows(
        &root,
        0,
        key_prefix,
        collapsed_folders,
        hidden_branches,
        &mut rows,
    );
    rows
}

pub(crate) fn append_branch_tree_rows(
    node: &BranchTreeNode,
    depth: usize,
    prefix: &str,
    collapsed_folders: &BTreeSet<String>,
    hidden_branches: &BTreeSet<String>,
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
            visibility: folder_visibility(child, hidden_branches),
        }));
        if !collapsed {
            append_branch_tree_rows(
                child,
                depth + 1,
                &path,
                collapsed_folders,
                hidden_branches,
                rows,
            );
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

pub(crate) fn folder_visibility(
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

pub(crate) fn collect_folder_visibility(
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
        if hidden_branches.contains(&branch_key(&branch.name, &branch.kind)) {
            *any_hidden = true;
        } else {
            *any_visible = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::repo;
    use gpui::{px, TestAppContext, VisualTestContext};

    #[test]
    fn flat_branch_names_produce_flat_rows() {
        let branches = vec![local_branch("feature", "f"), local_branch("master", "m")];

        let rows = build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &BTreeSet::new());

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

        let rows = build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "heads/features".to_string(),
                    depth: 0,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
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

        let rows = build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "team".to_string(),
                    path: "heads/team".to_string(),
                    depth: 0,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
                }),
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "alice".to_string(),
                    path: "heads/team/alice".to_string(),
                    depth: 1,
                    collapsed: false,
                    visibility: FolderVisibility::Visible,
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

        let rows = build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &BTreeSet::new());

        let order = rows
            .iter()
            .map(|row| match row {
                BranchTreeRow::Section(section) => format!("section:{}", section.title),
                BranchTreeRow::Folder(folder) => format!("folder:{}", folder.path),
                BranchTreeRow::Branch(branch_row) => {
                    format!("branch:{}", branch_row.branch.name)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            vec![
                "folder:heads/features",
                "branch:features/x",
                "branch:alpha",
                "branch:zeta"
            ]
        );
    }

    #[test]
    fn collapsed_folder_emits_no_descendant_rows() {
        let branches = vec![
            local_branch("features/inner/deep", "d"),
            local_branch("features/x", "x"),
            local_branch("master", "m"),
        ];
        let collapsed = ["heads/features"]
            .iter()
            .map(|path| path.to_string())
            .collect::<BTreeSet<_>>();

        let rows = build_branch_tree_rows(&branches, "heads", &collapsed, &BTreeSet::new());

        assert_eq!(
            rows,
            vec![
                BranchTreeRow::Folder(BranchFolderRow {
                    name: "features".to_string(),
                    path: "heads/features".to_string(),
                    depth: 0,
                    collapsed: true,
                    visibility: FolderVisibility::Visible,
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
    fn sidebar_rows_group_local_and_remote_branches_into_sections() {
        let branches = vec![
            local_branch("main", "sha-main"),
            remote_branch("origin", "main", "sha-remote-main"),
            remote_branch("origin", "feature/x", "sha-remote-feature"),
            remote_branch("upstream", "main", "sha-upstream-main"),
        ];

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );

        let summary = rows
            .iter()
            .map(|row| match row {
                BranchTreeRow::Section(section) => format!("section:{}", section.title),
                BranchTreeRow::Folder(folder) => {
                    format!("folder:{}@{}", folder.path, folder.depth)
                }
                BranchTreeRow::Branch(branch_row) => {
                    format!("branch:{}@{}", branch_row.display_name, branch_row.depth)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                "section:Local",
                "branch:main@0",
                "section:Remote",
                "folder:remotes/origin@0",
                "folder:remotes/origin/feature@1",
                "branch:x@2",
                "branch:main@1",
                "folder:remotes/upstream@0",
                "branch:main@1",
            ]
            .map(str::to_string)
            .to_vec(),
            "locals list under a Local section; each remote folders its branches, \
             nesting slash-named branches like local ones",
        );
    }

    #[test]
    fn sidebar_rows_omit_empty_sections() {
        let local_only = vec![local_branch("main", "sha-main")];
        let rows = build_branch_sidebar_rows(
            &local_only,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        assert!(
            !rows.iter().any(|row| matches!(
                row,
                BranchTreeRow::Section(section) if section.title == "Remote"
            )),
            "no Remote section without remote branches",
        );

        let remote_only = vec![remote_branch("origin", "main", "sha-remote")];
        let rows = build_branch_sidebar_rows(
            &remote_only,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        assert!(
            !rows.iter().any(|row| matches!(
                row,
                BranchTreeRow::Section(section) if section.title == "Local"
            )),
            "no Local section without local branches",
        );
    }

    #[test]
    fn section_rows_carry_their_branch_counts() {
        let branches = vec![
            local_branch("main", "sha-main"),
            local_branch("feature", "sha-feature"),
            remote_branch("origin", "main", "sha-remote-main"),
        ];

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );

        let sections = rows
            .iter()
            .filter_map(|row| match row {
                BranchTreeRow::Section(section) => Some((section.title.clone(), section.count)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sections,
            vec![("Local".to_string(), 2), ("Remote".to_string(), 1)],
            "each section header reports the number of branches it contains",
        );
    }

    #[test]
    fn collapsed_section_omits_its_child_rows() {
        let branches = vec![
            local_branch("main", "sha-main"),
            remote_branch("origin", "main", "sha-remote"),
        ];
        let collapsed_sections = ["heads".to_string()].into_iter().collect();

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &collapsed_sections,
            &BTreeSet::new(),
        );

        let summary = rows
            .iter()
            .map(|row| match row {
                BranchTreeRow::Section(section) => {
                    format!("section:{}:{}", section.title, section.collapsed)
                }
                BranchTreeRow::Folder(folder) => format!("folder:{}", folder.path),
                BranchTreeRow::Branch(branch_row) => format!("branch:{}", branch_row.branch.name),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                "section:Local:true".to_string(),
                "section:Remote:false".to_string(),
                "folder:remotes/origin".to_string(),
                "branch:origin/main".to_string(),
            ],
            "a collapsed section keeps its header (marked collapsed) but drops every descendant row",
        );
    }

    #[test]
    fn collapsing_a_local_folder_leaves_the_same_named_remote_folder_open() {
        let branches = vec![
            local_branch("origin/main", "sha-local"),
            remote_branch("origin", "main", "sha-remote"),
        ];
        let collapsed = ["heads/origin".to_string()].into_iter().collect();

        let rows =
            build_branch_sidebar_rows(&branches, &collapsed, &BTreeSet::new(), &BTreeSet::new());

        let branch_kinds = rows
            .iter()
            .filter_map(|row| match row {
                BranchTreeRow::Branch(branch_row) => Some(branch_row.branch.kind.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            branch_kinds,
            vec![repo::BranchKind::Remote {
                remote: "origin".to_string()
            }],
            "the collapsed local folder hides its branch row; the remote folder's row stays",
        );
    }

    #[test]
    fn folder_visibility_derives_from_descendants() {
        let branches = vec![
            local_branch("features/a", "a"),
            local_branch("features/b", "b"),
        ];

        let none_hidden =
            build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &hidden(&[]));
        let some_hidden = build_branch_tree_rows(
            &branches,
            "heads",
            &BTreeSet::new(),
            &hidden(&["heads/features/a"]),
        );
        let all_hidden = build_branch_tree_rows(
            &branches,
            "heads",
            &BTreeSet::new(),
            &hidden(&["heads/features/a", "heads/features/b"]),
        );

        assert_eq!(
            folder_visibilities(&none_hidden),
            vec![("heads/features".to_string(), FolderVisibility::Visible)]
        );
        assert_eq!(
            folder_visibilities(&some_hidden),
            vec![("heads/features".to_string(), FolderVisibility::Mixed)]
        );
        assert_eq!(
            folder_visibilities(&all_hidden),
            vec![("heads/features".to_string(), FolderVisibility::Hidden)]
        );
    }

    #[test]
    fn folder_visibility_spans_nested_subfolders() {
        // Hiding the only branch in a deep subfolder marks every ancestor
        // folder Hidden, because each ancestor's full descendant set is hidden.
        let branches = vec![local_branch("team/alice/feature-x", "tip")];

        let rows = build_branch_tree_rows(
            &branches,
            "heads",
            &BTreeSet::new(),
            &hidden(&["heads/team/alice/feature-x"]),
        );

        assert_eq!(
            folder_visibilities(&rows),
            vec![
                ("heads/team".to_string(), FolderVisibility::Hidden),
                ("heads/team/alice".to_string(), FolderVisibility::Hidden),
            ]
        );
    }

    #[test]
    fn folder_visibility_ignores_the_head_branch() {
        let branches = vec![
            head_branch("features/current", "c"),
            local_branch("features/other", "o"),
        ];

        let nothing_hidden =
            build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &hidden(&[]));
        let other_hidden = build_branch_tree_rows(
            &branches,
            "heads",
            &BTreeSet::new(),
            &hidden(&["heads/features/other"]),
        );

        // HEAD never counts: with the only hideable branch hidden, the folder
        // reads fully hidden even though HEAD inside it stays visible.
        assert_eq!(
            folder_visibilities(&nothing_hidden),
            vec![("heads/features".to_string(), FolderVisibility::Visible)]
        );
        assert_eq!(
            folder_visibilities(&other_hidden),
            vec![("heads/features".to_string(), FolderVisibility::Hidden)]
        );
    }

    #[test]
    fn folder_containing_only_the_head_branch_is_visible() {
        let branches = vec![head_branch("features/current", "c")];

        let rows = build_branch_tree_rows(&branches, "heads", &BTreeSet::new(), &hidden(&[]));

        assert_eq!(
            folder_visibilities(&rows),
            vec![("heads/features".to_string(), FolderVisibility::Visible)]
        );
    }

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
            .debug_bounds("branch-folder-heads-features")
            .expect("slash-named branches render a folder row");
        let alpha = visual
            .debug_bounds("branch-row-heads-features-alpha")
            .expect("nested branch row renders, keyed by full name");
        let beta = visual
            .debug_bounds("branch-row-heads-features-beta")
            .expect("sibling nested branch row renders");
        let master = visual
            .debug_bounds("branch-row-heads-master")
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
            .debug_bounds("branch-folder-heads-features")
            .expect("folder row renders");
        visual.simulate_click(folder.center(), Modifiers::none());

        // Verify app state after the click.
        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.is_empty(),
                    "collapsing is visual only; no branch becomes hidden"
                );
                assert!(
                    app.collapsed_branch_folders.contains("heads/features"),
                    "features must be in collapsed_branch_folders after click"
                );
                // Verify that the tree builder produces the collapsed layout.
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let rows = build_branch_tree_rows(
                    &repo.branches,
                    "heads",
                    &app.collapsed_branch_folders,
                    &app.hidden_branches,
                );
                assert_eq!(
                    rows.len(),
                    2,
                    "collapsed folder must hide descendant rows; rows: {:?}",
                    rows
                );
                assert!(
                    matches!(&rows[0], BranchTreeRow::Folder(f) if f.collapsed),
                    "first row is the collapsed folder"
                );
                assert!(
                    matches!(&rows[1], BranchTreeRow::Branch(b) if b.branch.name == "master"),
                    "second row is the master branch"
                );
            })
            .expect("read state after collapse");

        // Verify the folder row is still rendered (collapsed, not removed).
        visual
            .debug_bounds("branch-folder-heads-features")
            .expect("collapsed folder row still renders");

        // Click again to expand.
        let folder = visual
            .debug_bounds("branch-folder-heads-features")
            .expect("collapsed folder row still renders for second click");
        visual.simulate_click(folder.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "second click must expand the folder"
                );
            })
            .expect("read state after expand");
    }

    #[gpui::test]
    async fn reopening_a_repository_expands_all_folders(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.toggle_branch_folder("heads/features".to_string(), cx);
                assert!(app.collapsed_branch_folders.contains("heads/features"));

                app.open_repository_at(path, window, cx);
                assert!(
                    app.collapsed_branch_folders.is_empty(),
                    "reopening must reset collapse state"
                );
            })
            .expect("open, collapse, reopen");
    }

    #[gpui::test]
    async fn sidebar_renders_remote_section_with_a_folder_per_remote(cx: &mut TestAppContext) {
        let (dir, _root, _remote_tip) = init_repo_with_remote_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let local_section = visual
            .debug_bounds("branch-section-local")
            .expect("Local section header renders");
        let remote_section = visual
            .debug_bounds("branch-section-remote")
            .expect("Remote section header renders");
        visual
            .debug_bounds("branch-folder-remotes-origin")
            .expect("the remote renders as a folder");
        visual
            .debug_bounds("branch-folder-remotes-origin-feature")
            .expect("slash-named remote branches nest in subfolders");
        visual
            .debug_bounds("branch-row-remotes-origin-master")
            .expect("remote branch row renders, keyed by namespaced name");
        visual
            .debug_bounds("branch-row-remotes-origin-feature-x")
            .expect("nested remote branch row renders");
        let local_row = visual
            .debug_bounds("branch-row-heads-master")
            .expect("local branch row renders under the Local section");
        assert!(
            local_section.origin.y < local_row.origin.y
                && local_row.origin.y < remote_section.origin.y,
            "sections order: Local header, local rows, Remote header"
        );
        // Row 0 is the remote-only tip (origin/feature/x at time 200, newest).
        // Row 1 is the root commit (master + origin/master at time 100).
        visual
            .debug_bounds("commit-ref-label-0-remotes-origin-feature-x")
            .expect("remote ref label pill renders in the graph with its namespaced selector");
        visual
            .debug_bounds("commit-ref-label-1-remotes-origin-master")
            .expect("remote ref label pill renders in the graph with its namespaced selector");
        visual
            .debug_bounds("commit-ref-label-1-heads-master")
            .expect("local ref label pill renders on the shared-tip commit row");
    }

    #[gpui::test]
    async fn hiding_a_remote_branch_removes_its_exclusive_commits(cx: &mut TestAppContext) {
        let (dir, _root, remote_tip) = init_repo_with_remote_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                let visible_before = match &app.mode {
                    Mode::RepoOpen { repo } => visible_commits(repo, &app.hidden_branches)
                        .iter()
                        .map(|commit| commit.sha.clone())
                        .collect::<Vec<_>>(),
                    Mode::NoRepo => Vec::new(),
                };
                assert!(
                    visible_before.contains(&remote_tip),
                    "the remote-only commit loads and renders",
                );

                app.toggle_branch_visibility("remotes/origin/feature/x".to_string(), cx);
                let visible_after = match &app.mode {
                    Mode::RepoOpen { repo } => visible_commits(repo, &app.hidden_branches)
                        .iter()
                        .map(|commit| commit.sha.clone())
                        .collect::<Vec<_>>(),
                    Mode::NoRepo => Vec::new(),
                };
                assert!(
                    !visible_after.contains(&remote_tip),
                    "hiding the remote branch drops its exclusive commit",
                );
            })
            .expect("toggle remote branch visibility");
    }

    #[gpui::test]
    async fn folder_visibility_toggle_hides_then_shows_all_descendants(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);

                app.toggle_folder_visibility("heads/features", cx);
                assert!(app.hidden_branches.contains("heads/features/alpha"));
                assert!(app.hidden_branches.contains("heads/features/beta"));

                app.toggle_folder_visibility("heads/features", cx);
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
                app.toggle_branch_visibility("heads/features/alpha".to_string(), cx);

                app.toggle_folder_visibility("heads/features", cx);
                assert!(
                    app.hidden_branches.contains("heads/features/alpha")
                        && app.hidden_branches.contains("heads/features/beta"),
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

                app.toggle_folder_visibility("heads/features", cx);
                assert!(
                    !app.hidden_branches.contains("heads/features/alpha"),
                    "the checked-out branch must never be hidden"
                );
                assert!(app.hidden_branches.contains("heads/features/beta"));

                app.toggle_folder_visibility("heads/features", cx);
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

                app.toggle_folder_visibility("heads/features", cx);
                assert_eq!(
                    app.selection,
                    Selection::None,
                    "hiding the folder removed the selected commit, so the selection clears"
                );
            })
            .expect("hide folder containing the selection");
    }

    #[gpui::test]
    async fn clicking_a_folder_eye_hides_every_descendant_branch(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // The toggle is hover-revealed; tests drive the hover state
                // directly. Row 0 is the "Local" section header; the
                // features folder is row 1.
                app.hovered_branch_row = Some(1);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-folder-visibility-heads-features")
            .expect("hovered folder row reveals its visibility toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.contains("heads/features/alpha")
                        && app.hidden_branches.contains("heads/features/beta"),
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
                app.toggle_folder_visibility("heads/features", cx);
            })
            .expect("open repository and hide the folder");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-visibility-heads-features")
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
                app.toggle_branch_visibility("heads/features/alpha".to_string(), cx);
            })
            .expect("open repository and hide one branch");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-visibility-heads-features")
            .expect("a mixed folder keeps its toggle visible without hover");
    }

    #[gpui::test]
    async fn clicking_the_eye_icon_hides_the_branch(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // The toggle is hover-revealed; tests drive the hover state
                // directly. Row 0 is the "Local" section header; branches
                // sort alphabetically below it, so feature is row 1.
                app.hovered_branch_row = Some(1);
                cx.notify();
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-visibility-heads-feature")
            .expect("hovered branch row reveals its visibility toggle");
        visual.simulate_click(toggle.center(), Modifiers::none());

        // Verify clicking the toggle called toggle_branch_visibility:
        // (a) the branch is now in hidden_branches, and
        // (b) the visible commit count dropped from 3 to 2.
        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.hidden_branches.contains("heads/feature"),
                    "visibility toggle click must add branch to hidden_branches"
                );
                assert_eq!(
                    app.selection,
                    Selection::None,
                    "visibility toggle click must not focus the branch"
                );
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let head_sha = repo
                    .commits
                    .iter()
                    .find(|c| c.is_head)
                    .map(|c| c.sha.as_str());
                let visible = visible_commit_shas(
                    &repo.commits,
                    &repo.branches,
                    head_sha,
                    &app.hidden_branches,
                );
                assert_eq!(
                    visible.len(),
                    2,
                    "feature-exclusive commit must be absent from visible set"
                );
            })
            .expect("verify post-click state");
    }

    #[gpui::test]
    async fn hidden_branch_shows_its_toggle_without_hover(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-visibility-heads-feature")
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
            visual
                .debug_bounds("branch-visibility-heads-master")
                .is_none(),
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
                app.toggle_branch_visibility("heads/feature".to_string(), cx);
            })
            .expect("open repository and hide feature");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row = visual
            .debug_bounds("branch-row-heads-feature")
            .expect("hidden branch row still renders");
        let toggle = visual
            .debug_bounds("branch-visibility-heads-feature")
            .expect("hidden branch renders its always-visible toggle");
        assert!(
            row.origin.x + px(8.) < toggle.origin.x,
            "left-edge click point must fall outside the toggle icon"
        );
        // Click near the left edge so the click cannot land on the
        // always-visible eye-off icon at the row's right edge.
        visual.simulate_click(
            point(row.origin.x + px(8.), row.center().y),
            Modifiers::none(),
        );

        window
            .update(cx, |app, _window, _cx| {
                assert_eq!(app.selection, Selection::None);
            })
            .expect("read selection");
    }

    /// Rebuild the flat sidebar rows from an open app's current state, the way
    /// `render_branch_sidebar` does. Lets collapse tests assert against the
    /// model rather than the last-drawn frame.
    fn rebuild_sidebar_rows(app: &App) -> Vec<BranchTreeRow> {
        let Mode::RepoOpen { repo } = &app.mode else {
            panic!("expected RepoOpen mode");
        };
        build_branch_sidebar_rows(
            &repo.branches,
            &app.collapsed_branch_folders,
            &app.collapsed_branch_sections,
            &app.hidden_branches,
        )
    }

    fn local_branch_rows_present(app: &App) -> bool {
        rebuild_sidebar_rows(app).iter().any(|row| {
            matches!(row, BranchTreeRow::Branch(b) if matches!(b.branch.kind, repo::BranchKind::Local))
        })
    }

    fn remote_branch_rows_present(app: &App) -> bool {
        rebuild_sidebar_rows(app).iter().any(|row| {
            matches!(row, BranchTreeRow::Branch(b) if matches!(b.branch.kind, repo::BranchKind::Remote { .. }))
        })
    }

    #[gpui::test]
    async fn section_header_renders_its_icon_and_count(cx: &mut TestAppContext) {
        let (dir, _root, _remote_tip) = init_repo_with_remote_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-section-icon-local")
            .expect("the Local section header renders its icon");
        visual
            .debug_bounds("branch-section-count-local")
            .expect("the Local section header renders its branch count");
        visual
            .debug_bounds("branch-section-icon-remote")
            .expect("the Remote section header renders its icon");
        visual
            .debug_bounds("branch-section-count-remote")
            .expect("the Remote section header renders its branch count");
    }

    #[gpui::test]
    async fn clicking_a_section_header_collapses_and_expands_the_section(cx: &mut TestAppContext) {
        let (dir, _root, _remote_tip) = init_repo_with_remote_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repo");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let header = visual
            .debug_bounds("branch-section-local")
            .expect("the Local section header renders");
        visual.simulate_click(header.center(), Modifiers::none());

        // Verify collapse through the rows builder, mirroring the folder-row
        // collapse test: debug_bounds reflects the last drawn frame, which is
        // unreliable to assert against immediately after a click.
        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.collapsed_branch_sections.contains("heads"),
                    "clicking the Local header collapses the heads section"
                );
                assert!(
                    !local_branch_rows_present(app),
                    "the collapsed Local section hides its branch rows"
                );
                assert!(
                    remote_branch_rows_present(app),
                    "collapsing Local leaves the Remote section expanded"
                );
            })
            .expect("read state after collapse");

        // Click the header again (its position is unchanged) to expand.
        visual.simulate_click(header.center(), Modifiers::none());

        window
            .update(cx, |app, _window, _cx| {
                assert!(
                    app.collapsed_branch_sections.is_empty(),
                    "a second click expands the section"
                );
                assert!(
                    local_branch_rows_present(app),
                    "the expanded Local section shows its branch rows again"
                );
            })
            .expect("read state after expand");
    }

    #[gpui::test]
    async fn reopening_a_repository_expands_all_sections(cx: &mut TestAppContext) {
        let (dir, _root, _remote_tip) = init_repo_with_remote_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.toggle_branch_section("heads".to_string(), cx);
                assert!(app.collapsed_branch_sections.contains("heads"));

                app.open_repository_at(path, window, cx);
                assert!(
                    app.collapsed_branch_sections.is_empty(),
                    "reopening must reset section collapse state"
                );
            })
            .expect("open, collapse, reopen");
    }
}
