//! File-diff rendering: prepared-diff construction, side-by-side and
//! single-side row building, syntax and word-level highlight attachment, and
//! per-line rendering. Extracted from `app.rs`; see docs/adr/0002-project-layout.md
//! and docs/specs covering the diff surface.

use super::*;

pub(crate) fn render_prepared_file_diff(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> AnyElement {
    match prepared {
        PreparedFileDiff::Single { side, rows } => {
            let side = *side;
            let selector = match side {
                repo::DiffSide::Old => "file-diff-side-old",
                repo::DiffSide::New => "file-diff-side-new",
            };
            let cells = rows
                .iter()
                .map(|row| match side {
                    repo::DiffSide::Old => row.old.clone(),
                    repo::DiffSide::New => row.new.clone(),
                })
                .collect::<Vec<_>>();

            render_file_diff_side(selector, cells, scroll.handle_for(side).clone())
                .into_any_element()
        }
        PreparedFileDiff::SideBySide { rows } => {
            let old_cells = rows.iter().map(|row| row.old.clone()).collect::<Vec<_>>();
            let new_cells = rows.iter().map(|row| row.new.clone()).collect::<Vec<_>>();

            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(render_file_diff_side(
                    "file-diff-side-old",
                    old_cells,
                    scroll.side_by_side.clone(),
                ))
                .child(div().w(px(1.)).flex_none().bg(rgb(0x2a2a2a)))
                .child(render_file_diff_side(
                    "file-diff-side-new",
                    new_cells,
                    scroll.side_by_side.clone(),
                ))
                .into_any_element()
        }
        PreparedFileDiff::Binary => render_binary_diff_placeholder(),
    }
}

pub(crate) fn render_binary_diff_placeholder() -> AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(rgb(0x171717))
        .id("file-diff-binary")
        .debug_selector(|| "file-diff-binary".to_string())
        .text_color(rgb(0x999999))
        .text_size(px(14.))
        .child("No textual diff is available for this file.")
        .into_any_element()
}

pub(crate) fn render_file_content(
    content: repo::FileContentBody,
    scroll: &FileDiffScroll,
    language: &str,
) -> AnyElement {
    match content {
        repo::FileContentBody::Text(text) => {
            let runs = diff_highlight::line_highlight_runs(&text, language);
            let cells = read_only_file_cells(&text)
                .into_iter()
                .map(|mut cell| {
                    attach_line_runs(&mut cell, &runs);
                    cell
                })
                .collect::<Vec<_>>();

            render_file_diff_side(
                "file-read-only-content",
                cells,
                scroll.handle_for(repo::DiffSide::New).clone(),
            )
            .into_any_element()
        }
        repo::FileContentBody::Binary => render_binary_diff_placeholder(),
    }
}

pub(crate) fn render_file_diff_error(message: String) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .bg(rgb(0x171717))
        .id("file-diff-error")
        .debug_selector(|| "file-diff-error".to_string())
        .text_color(rgb(0xfca5a5))
        .text_size(px(14.))
        .child(message)
        .into_any_element()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLineStatus {
    Unchanged,
    Added,
    Removed,
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffLineCell {
    pub(crate) line_number: Option<usize>,
    pub(crate) text: String,
    pub(crate) status: DiffLineStatus,
    pub(crate) highlights: Vec<(Range<usize>, HighlightStyle)>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffRow {
    pub(crate) old: DiffLineCell,
    pub(crate) new: DiffLineCell,
}

/// Identifies a cached diff: the changed file's path plus the commit and base
/// shas it was diffed against. Two changesets that touch the same path produce
/// different keys, so a stale entry is never served.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DiffCacheKey {
    pub(crate) path: String,
    pub(crate) commit_sha: String,
    pub(crate) base_sha: Option<String>,
}

/// A changed file's diff content with the expensive work already done: the line
/// diff computed, the per-side rows aligned, and syntax/word-level highlights
/// attached. This is what the diff cache holds so `render_changed_file_detail`
/// can rebuild its elements cheaply. Not `Eq`: the attached `HighlightStyle`
/// runs carry colors that only implement `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreparedFileDiff {
    Single {
        side: repo::DiffSide,
        rows: Vec<DiffRow>,
    },
    SideBySide {
        rows: Vec<DiffRow>,
    },
    Binary,
}

