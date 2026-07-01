//! File-tree rendering and model building: row/icon/indent-guide rendering for
//! the changed-file tree, plus the helpers that fold a flat changed-file list
//! into a collapsible folder tree. Extracted from `app.rs`; see
//! docs/adr/0002-project-layout.md.

use super::*;

pub(crate) fn change_kind_border(kind: repo::ChangeKind) -> gpui::Rgba {
    match kind {
        repo::ChangeKind::Added => rgb(0xb8f77a),
        repo::ChangeKind::Modified => rgb(0x7da4ff),
        repo::ChangeKind::Deleted => rgb(0xff5f78),
        repo::ChangeKind::Renamed => rgb(0xf3d36b),
    }
}

pub(crate) fn change_kind_text(kind: repo::ChangeKind) -> gpui::Rgba {
    match kind {
        repo::ChangeKind::Added => rgb(0xb8f77a),
        repo::ChangeKind::Modified => rgb(0x7da4ff),
        repo::ChangeKind::Deleted => rgb(0xff5f78),
        repo::ChangeKind::Renamed => rgb(0xf3d36b),
    }
}

pub(crate) fn render_file_tree_folder_icon(
    path_fragment: &str,
    collapsed: bool,
    color: gpui::Rgba,
) -> gpui::Div {
    let state = if collapsed { "closed" } else { "open" };
    let selector = format!("file-tree-folder-icon-{state}-{path_fragment}");

    let icon = div()
        .relative()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .flex_none()
        .debug_selector(move || selector.clone());

    if collapsed {
        icon.child(render_file_tree_icon_part(
            format!("file-tree-folder-icon-closed-body-{path_fragment}"),
            1.,
            6.,
            14.,
            8.,
            color,
        ))
        .child(render_file_tree_icon_part(
            format!("file-tree-folder-icon-closed-tab-{path_fragment}"),
            2.,
            3.,
            7.,
            4.,
            color,
        ))
    } else {
        icon.child(render_file_tree_open_folder_outline(
            format!("file-tree-folder-icon-open-outline-{path_fragment}"),
            color,
        ))
    }
}

pub(crate) fn render_file_tree_icon_part(
    selector: String,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    color: gpui::Rgba,
) -> gpui::Div {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .rounded(px(1.5))
        .border_1()
        .border_color(color)
        .debug_selector(move || selector.clone())
}

pub(crate) fn render_file_tree_open_folder_outline(
    selector: String,
    color: gpui::Rgba,
) -> gpui::Div {
    div()
        .absolute()
        .left_0()
        .top_0()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .debug_selector(move || selector.clone())
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let point_at = |x, y| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                    let mut outline = PathBuilder::stroke(px(1.35));

                    outline.move_to(point_at(2.5, 8.));
                    outline.line_to(point_at(2.5, 5.));
                    outline.line_to(point_at(6.5, 5.));
                    outline.line_to(point_at(8., 7.));
                    outline.line_to(point_at(13., 7.));
                    outline.line_to(point_at(13.8, 9.));

                    outline.move_to(point_at(1.5, 9.));
                    outline.line_to(point_at(14.5, 9.));
                    outline.line_to(point_at(13., 14.));
                    outline.line_to(point_at(2.5, 14.));
                    outline.line_to(point_at(1.5, 9.));

                    if let Ok(path) = outline.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .w_full()
            .h_full(),
        )
}

pub(crate) fn render_file_tree_file_icon(selector: String, color: gpui::Rgba) -> gpui::Div {
    let folded_corner_selector = format!("{selector}-folded-corner");

    div()
        .relative()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .flex_none()
        .debug_selector(move || selector.clone())
        .child(
            div()
                .absolute()
                .left(px(3.))
                .top(px(2.))
                .w(px(10.))
                .h(px(12.))
                .rounded(px(1.5))
                .border_1()
                .border_color(color),
        )
        .child(
            div()
                .absolute()
                .left(px(9.))
                .top(px(2.))
                .w(px(4.))
                .h(px(4.))
                .border_1()
                .border_color(color)
                .debug_selector(move || folded_corner_selector.clone()),
        )
}

