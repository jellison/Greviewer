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
    query: &str,
) -> Vec<BranchTreeRow> {
    let filtering = !query.is_empty();

    let (local, remote): (Vec<_>, Vec<_>) = branches
        .iter()
        .filter(|branch| !filtering || fuzzy_match(&branch.name, query).is_some())
        .cloned()
        .partition(|branch| matches!(branch.kind, repo::BranchKind::Local));

    // While filtering, ignore saved collapse state so every surviving folder
    // and section is expanded and its matches are visible. Section counts fall
    // out of the (already filtered) slice lengths in `append_branch_section`.
    let no_collapsed_folders = BTreeSet::new();
    let no_collapsed_sections = BTreeSet::new();
    let (folders, sections) = if filtering {
        (&no_collapsed_folders, &no_collapsed_sections)
    } else {
        (collapsed_folders, collapsed_sections)
    };

    let mut rows = Vec::new();
    append_branch_section(
        &mut rows,
        "Local",
        "heads",
        &local,
        folders,
        sections,
        hidden_branches,
    );
    append_branch_section(
        &mut rows,
        "Remote",
        "remotes",
        &remote,
        folders,
        sections,
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
    // Draw a separating top border only when this header follows content rows.
    // Headers stacking directly (a collapsed section above) lean on the upper
    // header's bottom border, and the topmost header leans on the sidebar's own
    // border; either way a top border here would double up.
    let top_border = !matches!(rows.last(), None | Some(BranchTreeRow::Section(_)));
    rows.push(BranchTreeRow::Section(BranchSectionRow {
        title: title.to_string(),
        key: key.to_string(),
        count: branches.len(),
        collapsed,
        top_border,
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

    #[gpui::test]
    async fn branch_sidebar_virtualizes_offscreen_rows(cx: &mut TestAppContext) {
        use gpui::size;

        let dir = init_repo_with_many_branches(); // ~200 local branches
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(900.), px(400.)));

        // Branch names are zero-padded so row order is deterministic; an early
        // row is on screen, a far one is culled. Every branch tips the root
        // commit, which is the default selection on open, so every branch row
        // renders with the selected treatment.
        visual
            .debug_bounds("selected-branch-row-heads-branch-000")
            .expect("first branch row should be materialized");
        assert!(
            visual
                .debug_bounds("selected-branch-row-heads-branch-150")
                .is_none(),
            "row 150 is far below the viewport and must not be materialized"
        );
    }

    #[gpui::test]
    async fn sidebar_rows_are_cached_until_inputs_change(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let first = window
            .update(cx, |app, _window, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let _ = app.sidebar_rows(repo, "");
                app.sidebar_rows_recompute_count()
            })
            .expect("first build");

        let second = window
            .update(cx, |app, _window, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let _ = app.sidebar_rows(repo, "");
                app.sidebar_rows_recompute_count()
            })
            .expect("second build");
        assert_eq!(first, second, "identical inputs must not rebuild the model");

        let after_collapse = window
            .update(cx, |app, _window, cx| {
                app.toggle_branch_folder("heads/features".to_string(), cx);
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let _ = app.sidebar_rows(repo, "");
                app.sidebar_rows_recompute_count()
            })
            .expect("build after collapse");
        assert_eq!(
            after_collapse,
            second + 1,
            "collapsing a folder must rebuild the model exactly once"
        );
    }

    #[test]
    fn empty_query_reproduces_the_unfiltered_tree() {
        let branches = vec![
            local_branch("features/alpha", "a"),
            local_branch("master", "m"),
        ];
        let collapsed_folders = ["heads/features".to_string()].into_iter().collect();

        let filtered = build_branch_sidebar_rows(
            &branches,
            &collapsed_folders,
            &BTreeSet::new(),
            &BTreeSet::new(),
            "",
        );
        // The empty-query build must produce the honored-collapse tree: a
        // non-empty set of rows containing the `master` branch and the
        // `features` folder (rather than a vacuous compare-to-self).
        assert!(!filtered.is_empty(), "empty query must render the tree");
        assert!(
            filtered.iter().any(|row| matches!(
                row,
                BranchTreeRow::Branch(b) if b.branch.name == "master"
            )),
            "empty query must include the master branch row"
        );
        assert!(
            filtered.iter().any(|row| matches!(
                row,
                BranchTreeRow::Folder(f) if f.path == "heads/features"
            )),
            "empty query must include the features folder row"
        );
        // The collapsed folder still suppresses its descendant branch row.
        assert!(
            !filtered.iter().any(|row| matches!(
                row,
                BranchTreeRow::Branch(b) if b.branch.name == "features/alpha"
            )),
            "empty query must not force-expand collapsed folders"
        );
    }

    #[test]
    fn query_prunes_non_matching_branches_and_empty_folders() {
        let branches = vec![
            local_branch("features/login", "l"),
            local_branch("features/search", "s"),
            local_branch("master", "m"),
        ];

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            "login",
        );

        let summary = rows
            .iter()
            .map(|row| match row {
                BranchTreeRow::Section(s) => format!("section:{}:{}", s.title, s.count),
                BranchTreeRow::Folder(f) => format!("folder:{}", f.path),
                BranchTreeRow::Branch(b) => format!("branch:{}", b.branch.name),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                "section:Local:1".to_string(),
                "folder:heads/features".to_string(),
                "branch:features/login".to_string(),
            ],
            "only the matching branch, its folder, and a count of 1 survive"
        );
    }

    #[test]
    fn query_force_expands_collapsed_folders_and_sections() {
        let branches = vec![local_branch("features/login", "l")];
        let collapsed_folders = ["heads/features".to_string()].into_iter().collect();
        let collapsed_sections = ["heads".to_string()].into_iter().collect();

        let rows = build_branch_sidebar_rows(
            &branches,
            &collapsed_folders,
            &collapsed_sections,
            &BTreeSet::new(),
            "login",
        );

        assert!(
            rows.iter().any(|row| matches!(
                row,
                BranchTreeRow::Branch(b) if b.branch.name == "features/login"
            )),
            "a match inside a collapsed folder/section is revealed"
        );
        assert!(
            rows.iter().any(|row| matches!(
                row,
                BranchTreeRow::Section(s) if s.title == "Local" && !s.collapsed
            )),
            "the section renders expanded while filtering"
        );
        assert!(
            rows.iter().any(|row| matches!(
                row,
                BranchTreeRow::Folder(f) if f.path == "heads/features" && !f.collapsed
            )),
            "the folder renders expanded while filtering"
        );
    }

    #[test]
    fn query_matching_nothing_yields_no_rows() {
        let branches = vec![local_branch("master", "m")];
        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            "zzz",
        );
        assert!(rows.is_empty(), "no branch matches, so the model is empty");
    }

    #[test]
    fn query_matches_across_the_full_remote_path() {
        let branches = vec![
            remote_branch("origin", "feature/login", "l"),
            remote_branch("origin", "feature/search", "s"),
        ];

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            " orglgn".trim(),
        );

        // "orglgn" is a subsequence of "origin/feature/login" but not of
        // "origin/feature/search".
        let branch_names = rows
            .iter()
            .filter_map(|row| match row {
                BranchTreeRow::Branch(b) => Some(b.branch.name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(branch_names, vec!["origin/feature/login".to_string()]);
    }

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
            "",
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
            "",
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
            "",
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
            "",
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
            "",
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

    fn section_top_borders(rows: &[BranchTreeRow]) -> Vec<(String, bool)> {
        rows.iter()
            .filter_map(|row| match row {
                BranchTreeRow::Section(section) => {
                    Some((section.title.clone(), section.top_border))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn remote_section_draws_a_top_border_below_expanded_local_rows() {
        let branches = vec![
            local_branch("main", "sha-main"),
            remote_branch("origin", "main", "sha-remote"),
        ];

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            "",
        );

        assert_eq!(
            section_top_borders(&rows),
            vec![("Local".to_string(), false), ("Remote".to_string(), true)],
            "Local heads the list with no top border; Remote, sitting below the \
             expanded local rows, draws one to close the group",
        );
    }

    #[test]
    fn stacked_section_headers_draw_no_top_border() {
        // Local collapsed: the two headers stack directly, so Remote must not
        // stack a top border atop Local's bottom border.
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
            "",
        );

        assert_eq!(
            section_top_borders(&rows),
            vec![("Local".to_string(), false), ("Remote".to_string(), false)],
            "with Local collapsed the headers stack; neither draws a top border",
        );
    }

    #[test]
    fn lone_remote_section_draws_no_top_border() {
        let branches = vec![remote_branch("origin", "main", "sha-remote")];

        let rows = build_branch_sidebar_rows(
            &branches,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            "",
        );

        assert_eq!(
            section_top_borders(&rows),
            vec![("Remote".to_string(), false)],
            "a Remote section at the top of the list relies on the sidebar's own border",
        );
    }

    #[test]
    fn collapsing_a_local_folder_leaves_the_same_named_remote_folder_open() {
        let branches = vec![
            local_branch("origin/main", "sha-local"),
            remote_branch("origin", "main", "sha-remote"),
        ];
        let collapsed = ["heads/origin".to_string()].into_iter().collect();

        let rows = build_branch_sidebar_rows(
            &branches,
            &collapsed,
            &BTreeSet::new(),
            &BTreeSet::new(),
            "",
        );

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
        // Master's tip is the default selection on open, so its row renders
        // with the selected treatment.
        let master = visual
            .debug_bounds("selected-branch-row-heads-master")
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
        // The checked-out tip is the default selection on open; both master
        // rows point at it, so both render with the selected treatment.
        visual
            .debug_bounds("selected-branch-row-remotes-origin-master")
            .expect("remote branch row renders, keyed by namespaced name");
        visual
            .debug_bounds("branch-row-remotes-origin-feature-x")
            .expect("nested remote branch row renders");
        let local_row = visual
            .debug_bounds("selected-branch-row-heads-master")
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
    async fn hiding_a_folder_resets_a_selection_inside_it(cx: &mut TestAppContext) {
        let (dir, master_tip, alpha_tip) = init_repo_with_slash_named_branches();
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
                    Selection::Single {
                        sha: master_tip.clone(),
                    },
                    "hiding the folder removed the selected commit, so the selection resets to the checked-out tip"
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
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-folder-visibility-heads-features")
            .expect("folder row lays out its visibility toggle (revealed via group-hover)");
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
        let (dir, main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let toggle = visual
            .debug_bounds("branch-visibility-heads-feature")
            .expect("branch row lays out its visibility toggle (revealed via group-hover)");
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
                    Selection::Single {
                        sha: main_tip.clone(),
                    },
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
        let (dir, main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
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
                assert_eq!(
                    app.selection,
                    Selection::Single {
                        sha: main_tip.clone(),
                    },
                    "clicking a hidden branch row must leave the default selection in place"
                );
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
            "",
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
    async fn branch_rows_render_a_leading_branch_icon(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let icon = visual
            .debug_bounds("branch-icon-heads-feature")
            .expect("a branch row renders a leading branch icon");
        let row = visual
            .debug_bounds("branch-row-heads-feature")
            .expect("the branch row renders");
        assert!(
            icon.origin.x >= row.origin.x && icon.origin.x < row.center().x,
            "the branch icon sits at the start of its row"
        );
        visual
            .debug_bounds("branch-icon-heads-master")
            .expect("the checked-out branch row also renders a branch icon");
    }

    #[gpui::test]
    async fn branch_row_icon_renders_smaller_than_the_section_icon(cx: &mut TestAppContext) {
        let (dir, _root, _remote_tip) = init_repo_with_remote_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let branch_icon = visual
            .debug_bounds("branch-icon-heads-master")
            .expect("the branch row renders its leading icon");
        let section_icon = visual
            .debug_bounds("branch-section-icon-local")
            .expect("the section header renders its icon");
        assert!(
            branch_icon.size.width < section_icon.size.width,
            "the branch-row icon should render slightly smaller than the section icon, \
             got branch {:?} vs section {:?}",
            branch_icon.size.width,
            section_icon.size.width
        );
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
    async fn sidebar_rows_pad_inside_their_background_without_gaps(cx: &mut TestAppContext) {
        let (dir, _main_tip, _feature_tip) = init_repo_with_unmerged_branch_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let header = visual
            .debug_bounds("branch-section-local")
            .expect("the Local section header renders");
        let feature = visual
            .debug_bounds("branch-row-heads-feature")
            .expect("the feature branch row renders");
        // Master's tip is the default selection on open, so its row renders
        // with the selected treatment.
        let master = visual
            .debug_bounds("selected-branch-row-heads-master")
            .expect("the master branch row renders");

        // The breathing room is padding inside each row's border + background:
        // the row box grows ~2px so the highlight fills it, and adjacent rows —
        // group headings included — stay flush rather than showing a gap.
        let row_height = feature.size.height;
        assert!(
            row_height > px(FILE_TREE_ROW_HEIGHT + 1.5)
                && row_height < px(FILE_TREE_ROW_HEIGHT + 2.5),
            "each row carries 1px of inner padding top and bottom, growing it ~2px, got {row_height:?}"
        );

        let header_to_feature = feature.origin.y - (header.origin.y + header.size.height);
        let feature_to_master = master.origin.y - (feature.origin.y + feature.size.height);
        assert!(
            header_to_feature > px(-0.5) && header_to_feature < px(0.5),
            "heading and first row sit flush; the spacing is inside the row, got {header_to_feature:?}"
        );
        assert!(
            feature_to_master > px(-0.5) && feature_to_master < px(0.5),
            "adjacent branch rows sit flush, got {feature_to_master:?}"
        );
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

    #[gpui::test]
    async fn hovering_a_sidebar_row_does_not_invalidate_the_cached_model(cx: &mut TestAppContext) {
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
        let baseline_rows = window
            .update(cx, |app, _window, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let _ = app.sidebar_rows(repo, "");
                app.sidebar_rows_recompute_count()
            })
            .expect("baseline build");

        // Move the cursor over a branch row. Hover state is not part of
        // `SidebarRowsSignature`, so a hover must not invalidate the cached
        // model: the next `sidebar_rows` call cache-hits and the recompute
        // count stays put. (This guards the cache signature only. It does not
        // assert anything about `Render::render`: gpui re-renders the root view
        // once when the hovered hitbox set changes over the row's kept pure
        // `.hover(...)` styling, independent of any `cx.notify()` in our code,
        // so a render-count probe cannot isolate a re-introduced notify here.)
        let row = visual
            .debug_bounds("selected-branch-row-heads-master")
            .expect("branch row renders");
        visual.simulate_mouse_move(row.center(), None, Modifiers::none());
        cx.run_until_parked();

        let after_rows = window
            .update(cx, |app, _window, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let _ = app.sidebar_rows(repo, "");
                app.sidebar_rows_recompute_count()
            })
            .expect("build after hover");
        assert_eq!(
            baseline_rows, after_rows,
            "hovering a row must not invalidate the cached sidebar model"
        );
    }

    #[gpui::test]
    async fn typing_a_query_filters_the_branch_rows(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        // Type "alpha" into the sidebar filter.
        window
            .update(cx, |app, window, cx| {
                app.filter_input
                    .update(cx, |state, cx| state.set_value("alpha", window, cx));
            })
            .expect("set filter query");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-row-heads-features-alpha")
            .expect("the matching branch row remains");
        assert!(
            visual.debug_bounds("branch-row-heads-master").is_none(),
            "a non-matching branch row is filtered out"
        );
    }

    #[gpui::test]
    async fn highlighted_rows_still_render_their_labels(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // "fal" spans folder->leaf: f@0 and a@2 fall in the "features"
                // folder segment, and the final "l" matches inside the "alpha"
                // leaf, so both the folder and leaf rows get a highlight.
                app.filter_input
                    .update(cx, |state, cx| state.set_value("fal", window, cx));
            })
            .expect("open and filter with a cross-segment match");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-folder-heads-features")
            .expect("the matched folder row renders with a highlighted segment");
        visual
            .debug_bounds("branch-row-heads-features-alpha")
            .expect("the matched leaf row renders with a highlighted segment");
    }

    #[gpui::test]
    async fn clearing_the_query_restores_prior_collapse_state(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                // Collapse the features folder before filtering.
                app.toggle_branch_folder("heads/features".to_string(), cx);
                app.filter_input
                    .update(cx, |state, cx| state.set_value("alpha", window, cx));
            })
            .expect("collapse then filter");
        cx.run_until_parked();

        // While filtering, the collapsed folder is force-expanded.
        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-row-heads-features-alpha")
            .expect("filtering force-expands the collapsed folder");

        // Clear the query; the folder returns to collapsed, hiding descendants.
        window
            .update(cx, |app, window, cx| {
                app.filter_input
                    .update(cx, |state, cx| state.set_value("", window, cx));
            })
            .expect("clear query");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual
                .debug_bounds("branch-row-heads-features-alpha")
                .is_none(),
            "clearing the query restores the collapsed folder's hidden descendants"
        );
        visual
            .debug_bounds("branch-folder-heads-features")
            .expect("the collapsed folder row itself remains");
    }

    #[gpui::test]
    async fn a_query_matching_nothing_shows_the_empty_message(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.filter_input
                    .update(cx, |state, cx| state.set_value("zzzznomatch", window, cx));
            })
            .expect("open and filter to nothing");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("branch-filter-empty")
            .expect("a no-match query shows the empty message");
    }

    #[gpui::test]
    async fn clear_button_appears_only_with_a_query_and_clears_it(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
            })
            .expect("open repository");
        cx.run_until_parked();

        // With no query, the clear button is absent.
        let mut visual = VisualTestContext::from_window(*window, cx);
        assert!(
            visual.debug_bounds("branch-filter-clear").is_none(),
            "the clear button is hidden while the query is empty"
        );

        // Typing a query reveals the clear button.
        window
            .update(cx, |app, window, cx| {
                app.filter_input
                    .update(cx, |state, cx| state.set_value("alpha", window, cx));
            })
            .expect("set filter query");
        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let clear = visual
            .debug_bounds("branch-filter-clear")
            .expect("the clear button appears once the query has content");
        visual.simulate_click(clear.center(), Modifiers::none());
        cx.run_until_parked();

        window
            .read_with(cx, |app, cx| {
                assert_eq!(
                    app.filter_input.read(cx).value().as_ref(),
                    "",
                    "clicking the clear button empties the query"
                );
            })
            .expect("read filter value after clear");
    }

    #[gpui::test]
    async fn reopening_a_repository_clears_the_filter_query(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.filter_input
                    .update(cx, |state, cx| state.set_value("alpha", window, cx));
                app.open_repository_at(path, window, cx);
            })
            .expect("open, filter, reopen");
        cx.run_until_parked();

        window
            .update(cx, |app, _window, cx| {
                assert_eq!(
                    app.filter_input.read(cx).value().as_ref(),
                    "",
                    "reopening a repository resets the filter query"
                );
            })
            .expect("read filter value");
    }

    #[gpui::test]
    async fn filtering_alone_does_not_change_the_graph(cx: &mut TestAppContext) {
        let (dir, _master_tip, _alpha_tip) = init_repo_with_slash_named_branches();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);

                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let before = visible_commits(repo, &app.hidden_branches)
                    .iter()
                    .map(|c| c.sha.clone())
                    .collect::<Vec<_>>();
                let selection_before = app.selection.clone();

                app.filter_input
                    .update(cx, |state, cx| state.set_value("alpha", window, cx));

                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected RepoOpen mode");
                };
                let after = visible_commits(repo, &app.hidden_branches)
                    .iter()
                    .map(|c| c.sha.clone())
                    .collect::<Vec<_>>();

                assert_eq!(before, after, "the filter must not change visible commits");
                assert_eq!(
                    selection_before, app.selection,
                    "the filter must not change the selection"
                );
            })
            .expect("filter and compare graph state");
    }
}