impl PreparedFileDiff {
    pub(crate) fn from_content(content: repo::FileDiffContent, language: &str) -> Self {
        match content {
            repo::FileDiffContent::Single { side, text } => {
                let runs = diff_highlight::line_highlight_runs(&text, language);
                let mut rows = single_side_diff_rows(side, &text);
                for row in &mut rows {
                    match side {
                        repo::DiffSide::Old => attach_line_runs(&mut row.old, &runs),
                        repo::DiffSide::New => attach_line_runs(&mut row.new, &runs),
                    }
                }
                PreparedFileDiff::Single { side, rows }
            }
            repo::FileDiffContent::SideBySide { old_text, new_text } => {
                let mut rows = side_by_side_diff_rows(&old_text, &new_text);
                attach_diff_highlights(&mut rows, &old_text, &new_text, language);
                PreparedFileDiff::SideBySide { rows }
            }
            repo::FileDiffContent::Binary => PreparedFileDiff::Binary,
        }
    }
}

pub(crate) fn single_side_diff_rows(side: repo::DiffSide, text: &str) -> Vec<DiffRow> {
    let status = match side {
        repo::DiffSide::Old => DiffLineStatus::Removed,
        repo::DiffSide::New => DiffLineStatus::Added,
    };

    content_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let visible = DiffLineCell {
                line_number: Some(index + 1),
                text: line,
                status,
                highlights: Vec::new(),
            };

            match side {
                repo::DiffSide::Old => DiffRow {
                    old: visible,
                    new: empty_diff_cell(),
                },
                repo::DiffSide::New => DiffRow {
                    old: empty_diff_cell(),
                    new: visible,
                },
            }
        })
        .collect()
}

pub(crate) fn side_by_side_diff_rows(old_text: &str, new_text: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(old_text, new_text);
    let old_lines = diff.old_slices();
    let new_lines = diff.new_slices();
    let mut rows = Vec::new();

    for op in diff.ops() {
        match op.tag() {
            DiffTag::Equal => {
                for (old_index, new_index) in op.old_range().zip(op.new_range()) {
                    rows.push(DiffRow {
                        old: diff_cell(old_index, old_lines[old_index], DiffLineStatus::Unchanged),
                        new: diff_cell(new_index, new_lines[new_index], DiffLineStatus::Unchanged),
                    });
                }
            }
            DiffTag::Delete => {
                for old_index in op.old_range() {
                    rows.push(DiffRow {
                        old: diff_cell(old_index, old_lines[old_index], DiffLineStatus::Removed),
                        new: empty_diff_cell(),
                    });
                }
            }
            DiffTag::Insert => {
                for new_index in op.new_range() {
                    rows.push(DiffRow {
                        old: empty_diff_cell(),
                        new: diff_cell(new_index, new_lines[new_index], DiffLineStatus::Added),
                    });
                }
            }
            DiffTag::Replace => {
                let old_indices = op.old_range().collect::<Vec<_>>();
                let new_indices = op.new_range().collect::<Vec<_>>();
                let len = old_indices.len().max(new_indices.len());

                for index in 0..len {
                    let old = old_indices
                        .get(index)
                        .map(|old_index| {
                            diff_cell(*old_index, old_lines[*old_index], DiffLineStatus::Removed)
                        })
                        .unwrap_or_else(empty_diff_cell);
                    let new = new_indices
                        .get(index)
                        .map(|new_index| {
                            diff_cell(*new_index, new_lines[*new_index], DiffLineStatus::Added)
                        })
                        .unwrap_or_else(empty_diff_cell);

                    rows.push(DiffRow { old, new });
                }
            }
        }
    }

    rows
}

/// Emphasis colors for word-level changes (One Dark red/green at ~25% alpha).
pub(crate) const DIFF_REMOVED_EMPHASIS: u32 = 0xe06c7540;
pub(crate) const DIFF_ADDED_EMPHASIS: u32 = 0x98c37940;

pub(crate) fn attach_line_runs(
    cell: &mut DiffLineCell,
    runs: &[Vec<(Range<usize>, HighlightStyle)>],
) {
    if let Some(line) = cell.line_number {
        cell.highlights = runs.get(line - 1).cloned().unwrap_or_default();
    }
}