pub(crate) fn render_file_tree_indent_guides(depth: usize, path_fragment: &str) -> gpui::Div {
    let guides = (0..depth)
        .map(|level| {
            let selector = format!("file-tree-indent-guide-{path_fragment}-{level}");
            div()
                .absolute()
                .left(px((level + 1) as f32 * FILE_TREE_INDENT_WIDTH
                    + if level > 0 {
                        FILE_TREE_GUIDE_TO_ITEM_GAP
                    } else {
                        0.
                    }
                    - FILE_TREE_INDENT_GUIDE_WIDTH / 2.))
                .top_0()
                .w(px(FILE_TREE_INDENT_GUIDE_WIDTH))
                .h(px(FILE_TREE_ROW_HEIGHT))
                .bg(rgb(0x2b383f))
                .debug_selector(move || selector.clone())
        })
        .collect::<Vec<_>>();

    div()
        .relative()
        .flex_none()
        .w(px(file_tree_indent_guides_width(depth)))
        .min_h(px(FILE_TREE_ROW_HEIGHT))
        .children(guides)
}

pub(crate) fn file_tree_indent_guides_width(depth: usize) -> f32 {
    depth as f32 * FILE_TREE_INDENT_WIDTH
        + if depth > 0 {
            FILE_TREE_GUIDE_TO_ITEM_GAP
        } else {
            0.
        }
}

pub(crate) fn render_file_tree_file_name(
    selector: String,
    display_name: &str,
    deleted: bool,
    deleted_strike_selector: String,
) -> gpui::Div {
    div()
        .relative()
        .flex()
        .items_center()
        .text_color(rgb(0xe6eef0))
        .text_size(px(FILE_TREE_TEXT_SIZE))
        .line_height(px(FILE_TREE_ROW_TEXT_LINE_HEIGHT))
        .font_family(MONO_FONT_FAMILY)
        .whitespace_nowrap()
        .debug_selector(move || selector.clone())
        .child(display_name.to_string())
        .when(deleted, |label| {
            label.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(10.))
                    .h(px(1.))
                    .bg(rgb(0xe6eef0))
                    .debug_selector(move || deleted_strike_selector.clone()),
            )
        })
}

pub(crate) fn render_change_status_icon(
    kind: repo::ChangeKind,
    selector: String,
    icon_selector: String,
) -> gpui::Div {
    let color = change_kind_text(kind);
    let glyph: AnyElement = match kind {
        repo::ChangeKind::Added => Icon::new(LucideIcon::SquarePlus)
            .text_color(color)
            .size(px(FILE_TREE_STATUS_ICON_SIZE))
            .into_any_element(),
        repo::ChangeKind::Deleted => Icon::new(LucideIcon::SquareMinus)
            .text_color(color)
            .size(px(FILE_TREE_STATUS_ICON_SIZE))
            .into_any_element(),
        repo::ChangeKind::Modified => Icon::new(LucideIcon::SquareDot)
            .text_color(color)
            .size(px(FILE_TREE_STATUS_ICON_SIZE))
            .into_any_element(),
        repo::ChangeKind::Renamed => div()
            .text_size(px(FILE_TREE_BADGE_TEXT_SIZE))
            .font_family(MONO_FONT_FAMILY)
            .text_color(color)
            .child("R")
            .into_any_element(),
    };

    let mut marker = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(FILE_TREE_STATUS_ICON_SIZE))
        .h(px(FILE_TREE_STATUS_ICON_SIZE))
        .flex_none()
        .debug_selector(move || icon_selector.clone());
    // The Lucide square-* glyphs draw their own outline, so only the rename
    // marker still needs a hand-drawn bordered box around its letter.
    if matches!(kind, repo::ChangeKind::Renamed) {
        marker = marker
            .border_1()
            .rounded(px(2.))
            .border_color(change_kind_border(kind));
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(FILE_TREE_FOLDER_ICON_SIZE))
        .h(px(FILE_TREE_FOLDER_ICON_SIZE))
        .flex_none()
        .debug_selector(move || selector.clone())
        .child(marker.child(glyph))
}

pub(crate) fn render_file_diff_stat(selector: String, stats: repo::LineStats) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap_2()
        .w(px(FILE_TREE_DIFF_STAT_WIDTH))
        .flex_none()
        .font_family(MONO_FONT_FAMILY)
        .text_size(px(FILE_TREE_DIFF_STAT_TEXT_SIZE))
        .debug_selector(move || selector.clone())
        .child(
            div()
                .text_color(rgb(0xb8f77a))
                .child(format!("+ {}", stats.added)),
        )
        .child(
            div()
                .text_color(rgb(0xff5f78))
                .child(format!("- {}", stats.removed)),
        )
}

pub(crate) fn insert_file_tree_entry(root: &mut FileTreeBranch, entry: FileListEntry) {
    let path = entry.path().to_string();
    let mut parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some(file_name) = parts.pop() else {
        return;
    };

    let mut branch = root;
    for folder in parts {
        branch = branch.folders.entry(folder.to_string()).or_default();
    }

    branch.files.push(FileTreeLeaf {
        name: file_name.to_string(),
        entry,
    });
    branch.files.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.entry.path().cmp(right.entry.path()))
    });
}

/// Collect every folder path that is an ancestor of a changed file. These are
/// the folders that lead to the diff, so they stay expanded by default even in
/// "all files" mode.
pub(crate) fn changed_file_ancestor_paths(entries: &[FileListEntry]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();

    for entry in entries {
        if !matches!(entry, FileListEntry::Changed(_)) {
            continue;
        }

        let mut parts = entry
            .path()
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        parts.pop();

        let mut prefix = String::new();
        for part in parts {
            if prefix.is_empty() {
                prefix = part.to_string();
            } else {
                prefix = format!("{prefix}/{part}");
            }
            paths.insert(prefix.clone());
        }
    }

    paths
}

/// Walk every folder in the tree (regardless of collapse state) recording its
/// path and whether it collapses by default. Mirrors the default-collapse logic
/// in [`append_file_tree_rows`] so bulk collapse/expand stays consistent.
pub(crate) fn collect_file_tree_folder_defaults(
    branch: &FileTreeBranch,
    prefix: &str,
    collapse_unchanged_by_default: bool,
    changed_ancestor_paths: &BTreeSet<String>,
    folders: &mut Vec<(String, bool)>,
) {
    for (name, child) in &branch.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let collapsed_by_default =
            collapse_unchanged_by_default && !changed_ancestor_paths.contains(&path);
        folders.push((path.clone(), collapsed_by_default));
        collect_file_tree_folder_defaults(
            child,
            &path,
            collapse_unchanged_by_default,
            changed_ancestor_paths,
            folders,
        );
    }
}