pub(crate) fn attach_diff_highlights(
    rows: &mut [DiffRow],
    old_text: &str,
    new_text: &str,
    language: &str,
) {
    let old_runs = diff_highlight::line_highlight_runs(old_text, language);
    let new_runs = diff_highlight::line_highlight_runs(new_text, language);

    for row in rows.iter_mut() {
        attach_line_runs(&mut row.old, &old_runs);
        attach_line_runs(&mut row.new, &new_runs);

        if row.old.status == DiffLineStatus::Removed && row.new.status == DiffLineStatus::Added {
            let (old_emphasis, new_emphasis) =
                diff_highlight::inline_diff_ranges(&row.old.text, &row.new.text);
            let any_changes = !old_emphasis.is_empty() || !new_emphasis.is_empty();
            if any_changes
                && emphasis_is_subtle(&old_emphasis, row.old.text.len())
                && emphasis_is_subtle(&new_emphasis, row.new.text.len())
            {
                row.old.highlights = diff_highlight::merge_emphasis(
                    &row.old.highlights,
                    &old_emphasis,
                    rgba(DIFF_REMOVED_EMPHASIS).into(),
                );
                row.new.highlights = diff_highlight::merge_emphasis(
                    &row.new.highlights,
                    &new_emphasis,
                    rgba(DIFF_ADDED_EMPHASIS).into(),
                );
            }
        }
    }
}

/// Word-level emphasis only helps when it stays a small slice of the line;
/// a pair whose differences cover most of the line reads better as a
/// whole-line change. A side with no changed ranges passes trivially —
/// pure insertions/deletions still emphasize the other side.
pub(crate) fn emphasis_is_subtle(ranges: &[Range<usize>], line_len: usize) -> bool {
    if ranges.is_empty() {
        return true;
    }
    if line_len == 0 {
        return false;
    }
    let covered: usize = ranges.iter().map(|range| range.len()).sum();
    covered * 100 <= line_len * EMPHASIS_MAX_COVERAGE_PERCENT
}

/// Above this share of the line, word-level emphasis is noise.
pub(crate) const EMPHASIS_MAX_COVERAGE_PERCENT: usize = 60;

pub(crate) fn diff_cell(line_index: usize, line: &str, status: DiffLineStatus) -> DiffLineCell {
    DiffLineCell {
        line_number: Some(line_index + 1),
        text: trim_line_ending(line),
        status,
        highlights: Vec::new(),
    }
}

pub(crate) fn empty_diff_cell() -> DiffLineCell {
    DiffLineCell {
        line_number: None,
        text: String::new(),
        status: DiffLineStatus::Empty,
        highlights: Vec::new(),
    }
}

pub(crate) fn read_only_file_cells(text: &str) -> Vec<DiffLineCell> {
    content_lines(text)
        .into_iter()
        .enumerate()
        .map(|(index, line)| DiffLineCell {
            line_number: Some(index + 1),
            text: line,
            status: DiffLineStatus::Unchanged,
            highlights: Vec::new(),
        })
        .collect()
}

pub(crate) fn render_file_diff_side(
    selector: &'static str,
    cells: Vec<DiffLineCell>,
    scroll_handle: UniformListScrollHandle,
) -> impl IntoElement {
    uniform_list(selector, cells.len(), move |range, _window, _cx| {
        range
            .map(|index| render_file_diff_line(selector, index, cells[index].clone()))
            .collect::<Vec<_>>()
    })
    .flex_1()
    .h_full()
    .min_h_0()
    .min_w_0()
    .bg(rgb(0x171717))
    // Reserve the same 12px right gutter the pre-virtualization scroll area
    // held via `scrollbar_width`. `UniformList` does not implement
    // `StatefulInteractiveElement`, so that modifier is unavailable; right
    // padding insets the rows identically and keeps the diff's appearance.
    .pr(px(12.))
    .track_scroll(scroll_handle)
    .debug_selector(move || selector.to_string())
}