pub(crate) fn append_file_tree_rows(
    branch: &FileTreeBranch,
    depth: usize,
    prefix: &str,
    collapsed_paths: &BTreeSet<String>,
    collapse_unchanged_by_default: bool,
    changed_ancestor_paths: &BTreeSet<String>,
    rows: &mut Vec<FileTreeRow>,
) {
    for (name, child) in &branch.folders {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        // A folder is collapsed by default when it leads to no changed files in
        // "all files" mode. `collapsed_paths` records the user's manual toggles
        // and flips the folder away from its default state.
        let collapsed_by_default =
            collapse_unchanged_by_default && !changed_ancestor_paths.contains(&path);
        let collapsed = collapsed_by_default ^ collapsed_paths.contains(&path);

        rows.push(FileTreeRow::Folder {
            name: name.clone(),
            path: path.clone(),
            depth,
            collapsed,
        });

        if !collapsed {
            append_file_tree_rows(
                child,
                depth + 1,
                &path,
                collapsed_paths,
                collapse_unchanged_by_default,
                changed_ancestor_paths,
                rows,
            );
        }
    }

    for leaf in &branch.files {
        rows.push(FileTreeRow::File {
            name: leaf.name.clone(),
            entry: leaf.entry.clone(),
            depth,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use gpui::{font, px, TestAppContext, VisualTestContext};

    #[gpui::test]
    async fn file_tree_scrolls_both_axes(cx: &mut TestAppContext) {
        use gpui::px;

        let (window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // Query bounds to ensure a layout pass has occurred, then read scroll state.
        visual
            .debug_bounds("changed-files-scroll")
            .expect("changed-files-scroll must be rendered in changeset view");
        cx.run_until_parked();

        let v_max = window
            .read_with(cx, |app, _cx| app.file_tree_scroll.max_offset())
            .expect("v max");
        let h_max = window
            .read_with(cx, |app, _cx| app.file_tree_hscroll.max_offset())
            .expect("h max");
        assert!(
            v_max.height > px(0.),
            "should overflow vertically; v_max {v_max:?}"
        );
        assert!(
            h_max.width > px(0.),
            "should overflow horizontally; h_max {h_max:?}"
        );
    }

    #[gpui::test]
    async fn file_tree_rows_are_uniform_width(cx: &mut TestAppContext) {
        use gpui::px;

        let (_window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // The folder row "deeply" (index 0) has short content. The fixture's
        // deep_dir has 7 components, so folder rows occupy indices 0-6; the first
        // changed-file row is at index 7. Rows now live in the path pane only;
        // they must be equal width and wider than the path pane's own width
        // (proving they fill the full scrolled content width, not the viewport).
        let pane = visual
            .debug_bounds("changed-files-path-pane")
            .expect("path pane bounds");
        // NOTE: "file-tree-folder-deeply" is the top-level folder at index 0.
        // "changed-file-row-7" is index 7 because deep_dir
        // ("deeply/nested/directory/structure/that/keeps/going") has exactly 7
        // path components, placing folder rows at indices 0–6 and the first file
        // row at index 7. If deep_dir in init_repo_with_deeply_nested_long_paths
        // is ever changed to a path with a different component count, this index
        // must be updated to match.
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-deeply")
            .expect("top-level folder row must be rendered");
        let file_bounds = visual
            .debug_bounds("changed-file-row-7")
            .expect("first changed-file row (index 7, after 7 folder levels) must be rendered");

        assert!(
            (folder_bounds.size.width - file_bounds.size.width).abs() < px(1.),
            "rows must be uniform width: {:?} vs {:?}",
            folder_bounds.size.width,
            file_bounds.size.width
        );
        assert!(
            folder_bounds.size.width > pane.size.width,
            "rows should fill the full scrolled width, not just the pane"
        );
    }

    #[gpui::test]
    async fn diff_stats_stay_frozen_during_horizontal_scroll(cx: &mut TestAppContext) {
        use gpui::{point, px};

        let (window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        let before = visual
            .debug_bounds("changed-file-gutter-7")
            .expect("stat gutter cell bounds before scroll");

        window
            .update(cx, |app, _window, _cx| {
                app.file_tree_hscroll.set_offset(point(px(-200.), px(0.)));
            })
            .expect("scroll path pane horizontally");
        cx.run_until_parked();

        let after = visual
            .debug_bounds("changed-file-gutter-7")
            .expect("stat gutter cell bounds after scroll");

        assert!(
            (before.origin.x - after.origin.x).abs() < px(1.),
            "diff-stat gutter should not move horizontally; before {:?} after {:?}",
            before.origin.x,
            after.origin.x
        );

        let max = window
            .read_with(cx, |app, _cx| app.file_tree_hscroll.max_offset())
            .expect("read hscroll max");
        assert!(
            max.width > px(0.),
            "path pane should overflow horizontally; max {max:?}"
        );
    }

    #[gpui::test]
    async fn file_tree_scrollbar_reveals_on_panel_hover(cx: &mut TestAppContext) {
        use gpui::{point, px, Modifiers};

        // Reuse the Task 1 setup helper (deeply-nested changeset, 360×200 window).
        let (_window, mut visual) = open_deeply_nested_changeset_at_360x200(cx);

        // Not hovered: no scrollbar overlay is rendered.
        assert!(
            visual.debug_bounds("file-tree-scrollbar").is_none(),
            "scrollbar overlay should be hidden until the panel is hovered"
        );

        // Move the cursor into the file-tree panel.
        let panel = visual
            .debug_bounds("changed-files")
            .expect("file tree panel bounds");
        visual.simulate_mouse_move(panel.center(), None, Modifiers::default());
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("file-tree-scrollbar").is_some(),
            "scrollbar overlay should appear while the cursor is over the panel"
        );

        // Move the cursor outside the panel; the overlay should disappear.
        visual.simulate_mouse_move(point(px(-10.), px(-10.)), None, Modifiers::default());
        cx.run_until_parked();

        assert!(
            visual.debug_bounds("file-tree-scrollbar").is_none(),
            "scrollbar overlay should disappear when the cursor leaves the panel"
        );
    }

    #[gpui::test]
    async fn toggling_all_files_shows_unchanged_files_and_opens_read_only_content(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        let unchanged_bounds = visual
            .debug_bounds("unchanged-file-row-1")
            .expect("unchanged file row debug bounds");
        visual.simulate_click(unchanged_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-read-only-content")
            .expect("read-only file content debug bounds");

        window
            .read_with(cx, |app, _cx| {
                assert_eq!(
                    app.workspace
                        .active_item(0)
                        .map(|item| item.path().to_string()),
                    Some("context.txt".to_string()),
                );
                assert_eq!(
                    app.file_tree_highlight_path,
                    Some("context.txt".to_string()),
                );
            })
            .expect("read selected context file");
    }

    #[gpui::test]
    async fn all_files_mode_aligns_unchanged_and_changed_file_icons_at_the_same_depth(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        let changed_icon_bounds = visual
            .debug_bounds("changed-file-kind-src-changed.txt")
            .expect("changed file icon debug bounds");
        let unchanged_icon_bounds = visual
            .debug_bounds("unchanged-file-icon-src-context.txt")
            .expect("unchanged file icon debug bounds");

        assert_eq!(
            unchanged_icon_bounds.origin.x, changed_icon_bounds.origin.x,
            "unchanged files should use the same icon slot as changed files at the same depth"
        );
    }

    #[gpui::test]
    async fn file_list_renders_nested_paths_as_tree_folders(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("file-tree-folder-src")
            .expect("src folder debug bounds");
        visual
            .debug_bounds("changed-file-row-1")
            .expect("nested changed file row debug bounds");
    }

    #[gpui::test]
    async fn file_tree_rows_render_icons_status_icons_and_diff_stats(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_nested_line_stat_changes();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "src/notes.txt");
                    assert_eq!(changeset.files[0].line_stats.added, 2);
                    assert_eq!(changeset.files[0].line_stats.removed, 1);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read changeset line stats");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let repo_icon_bounds = visual
            .debug_bounds("file-tree-folder-icon-open-repo-root")
            .expect("repo root folder icon debug bounds");
        let folder_icon_bounds = visual
            .debug_bounds("file-tree-folder-icon-open-src")
            .expect("folder icon debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-outline-src")
            .expect("open folder outline debug bounds");
        let root_guide_bounds = visual
            .debug_bounds("file-tree-indent-guide-src-notes.txt-0")
            .expect("root-level indent guide debug bounds");
        let guide_bounds = visual
            .debug_bounds("file-tree-indent-guide-src-notes.txt-1")
            .expect("nested file indent guide debug bounds");
        let changed_kind_bounds = visual
            .debug_bounds("changed-file-kind-src-notes.txt")
            .expect("changed file kind marker debug bounds");
        assert_eq!(
            root_guide_bounds.origin.x + root_guide_bounds.size.width / 2.,
            repo_icon_bounds.origin.x + repo_icon_bounds.size.width / 2.,
            "root guide should be centered under the repo root icon"
        );
        assert_eq!(
            guide_bounds.origin.x + guide_bounds.size.width / 2.,
            folder_icon_bounds.origin.x + folder_icon_bounds.size.width / 2.,
            "nested guide should be centered under its parent folder icon"
        );
        assert!(
            changed_kind_bounds.origin.x - (guide_bounds.origin.x + guide_bounds.size.width)
                >= px(7.),
            "nested file item should have breathing room after the guide"
        );
        visual
            .debug_bounds("changed-file-status-icon-src-notes.txt")
            .expect("changed file status icon debug bounds");
        let status_icon_bounds = visual
            .debug_bounds("changed-file-status-icon-src-notes.txt")
            .expect("changed file status icon debug bounds");
        assert_eq!(
            status_icon_bounds.size.width,
            px(FILE_TREE_STATUS_ICON_SIZE)
        );
        assert_eq!(
            status_icon_bounds.size.height,
            px(FILE_TREE_STATUS_ICON_SIZE)
        );
        visual
            .debug_bounds("changed-file-diff-stat-src-notes.txt")
            .expect("changed file diff stat debug bounds");
    }

    #[gpui::test]
    async fn file_tree_shows_repo_root_header_with_inline_controls(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_nested_line_stat_changes();
        let path = dir.path().to_path_buf();
        let repo_name = path
            .file_name()
            .expect("repo dir name")
            .to_string_lossy()
            .to_string();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path.clone(), window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let header_bounds = visual
            .debug_bounds("file-tree-repo-root")
            .expect("repo root header debug bounds");
        visual
            .debug_bounds(test_debug_selector(format!(
                "file-tree-repo-root-name-{}",
                repo_name.replace('/', "-")
            )))
            .expect("repo root name debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-repo-root")
            .expect("repo root folder icon debug bounds");

        // The controls are inline children of the header row, not a floating overlay.
        for selector in [
            "file-list-mode-toggle",
            "file-tree-collapse-all",
            "file-tree-expand-all",
        ] {
            let control_bounds = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing control bounds for {selector}"));
            assert!(
                header_bounds.contains(&control_bounds.center()),
                "{selector} should sit inside the repo root header row"
            );
        }

        // The first file row's diff stats are clear of the controls.
        let stat_bounds = visual
            .debug_bounds("changed-file-diff-stat-src-notes.txt")
            .expect("diff stat debug bounds");
        let toggle_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("toggle debug bounds");
        assert!(
            !stat_bounds.intersects(&toggle_bounds),
            "diff stats must not collide with the tree controls"
        );
    }

    #[test]
    fn file_tree_indent_width_is_compact() {
        assert_eq!(FILE_TREE_INDENT_WIDTH, 16.);
    }

    #[test]
    fn file_tree_density_matches_zed_reference_scale() {
        assert_eq!(FILE_TREE_ROW_HEIGHT, 24.);
        assert_eq!(FILE_TREE_TEXT_SIZE, 14.);
        assert_eq!(FILE_TREE_FOLDER_ICON_SIZE, 16.);
        assert_eq!(FILE_TREE_STATUS_ICON_SIZE, 14.);
    }

    #[test]
    fn mono_font_family_uses_installed_berkeley_mono_family() {
        assert_eq!(MONO_FONT_FAMILY, "BerkeleyMono Nerd Font");
    }

    #[gpui::test]
    async fn mono_font_family_resolves_without_fallback(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let requested_font = font(MONO_FONT_FAMILY);
            let font_id = cx.text_system().resolve_font(&requested_font);
            let resolved_font = cx
                .text_system()
                .get_font_for_id(font_id)
                .expect("resolved font should be cached");

            assert_eq!(resolved_font.family.as_ref(), MONO_FONT_FAMILY);
        });
    }

    #[gpui::test]
    async fn deleted_file_tree_rows_render_deletion_marker_and_struck_name(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_deleted_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open deleted file changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("changed-file-status-icon-obsolete.txt")
            .expect("deleted file status icon debug bounds");
        visual
            .debug_bounds("changed-file-deleted-strike-obsolete.txt")
            .expect("deleted file strike debug bounds");
    }

    #[gpui::test]
    async fn collapsing_file_tree_folder_persists_across_file_list_mode_toggle(
        cx: &mut TestAppContext,
    ) {
        let (dir, oid_hex) = init_repo_with_nested_changed_and_context_files();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-src")
            .expect("src folder debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-open-src")
            .expect("open folder icon debug bounds");
        visual.simulate_click(folder_bounds.center(), Modifiers::none());

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("file-tree-folder-icon-closed-src")
            .expect("closed folder icon debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-closed-body-src")
            .expect("closed folder body debug bounds");
        visual
            .debug_bounds("file-tree-folder-icon-closed-tab-src")
            .expect("closed folder tab debug bounds");

        window
            .read_with(cx, |app, _cx| {
                assert!(app.collapsed_file_tree_paths.contains("src"));
            })
            .expect("read collapsed folder state");

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let ReviewScreen::Changeset { changeset, .. } = &app.review_screen else {
                    panic!("expected changeset screen");
                };
                let rows = app
                    .file_list_entries(repo, changeset)
                    .map(|entries| app.file_tree_rows(entries))
                    .expect("file tree rows");

                assert_eq!(rows.len(), 1, "collapsed folder should hide children");
                assert!(matches!(
                    &rows[0],
                    FileTreeRow::Folder {
                        path,
                        collapsed: true,
                        ..
                    } if path == "src"
                ));
            })
            .expect("read collapsed tree rows");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let all_files_bounds = visual
            .debug_bounds("file-list-mode-toggle")
            .expect("all files toggle debug bounds");
        visual.simulate_click(all_files_bounds.center(), Modifiers::none());

        window
            .read_with(cx, |app, _cx| {
                let Mode::RepoOpen { repo } = &app.mode else {
                    panic!("expected repo open mode");
                };
                let ReviewScreen::Changeset { changeset, .. } = &app.review_screen else {
                    panic!("expected changeset screen");
                };
                let rows = app
                    .file_list_entries(repo, changeset)
                    .map(|entries| app.file_tree_rows(entries))
                    .expect("file tree rows");

                assert_eq!(
                    app.file_list_mode,
                    FileListMode::All,
                    "test should be in all-files mode"
                );
                assert_eq!(
                    rows.len(),
                    1,
                    "collapsed folder should hide all file children after toggling mode"
                );
                assert!(matches!(
                    &rows[0],
                    FileTreeRow::Folder {
                        path,
                        collapsed: true,
                        ..
                    } if path == "src"
                ));
            })
            .expect("read collapsed all-files tree rows");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let folder_bounds = visual
            .debug_bounds("file-tree-folder-src")
            .expect("src folder debug bounds after toggling mode");
        visual.simulate_click(folder_bounds.center(), Modifiers::none());

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual
            .debug_bounds("changed-file-row-1")
            .expect("changed file child after expanding folder");
        visual
            .debug_bounds("unchanged-file-row-2")
            .expect("unchanged file child after expanding folder");
    }

    #[gpui::test]
    async fn all_files_mode_collapses_folders_without_changes_by_default(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, _cx| {
                app.file_list_mode = FileListMode::All;
                app.file_tree_rows(vec![
                    changed_file_entry("src/app/changed.rs"),
                    unchanged_file_entry("src/app/sibling/context.rs"),
                    unchanged_file_entry("docs/readme.md"),
                ])
            })
            .expect("compute all-files tree rows");

        assert!(
            !folder_collapsed(&rows, "src"),
            "folders on a changed path stay expanded"
        );
        assert!(
            !folder_collapsed(&rows, "src/app"),
            "folders on a changed path stay expanded at every depth"
        );
        assert!(
            folder_collapsed(&rows, "src/app/sibling"),
            "sibling folder without changes is collapsed by default"
        );
        assert!(
            folder_collapsed(&rows, "docs"),
            "unrelated folder without changes is collapsed by default"
        );
        assert!(
            rows.iter().any(|row| row.path() == "src/app/changed.rs"),
            "changed file stays visible"
        );
        assert!(
            !rows.iter().any(|row| row.path() == "docs/readme.md"),
            "collapsed folder hides its files"
        );
    }

    #[gpui::test]
    async fn toggle_file_list_mode_flips_between_changed_and_all(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        window
            .update(cx, |app, _window, cx| {
                assert_eq!(app.file_list_mode, FileListMode::Changed);
                app.toggle_file_list_mode(cx);
                assert_eq!(app.file_list_mode, FileListMode::All);
                app.toggle_file_list_mode(cx);
                assert_eq!(app.file_list_mode, FileListMode::Changed);
            })
            .expect("toggle file list mode");
    }

    #[gpui::test]
    async fn collapse_all_collapses_every_folder(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, cx| {
                app.file_list_mode = FileListMode::Changed;
                let entries = vec![
                    changed_file_entry("src/app/a.rs"),
                    changed_file_entry("src/b.rs"),
                    changed_file_entry("docs/c.md"),
                ];
                let folders = app.file_tree_folder_defaults(&entries);
                app.apply_folder_collapse(&folders, true, cx);
                app.file_tree_rows(entries)
            })
            .expect("collapse-all tree rows");

        assert!(folder_collapsed(&rows, "src"), "top-level folder collapsed");
        assert!(
            folder_collapsed(&rows, "docs"),
            "top-level folder collapsed"
        );
        assert!(
            !rows.iter().any(|row| row.path() == "src/app"),
            "collapsing the parent hides nested folders"
        );
        assert!(
            !rows.iter().any(|row| row.path() == "src/b.rs"),
            "collapsing hides files"
        );
    }

    #[gpui::test]
    async fn expand_all_expands_every_folder(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, cx| {
                app.file_list_mode = FileListMode::All;
                let entries = vec![
                    changed_file_entry("src/app/changed.rs"),
                    unchanged_file_entry("docs/nested/readme.md"),
                ];
                let folders = app.file_tree_folder_defaults(&entries);
                app.apply_folder_collapse(&folders, false, cx);
                app.file_tree_rows(entries)
            })
            .expect("expand-all tree rows");

        assert!(
            !folder_collapsed(&rows, "docs"),
            "expand-all overrides the default collapse of unchanged folders"
        );
        assert!(
            !folder_collapsed(&rows, "docs/nested"),
            "expand-all reaches nested folders"
        );
        assert!(
            rows.iter().any(|row| row.path() == "docs/nested/readme.md"),
            "expanded folders reveal their files"
        );
    }

    #[gpui::test]
    async fn changed_mode_keeps_all_folders_expanded_by_default(cx: &mut TestAppContext) {
        let window = add_app_window(cx);

        let rows = window
            .update(cx, |app, _window, _cx| {
                app.file_list_mode = FileListMode::Changed;
                app.file_tree_rows(vec![changed_file_entry("src/app/changed.rs")])
            })
            .expect("compute changed tree rows");

        assert!(
            !folder_collapsed(&rows, "src"),
            "changed mode leaves folders expanded by default"
        );
        assert!(!folder_collapsed(&rows, "src/app"));
    }
}