pub(crate) fn render_file_diff_line(
    pane_selector: &'static str,
    row_index: usize,
    cell: DiffLineCell,
) -> impl IntoElement {
    let line_number = cell
        .line_number
        .map(|line_number| line_number.to_string())
        .unwrap_or_default();
    let pane_offset = match pane_selector {
        "file-diff-side-old" => 0,
        "file-diff-side-new" => 1,
        _ => 2,
    };
    let id_index = row_index * 3 + pane_offset;
    let row_selector = diff_line_debug_selector(cell.status);
    let accent = diff_line_accent(cell.status);
    let has_text = !cell.text.is_empty();

    div()
        .flex()
        .h(px(DIFF_LINE_HEIGHT))
        .bg(diff_line_fill(cell.status))
        .id(("file-diff-line", id_index))
        .debug_selector(move || row_selector.to_string())
        .child(
            div()
                .w(px(3.))
                .flex_none()
                .when_some(accent, |bar, (color, accent_selector)| {
                    bar.bg(color)
                        .debug_selector(move || accent_selector.to_string())
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .w(px(48.))
                .pr_2()
                .flex_none()
                .text_color(rgb(0x666666))
                .text_size(px(12.))
                .line_height(px(DIFF_LINE_HEIGHT))
                .font_family("monospace")
                .debug_selector(move || diff_line_index_selector(pane_selector, row_index))
                .child(line_number),
        )
        .child(
            div()
                .flex()
                .items_center()
                .flex_1()
                .px_2()
                .min_w_0()
                .when(has_text, |content| {
                    content.child(
                        StyledText::new(cell.text)
                            .with_default_highlights(&diff_text_style(), cell.highlights),
                    )
                }),
        )
}

/// Per-row debug selector encoding the column side and row index, e.g.
/// `file-diff-line-new-12`. Called from a `debug_selector` closure, so it runs
/// only in test/test-support builds and only for rows the virtualized list
/// actually renders — which is what lets a test prove off-screen rows are absent.
pub(crate) fn diff_line_index_selector(pane_selector: &str, row_index: usize) -> String {
    let side = match pane_selector {
        "file-diff-side-old" => "old",
        "file-diff-side-new" => "new",
        _ => "single",
    };
    format!("file-diff-line-{side}-{row_index}")
}

/// Row height for one diff line: tall enough to contain the 12px monospace
/// glyphs without clipping, and the shared box that vertically centers the
/// gutter number against its code line.
pub(crate) const DIFF_LINE_HEIGHT: f32 = 20.;

/// Base text style for diff code lines; syntax runs override color per token.
pub(crate) fn diff_text_style() -> TextStyle {
    TextStyle {
        color: Hsla::from(rgb(0xabb2bf)),
        font_family: "monospace".into(),
        font_size: px(12.).into(),
        line_height: px(DIFF_LINE_HEIGHT).into(),
        ..TextStyle::default()
    }
}

/// One Dark red/green at ~9% alpha over the 0x171717 chrome; alignment gaps
/// hatch with a diagonal pattern like Zed's.
pub(crate) fn diff_line_fill(status: DiffLineStatus) -> Background {
    match status {
        DiffLineStatus::Unchanged => Hsla::from(rgb(0x171717)).into(),
        DiffLineStatus::Added => Hsla::from(rgba(0x98c37918)).into(),
        DiffLineStatus::Removed => Hsla::from(rgba(0xe06c7518)).into(),
        DiffLineStatus::Empty => pattern_slash(Hsla::from(rgba(0x26262680)), 1., 6.),
    }
}

pub(crate) fn diff_line_accent(status: DiffLineStatus) -> Option<(gpui::Rgba, &'static str)> {
    match status {
        DiffLineStatus::Added => Some((rgb(0x98c379), "file-diff-accent-added")),
        DiffLineStatus::Removed => Some((rgb(0xe06c75), "file-diff-accent-removed")),
        DiffLineStatus::Unchanged | DiffLineStatus::Empty => None,
    }
}

pub(crate) fn diff_line_debug_selector(status: DiffLineStatus) -> &'static str {
    match status {
        DiffLineStatus::Unchanged => "file-diff-row-unchanged",
        DiffLineStatus::Added => "file-diff-row-added",
        DiffLineStatus::Removed => "file-diff-row-removed",
        DiffLineStatus::Empty => "file-diff-row-empty",
    }
}

pub(crate) fn content_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.lines().map(str::to_string).collect()
}

pub(crate) fn trim_line_ending(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::*;
    use crate::repo::{ChangeKind, DiffSide};
    use gpui::{px, TestAppContext, VisualTestContext};

    #[test]
    fn line_diff_single_side_added_marks_lines_as_added() {
        let rows = single_side_diff_rows(DiffSide::New, "first\nsecond\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].new.status, DiffLineStatus::Added);
        assert_eq!(rows[0].new.line_number, Some(1));
        assert_eq!(rows[0].new.text, "first");
        assert_eq!(rows[0].old.status, DiffLineStatus::Empty);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
    }

    #[test]
    fn line_diff_single_side_deleted_marks_lines_as_removed() {
        let rows = single_side_diff_rows(DiffSide::Old, "first\nsecond\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[0].old.line_number, Some(1));
        assert_eq!(rows[0].old.text, "first");
        assert_eq!(rows[0].new.status, DiffLineStatus::Empty);
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
    }

    #[test]
    fn line_diff_side_by_side_aligns_equal_removed_and_added_rows() {
        let rows = side_by_side_diff_rows("same\nold\n", "same\nnew\n");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].old.status, DiffLineStatus::Unchanged);
        assert_eq!(rows[0].new.status, DiffLineStatus::Unchanged);
        assert_eq!(rows[0].old.text, "same");
        assert_eq!(rows[0].new.text, "same");
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
        assert_eq!(rows[1].old.text, "old");
        assert_eq!(rows[1].new.text, "new");
    }

    #[test]
    fn line_diff_side_by_side_pads_uneven_replacements() {
        let rows = side_by_side_diff_rows("one\ntwo\n", "one\nalpha\nbeta\n");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].old.status, DiffLineStatus::Removed);
        assert_eq!(rows[1].new.status, DiffLineStatus::Added);
        assert_eq!(rows[2].old.status, DiffLineStatus::Empty);
        assert_eq!(rows[2].new.status, DiffLineStatus::Added);
        assert_eq!(rows[2].new.text, "beta");
    }

    #[test]
    fn python_replace_rows_carry_syntax_and_word_level_highlights() {
        let old_text = "threshold = 10\n";
        let new_text = "threshold = None\n";
        let mut rows = side_by_side_diff_rows(old_text, new_text);
        attach_diff_highlights(&mut rows, old_text, new_text, "py");

        assert!(
            !rows[0].old.highlights.is_empty(),
            "old side should carry syntax runs"
        );
        assert!(
            rows[0]
                .new
                .highlights
                .iter()
                .any(|(_, style)| style.background_color.is_some()),
            "changed token on the new side should carry an emphasis background"
        );
    }

    #[test]
    fn unrelated_replace_rows_skip_word_level_emphasis() {
        let old_text = "alpha beta gamma\n";
        let new_text = "completely different text\n";
        let mut rows = side_by_side_diff_rows(old_text, new_text);
        attach_diff_highlights(&mut rows, old_text, new_text, "");

        assert!(
            rows[0]
                .new
                .highlights
                .iter()
                .all(|(_, style)| style.background_color.is_none()),
            "near-total rewrites should read as whole-line changes"
        );
    }

    #[test]
    fn pure_insertion_replace_rows_keep_word_level_emphasis() {
        let old_text = "return x\n";
        let new_text = "return x + 1\n";
        let mut rows = side_by_side_diff_rows(old_text, new_text);
        attach_diff_highlights(&mut rows, old_text, new_text, "");

        assert!(
            rows[0]
                .new
                .highlights
                .iter()
                .any(|(_, style)| style.background_color.is_some()),
            "an appended token should still be emphasized on the new side"
        );
        assert!(
            rows[0]
                .old
                .highlights
                .iter()
                .all(|(_, style)| style.background_color.is_none()),
            "the unchanged old side has nothing to emphasize"
        );
    }

    #[test]
    fn emphasis_subtlety_gate_boundary() {
        // Exactly 60% coverage passes; just over fails; empty side passes.
        // Use struct-literal form to avoid the single_range_in_vec_init Clippy lint.
        let r = |start, end| std::ops::Range::<usize> { start, end };
        assert!(emphasis_is_subtle(&[r(0, 6)], 10));
        assert!(!emphasis_is_subtle(&[r(0, 7)], 10));
        assert!(emphasis_is_subtle(&[], 0));
        assert!(!emphasis_is_subtle(&[r(0, 1)], 0));
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_text_diff_content(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
    }

    #[gpui::test]
    async fn added_file_diff_renders_only_the_new_side(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open added file changeset");

        cx.run_until_parked();

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "hello.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Added);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read added file changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("added line row debug bounds");
        assert!(
            visual.debug_bounds("file-diff-side-old").is_none(),
            "added file diff should not render an empty old-side pane"
        );
    }

    #[gpui::test]
    async fn deleted_file_diff_renders_only_the_old_side(cx: &mut TestAppContext) {
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

        window
            .read_with(cx, |app, _cx| match &app.review_screen {
                ReviewScreen::Changeset { changeset, .. } => {
                    assert_eq!(changeset.files.len(), 1);
                    assert_eq!(changeset.files[0].path, "obsolete.txt");
                    assert_eq!(changeset.files[0].kind, ChangeKind::Deleted);
                }
                ReviewScreen::Graph => panic!("expected changeset review screen"),
            })
            .expect("read deleted file changeset");

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed line row debug bounds");
        assert!(
            visual.debug_bounds("file-diff-side-new").is_none(),
            "deleted file diff should not render an empty new-side pane"
        );
    }

    #[gpui::test]
    async fn clicking_changed_file_renders_line_highlights(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_two_commits();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
            })
            .expect("open repo and select commit");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let open_bounds = visual
            .debug_bounds("open-changeset")
            .expect("open changeset debug bounds");
        visual.simulate_click(open_bounds.center(), Modifiers::none());

        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-row-removed")
            .expect("removed line row debug bounds");
        visual
            .debug_bounds("file-diff-row-added")
            .expect("added line row debug bounds");
    }

    #[gpui::test]
    async fn modified_file_diff_renders_status_accent_bars(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_python_change();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open python changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());

        visual
            .debug_bounds("file-diff-accent-removed")
            .expect("removed accent bar debug bounds");
        visual
            .debug_bounds("file-diff-accent-added")
            .expect("added accent bar debug bounds");
    }

    #[gpui::test]
    async fn scrolling_long_file_diff_moves_the_diff_scroll_area(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let max_offset = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_max_offset())
            .expect("read new diff scroll max offset");
        assert!(
            max_offset.height > px(0.),
            "long diff should exceed the visible diff scroll area"
        );

        let before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset before wheel");
        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });
        let after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset after wheel");

        assert!(
            after.y < before.y,
            "wheel scroll should move the diff content upward"
        );
    }

    #[gpui::test]
    async fn scrolling_new_side_of_side_by_side_diff_scrolls_old_side(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side debug bounds");
        let old_before = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset before wheel");
        let new_before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });

        let old_after = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset after wheel");
        let new_after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset after wheel");

        assert!(
            new_after.y < new_before.y,
            "wheel scroll should move the new side upward"
        );
        assert_ne!(
            old_after.y, old_before.y,
            "old side should move when the new side scrolls"
        );
        assert_eq!(
            old_after.y, new_after.y,
            "old side should stay aligned with new side"
        );
    }

    #[gpui::test]
    async fn scrolling_old_side_of_side_by_side_diff_scrolls_new_side(cx: &mut TestAppContext) {
        use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent};

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(320.)));

        let scroll_bounds = visual
            .debug_bounds("file-diff-side-old")
            .expect("old file diff side debug bounds");
        let old_before = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset before wheel");
        let new_before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset before wheel");

        visual.simulate_event(ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
            ..Default::default()
        });

        let old_after = window
            .read_with(cx, |app, _cx| app.file_diff_old_scroll_offset())
            .expect("read old diff scroll offset after wheel");
        let new_after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read new diff scroll offset after wheel");

        assert!(
            old_after.y < old_before.y,
            "wheel scroll should move the old side upward"
        );
        assert_ne!(
            new_after.y, new_before.y,
            "new side should move when the old side scrolls"
        );
        assert_eq!(
            old_after.y, new_after.y,
            "new side should stay aligned with old side"
        );
    }

    #[gpui::test]
    async fn diff_view_virtualizes_offscreen_rows(cx: &mut TestAppContext) {
        use gpui::size;

        let (dir, oid_hex) = init_repo_with_long_diff();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("long.txt".to_string(), cx);
            })
            .expect("open long diff");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        // The diff rewrites 160 lines, so the content (>=160 rows x 20px) far
        // exceeds this 320px-tall viewport (~16 rows visible).
        visual.simulate_resize(size(px(700.), px(320.)));

        // The first row is on screen and must be materialized.
        visual
            .debug_bounds("file-diff-line-new-0")
            .expect("first diff row should be materialized");

        // Row 150 sits ~3000px down, far past the viewport, and must NOT be
        // built while off screen. That asymmetry is the proof of virtualization.
        assert!(
            visual.debug_bounds("file-diff-line-new-150").is_none(),
            "row 150 is far below the viewport and must not be materialized"
        );
    }
}
