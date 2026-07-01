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
        PreparedFileDiff::Single { side, rows, .. } => {
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
        PreparedFileDiff::SideBySide { rows, .. } => {
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

/// The block the counter reports: the first block still at or below the top of
/// the viewport (`end_row >= topmost_row`). This reads correctly even when the
/// diff is scrolled to the bottom and a late block cannot reach the very top —
/// the block is still visible, so it is still reported. Falls back to the last
/// block when everything is scrolled above the top, and returns 0 for an empty
/// slice (callers only invoke this with at least one block).
pub(crate) fn current_block_index(blocks: &[ChangeBlock], topmost_row: usize) -> usize {
    blocks
        .iter()
        .position(|block| block.end_row >= topmost_row)
        .unwrap_or_else(|| blocks.len().saturating_sub(1))
}

/// The block to step forward to: the first block that begins strictly below the
/// anchor row. This is visibility-aware — a block below the current top is the
/// next stop even when the counter already reports it, so a step never skips an
/// unseen change. Wraps to the first block when no block begins below the
/// anchor. (The caller handles the bottom-clamp wrap, where a late block sits
/// below the anchor only because the viewport cannot scroll far enough.)
pub(crate) fn next_block_index(blocks: &[ChangeBlock], anchor_row: usize) -> usize {
    blocks
        .iter()
        .position(|block| block.start_row > anchor_row)
        .unwrap_or(0)
}

/// The block to step backward to: the last block that begins strictly above the
/// anchor row. Wraps to the last block when no block begins above the anchor
/// (e.g. stepping back from the first block or the top of the file). Returns 0
/// for an empty slice.
pub(crate) fn previous_block_index(blocks: &[ChangeBlock], anchor_row: usize) -> usize {
    blocks
        .iter()
        .rposition(|block| block.start_row < anchor_row)
        .unwrap_or_else(|| blocks.len().saturating_sub(1))
}

/// The index of the topmost row visible for a given vertical scroll offset. The
/// offset is zero at the top and grows negative as the content scrolls up, so
/// the row count above the fold is `-offset / row_height`, floored. Clamped to
/// the last row.
pub(crate) fn topmost_row_for_offset(offset_y: Pixels, row_count: usize) -> usize {
    if row_count == 0 {
        return 0;
    }
    let rows_above = (-offset_y / px(DIFF_LINE_HEIGHT)).floor().max(0.) as usize;
    rows_above.min(row_count - 1)
}

/// The scroll handle and row count that change-block navigation drives for a
/// prepared diff: the shared handle for a side-by-side diff, or the visible
/// side's handle for a single-side one. `None` for a binary diff, which has no
/// navigable rows.
pub(crate) fn change_block_scroll_target(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> Option<(UniformListScrollHandle, usize)> {
    match prepared {
        PreparedFileDiff::Single { side, rows, .. } => {
            Some((scroll.handle_for(*side).clone(), rows.len()))
        }
        PreparedFileDiff::SideBySide { rows, .. } => {
            Some((scroll.side_by_side.clone(), rows.len()))
        }
        PreparedFileDiff::Binary => None,
    }
}

/// The topmost visible row for a prepared diff at its current scroll position.
/// `None` for a binary diff.
pub(crate) fn change_block_topmost_row(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> Option<usize> {
    let (handle, row_count) = change_block_scroll_target(prepared, scroll)?;
    let offset_y = handle.0.borrow().base_handle.offset().y;
    Some(topmost_row_for_offset(offset_y, row_count))
}

/// The anchor row that next/previous navigation compares block starts against:
/// the topmost visible row plus the context margin, so a block resting at its
/// scrolled position (context rows above it) is treated as the current block
/// and a step moves off it. `None` for a binary diff.
pub(crate) fn change_block_anchor_row(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> Option<usize> {
    Some(change_block_topmost_row(prepared, scroll)? + CHANGE_BLOCK_CONTEXT_ROWS)
}

/// Whether the diff is scrolled to the bottom, where a late change block cannot
/// be brought fully to the top. Forward navigation treats this as "past the
/// last block" so the next step wraps to the first. Requires a real scroll
/// range, so a short diff that entirely fits is never considered at-bottom.
pub(crate) fn change_block_scrolled_to_bottom(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> bool {
    let Some((handle, _row_count)) = change_block_scroll_target(prepared, scroll) else {
        return false;
    };
    let state = handle.0.borrow();
    let max_height = state.base_handle.max_offset().height;
    let offset_y = state.base_handle.offset().y;
    max_height > px(0.) && offset_y <= -max_height + px(1.)
}

/// The block the reviewer is currently looking at, for a prepared diff at its
/// current scroll position. `None` when the diff has no change blocks.
pub(crate) fn current_change_block(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> Option<usize> {
    let blocks = prepared.blocks();
    if blocks.is_empty() {
        return None;
    }
    let topmost = change_block_topmost_row(prepared, scroll)?;
    Some(current_block_index(blocks, topmost))
}

/// The vertical scroll offset that places a block's `start_row` at the top of
/// the viewport, leaving up to `context_rows` of context above it. The offset
/// is negative because the content scrolls up. Clamped at the top of the file
/// via `saturating_sub`, so early blocks simply sit at the top.
pub(crate) fn scroll_offset_for_block_top(start_row: usize, context_rows: usize) -> Pixels {
    let top_row = start_row.saturating_sub(context_rows);
    -px(top_row as f32 * DIFF_LINE_HEIGHT)
}

/// Set a diff side's vertical scroll offset directly, preserving the horizontal
/// offset. Used to jump to a change block synchronously so the footer counter
/// reflects the new position in the same frame.
pub(crate) fn set_diff_scroll_top(handle: &UniformListScrollHandle, offset_y: Pixels) {
    let state = handle.0.borrow();
    let x = state.base_handle.offset().x;
    state.base_handle.set_offset(point(x, offset_y));
}

/// When a diff is first shown, scroll it to its first change block so the
/// reviewer lands on the change instead of the file's top. Consumes the
/// scroll's pending-focus flag (set on open), so it fires once per open and
/// leaves later manual scrolling untouched. A no-op for a diff with no blocks.
pub(crate) fn focus_first_change_block(prepared: &PreparedFileDiff, scroll: &FileDiffScroll) {
    if !scroll.take_pending_focus() {
        return;
    }
    let Some(first) = prepared.blocks().first() else {
        return;
    };
    let Some((handle, _row_count)) = change_block_scroll_target(prepared, scroll) else {
        return;
    };
    set_diff_scroll_top(
        &handle,
        scroll_offset_for_block_top(first.start_row, CHANGE_BLOCK_CONTEXT_ROWS),
    );
}

/// Height of the change-block navigation footer.
const CHANGE_BLOCK_FOOTER_HEIGHT: f32 = 28.;
const CHANGE_BLOCK_BUTTON_SIZE: f32 = 20.;
const CHANGE_BLOCK_ICON_SIZE: f32 = 14.;

/// The right-aligned footer bar for change-block navigation: a `Change N of M`
/// counter and up/down chevrons that jump to the previous/next change block.
/// `None` when the diff has no change blocks, so the caller renders nothing.
pub(crate) fn render_change_block_footer(
    prepared: &PreparedFileDiff,
    scroll: &FileDiffScroll,
) -> Option<AnyElement> {
    let total = prepared.blocks().len();
    let current = current_change_block(prepared, scroll)?;

    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap_2()
            .h(px(CHANGE_BLOCK_FOOTER_HEIGHT))
            .px_2()
            .border_t_1()
            .border_color(rgb(0x2a2a2a))
            .bg(rgb(0x1d1d1d))
            .id("change-block-footer")
            .debug_selector(|| "change-block-footer".to_string())
            .child(
                div()
                    .text_color(rgb(0x999999))
                    .text_size(px(12.))
                    .font_family(MONO_FONT_FAMILY)
                    .debug_selector(|| "change-block-label".to_string())
                    .child(format!("Change {} of {}", current + 1, total)),
            )
            .child(change_block_button(
                "change-block-prev",
                LucideIcon::ChevronUp,
                |window, cx| window.dispatch_action(Box::new(PreviousChangeBlock), cx),
            ))
            .child(change_block_button(
                "change-block-next",
                LucideIcon::ChevronDown,
                |window, cx| window.dispatch_action(Box::new(NextChangeBlock), cx),
            ))
            .into_any_element(),
    )
}

/// One chevron button in the change-block footer. Clicking it dispatches the
/// navigation action `dispatch` runs, so the button and keybinding share one
/// path.
fn change_block_button(
    selector: &'static str,
    icon: LucideIcon,
    dispatch: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(selector)
        .debug_selector(move || selector.to_string())
        .flex()
        .items_center()
        .justify_center()
        .w(px(CHANGE_BLOCK_BUTTON_SIZE))
        .h(px(CHANGE_BLOCK_BUTTON_SIZE))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(0x2a2a2a)))
        .on_click(move |_event: &gpui::ClickEvent, window, cx| dispatch(window, cx))
        .child(
            Icon::new(icon)
                .size(px(CHANGE_BLOCK_ICON_SIZE))
                .text_color(rgb(0x999999)),
        )
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

impl DiffRow {
    /// A row is "changed" when either side departs from `Unchanged`. `Empty`
    /// is the alignment counterpart of a change on the other side, so it
    /// counts as changed too.
    fn is_changed(&self) -> bool {
        self.old.status != DiffLineStatus::Unchanged || self.new.status != DiffLineStatus::Unchanged
    }
}

/// A navigable region of change: an inclusive range of row indices into the
/// flat rows vec. Endpoints are always changed rows; runs of changed rows
/// separated by no more than `CHANGE_BLOCK_MAX_GAP` unchanged rows merge into
/// a single block, so the merged-in context rows sit inside the span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChangeBlock {
    pub(crate) start_row: usize,
    pub(crate) end_row: usize,
}

/// Two runs of changed rows separated by this many unchanged "context" rows or
/// fewer merge into a single change block, mirroring git's default 3-line
/// context.
pub(crate) const CHANGE_BLOCK_MAX_GAP: usize = 3;

/// When navigating to a change block, leave up to this many context rows above
/// it so the reviewer sees a little of what precedes the change. Clamped
/// automatically at the top of the file.
pub(crate) const CHANGE_BLOCK_CONTEXT_ROWS: usize = 3;

/// Group the flat diff rows into navigable change blocks. Contiguous runs of
/// changed rows separated by `max_gap` or fewer unchanged rows merge into one
/// block; larger gaps split them. Returns blocks in row order.
pub(crate) fn change_blocks(rows: &[DiffRow], max_gap: usize) -> Vec<ChangeBlock> {
    let mut blocks: Vec<ChangeBlock> = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        if !row.is_changed() {
            continue;
        }

        match blocks.last_mut() {
            // Extend the open block when this change is within the gap of the
            // previous change (the unchanged rows between them number
            // `index - last.end_row - 1`).
            Some(last) if index - last.end_row - 1 <= max_gap => {
                last.end_row = index;
            }
            _ => blocks.push(ChangeBlock {
                start_row: index,
                end_row: index,
            }),
        }
    }

    blocks
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
        blocks: Vec<ChangeBlock>,
    },
    SideBySide {
        rows: Vec<DiffRow>,
        blocks: Vec<ChangeBlock>,
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
                let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);
                PreparedFileDiff::Single { side, rows, blocks }
            }
            repo::FileDiffContent::SideBySide { old_text, new_text } => {
                let mut rows = side_by_side_diff_rows(&old_text, &new_text);
                attach_diff_highlights(&mut rows, &old_text, &new_text, language);
                let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);
                PreparedFileDiff::SideBySide { rows, blocks }
            }
            repo::FileDiffContent::Binary => PreparedFileDiff::Binary,
        }
    }

    /// The navigable change blocks for this diff, in row order. Empty for a
    /// binary diff or a diff with no changes.
    pub(crate) fn blocks(&self) -> &[ChangeBlock] {
        match self {
            PreparedFileDiff::Single { blocks, .. }
            | PreparedFileDiff::SideBySide { blocks, .. } => blocks,
            PreparedFileDiff::Binary => &[],
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
                .text_size(px(DIFF_TEXT_SIZE))
                .line_height(px(DIFF_LINE_HEIGHT))
                .font_family(MONO_FONT_FAMILY)
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
                // gpui's `StyledText` takes its font size and line height from
                // the ambient `window.text_style()` (the cascade of `.text_size`
                // / `.line_height` from ancestor divs), NOT from the `TextStyle`
                // passed to `with_default_highlights` — that style only supplies
                // per-run font family, weight, and color. So the authoritative
                // sizing for the code text lives here on the parent div; changing
                // the fields in `diff_text_style()` alone has no visual effect.
                .text_size(px(DIFF_TEXT_SIZE))
                .line_height(px(DIFF_LINE_HEIGHT))
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

/// Row height for one diff line: tall enough to contain the 14px monospace
/// glyphs without clipping, with extra vertical padding around them, and the
/// shared box that vertically centers the gutter number against its code line.
pub(crate) const DIFF_LINE_HEIGHT: f32 = 22.;

/// Font size for diff gutter numbers and code lines. Kept in one place so the
/// gutter and the code text always render at the same scale.
pub(crate) const DIFF_TEXT_SIZE: f32 = 14.;

/// Base text style for diff code lines; syntax runs override color per token.
///
/// Note: gpui's `StyledText` reads `font_size` and `line_height` from the
/// ambient `window.text_style()`, not from this struct — only the per-run
/// font family, weight, and color carry through `with_default_highlights`.
/// The `font_size`/`line_height` here are kept in sync with `DIFF_TEXT_SIZE`
/// and `DIFF_LINE_HEIGHT` for documentation, but the values that actually
/// drive layout are set on the code-cell div in `render_file_diff_line`.
pub(crate) fn diff_text_style() -> TextStyle {
    TextStyle {
        color: Hsla::from(rgb(0xabb2bf)),
        font_family: MONO_FONT_FAMILY.into(),
        font_size: px(DIFF_TEXT_SIZE).into(),
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
    use gpui::{px, TestAppContext, VisualTestContext, WindowHandle};

    #[test]
    fn change_blocks_group_one_contiguous_run() {
        let rows = side_by_side_diff_rows("a_old\nb_old\n", "a_new\nb_new\n");
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 1);
    }

    #[test]
    fn change_blocks_split_runs_separated_by_a_large_gap() {
        // Four unchanged context rows separate the two changes: gap > max, so
        // the runs stay distinct blocks.
        let old_text = "a_old\nctx1\nctx2\nctx3\nctx4\nb_old\n";
        let new_text = "a_new\nctx1\nctx2\nctx3\nctx4\nb_new\n";
        let rows = side_by_side_diff_rows(old_text, new_text);
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 0);
        assert_eq!(blocks[1].start_row, 5);
        assert_eq!(blocks[1].end_row, 5);
    }

    #[test]
    fn change_blocks_merge_runs_within_the_gap() {
        // Exactly three unchanged context rows separate the changes: gap == max,
        // so the runs merge into one block whose endpoints are changed rows.
        let old_text = "a_old\nctx1\nctx2\nctx3\nb_old\n";
        let new_text = "a_new\nctx1\nctx2\nctx3\nb_new\n";
        let rows = side_by_side_diff_rows(old_text, new_text);
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 4);
    }

    #[test]
    fn change_blocks_single_side_is_one_block() {
        let rows = single_side_diff_rows(DiffSide::New, "first\nsecond\nthird\n");
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_row, 0);
        assert_eq!(blocks[0].end_row, 2);
    }

    #[test]
    fn change_blocks_are_empty_when_nothing_changed() {
        let rows = side_by_side_diff_rows("same\nlines\n", "same\nlines\n");
        let blocks = change_blocks(&rows, CHANGE_BLOCK_MAX_GAP);

        assert!(blocks.is_empty());
    }

    #[test]
    fn prepared_side_by_side_diff_exposes_change_blocks() {
        let content = repo::FileDiffContent::SideBySide {
            old_text: "a_old\nctx\nb_old\n".to_string(),
            new_text: "a_new\nctx\nb_new\n".to_string(),
        };
        let prepared = PreparedFileDiff::from_content(content, "");

        // One unchanged context row separates the changes (gap 1 <= max), so
        // they merge into a single block.
        assert_eq!(prepared.blocks().len(), 1);
        assert_eq!(prepared.blocks()[0].start_row, 0);
        assert_eq!(prepared.blocks()[0].end_row, 2);
    }

    #[test]
    fn prepared_binary_diff_has_no_change_blocks() {
        let prepared = PreparedFileDiff::from_content(repo::FileDiffContent::Binary, "");
        assert!(prepared.blocks().is_empty());
    }

    #[test]
    fn current_block_index_reports_the_first_block_at_or_below_the_top() {
        let blocks = [
            ChangeBlock {
                start_row: 4,
                end_row: 6,
            },
            ChangeBlock {
                start_row: 29,
                end_row: 31,
            },
        ];

        // Above the first block: it is the first one still in view.
        assert_eq!(current_block_index(&blocks, 0), 0);
        // Within the first block.
        assert_eq!(current_block_index(&blocks, 5), 0);
        // Below the first block but above the second: the second is next in view.
        assert_eq!(current_block_index(&blocks, 20), 1);
        // Within the second block — reported even if it cannot reach the top.
        assert_eq!(current_block_index(&blocks, 30), 1);
        // Scrolled past every block: clamp to the last.
        assert_eq!(current_block_index(&blocks, 50), 1);
    }

    #[test]
    fn next_block_index_steps_to_the_first_block_below_the_anchor() {
        let blocks = [
            ChangeBlock {
                start_row: 4,
                end_row: 4,
            },
            ChangeBlock {
                start_row: 29,
                end_row: 29,
            },
        ];

        // Anchor above the first block: the first block is the next stop, not
        // skipped (the visibility-aware fix).
        assert_eq!(next_block_index(&blocks, 0), 0);
        // Resting on the first block: advance to the second.
        assert_eq!(next_block_index(&blocks, 4), 1);
        // In the gap: the second block is the next stop.
        assert_eq!(next_block_index(&blocks, 10), 1);
        // Nothing begins below the anchor: wrap to the first block.
        assert_eq!(next_block_index(&blocks, 29), 0);
    }

    #[test]
    fn previous_block_index_steps_to_the_last_block_above_the_anchor() {
        let blocks = [
            ChangeBlock {
                start_row: 4,
                end_row: 4,
            },
            ChangeBlock {
                start_row: 29,
                end_row: 29,
            },
        ];

        // At or before the first block: wrap to the last.
        assert_eq!(previous_block_index(&blocks, 4), 1);
        assert_eq!(previous_block_index(&blocks, 0), 1);
        // In the gap: step back to the first block.
        assert_eq!(previous_block_index(&blocks, 10), 0);
        // At the last block: step back to the first.
        assert_eq!(previous_block_index(&blocks, 29), 0);
    }

    #[test]
    fn topmost_row_for_offset_floors_by_row_height() {
        // No scroll: the first row is at the top.
        assert_eq!(topmost_row_for_offset(px(0.), 10), 0);
        // Offset is negative as the content scrolls up; 2.5 rows down floors to 2.
        assert_eq!(topmost_row_for_offset(px(-DIFF_LINE_HEIGHT * 2.5), 10), 2);
        // Beyond the last row clamps to it.
        assert_eq!(topmost_row_for_offset(px(-DIFF_LINE_HEIGHT * 100.), 10), 9);
        // An empty diff has no rows to land on.
        assert_eq!(topmost_row_for_offset(px(-50.), 0), 0);
    }

    #[test]
    fn scroll_offset_for_block_top_leaves_context_above_the_block() {
        // A block starting at row 10 lands with three context rows above it.
        assert_eq!(
            scroll_offset_for_block_top(10, CHANGE_BLOCK_CONTEXT_ROWS),
            -px(7. * DIFF_LINE_HEIGHT)
        );
        // A block within the context margin of the top clamps to the top.
        assert_eq!(
            scroll_offset_for_block_top(2, CHANGE_BLOCK_CONTEXT_ROWS),
            px(0.)
        );
    }

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

    /// Open the three-block modified file in a single pane at a size that keeps
    /// the diff scrollable, returning the temp repo (kept alive by the caller),
    /// the window, and its visual context.
    fn open_multi_block_diff(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, WindowHandle<App>, VisualTestContext) {
        use gpui::{px, size};

        let (dir, oid_hex) = init_repo_with_multiple_change_blocks();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
                app.open_file_preview("blocks.txt".to_string(), cx);
            })
            .expect("open multi-block diff");

        cx.run_until_parked();

        let visual = VisualTestContext::from_window(*window, cx);
        visual.simulate_resize(size(px(700.), px(360.)));
        cx.run_until_parked();

        (dir, window, visual)
    }

    #[gpui::test]
    async fn change_block_footer_reports_the_current_position(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        visual
            .debug_bounds("change-block-footer")
            .expect("change-block footer");
        visual
            .debug_bounds("change-block-label")
            .expect("change-block label");

        let position = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position");
        assert_eq!(
            position,
            Some((0, 3)),
            "a fresh diff sits on the first of three blocks"
        );
    }

    #[gpui::test]
    async fn clicking_next_change_block_advances_and_scrolls(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        let before = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read offset before");

        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let position = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position");
        assert_eq!(position, Some((1, 3)), "next advances to the second block");

        let after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read offset after");
        assert!(
            after.y < before.y,
            "advancing to the next block scrolls the diff down"
        );
    }

    #[gpui::test]
    async fn clicking_previous_change_block_wraps_to_the_last(cx: &mut TestAppContext) {
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        let prev = visual
            .debug_bounds("change-block-prev")
            .expect("previous button");
        visual.simulate_click(prev.center(), Modifiers::none());
        cx.run_until_parked();

        let position = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position");
        assert_eq!(
            position,
            Some((2, 3)),
            "previous from the first block wraps to the last"
        );
    }

    #[gpui::test]
    async fn change_block_keybindings_navigate_the_active_diff(cx: &mut TestAppContext) {
        cx.update(crate::app::bind_app_keys);
        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        visual.simulate_keystrokes("cmd-down");
        cx.run_until_parked();
        let after_next = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position after cmd-down");
        assert_eq!(after_next, Some((1, 3)), "cmd-down advances a block");

        visual.simulate_keystrokes("cmd-up");
        cx.run_until_parked();
        let after_prev = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position after cmd-up");
        assert_eq!(after_prev, Some((0, 3)), "cmd-up steps back a block");
    }

    #[gpui::test]
    async fn opening_a_diff_scrolls_to_its_first_change_block(cx: &mut TestAppContext) {
        use gpui::px;

        let (_dir, window, _visual) = open_multi_block_diff(cx);

        let offset = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read offset");
        assert!(
            offset.y < px(0.),
            "a freshly opened diff auto-scrolls down to its first change block"
        );
    }

    #[gpui::test]
    async fn stepping_next_from_the_top_lands_on_the_first_block_without_skipping(
        cx: &mut TestAppContext,
    ) {
        use gpui::{point, px, ScrollDelta, ScrollWheelEvent};

        let (_dir, window, mut visual) = open_multi_block_diff(cx);

        // Scroll back above the first change, to the very top of the file.
        let side = visual
            .debug_bounds("file-diff-side-new")
            .expect("new file diff side");
        visual.simulate_event(ScrollWheelEvent {
            position: side.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(480.))),
            ..Default::default()
        });
        cx.run_until_parked();

        let at_top = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read offset at top");
        assert_eq!(at_top.y, px(0.), "scrolled to the very top of the file");
        let at_top_position = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position at top");
        assert_eq!(
            at_top_position,
            Some((0, 3)),
            "the counter still points at the first block while above it"
        );

        // Next must bring the first block into view, not skip to the second.
        let next = visual
            .debug_bounds("change-block-next")
            .expect("next button");
        visual.simulate_click(next.center(), Modifiers::none());
        cx.run_until_parked();

        let after = window
            .read_with(cx, |app, _cx| app.file_diff_new_scroll_offset())
            .expect("read offset after next");
        assert!(
            after.y < px(0.),
            "next from the top scrolls down into the first block"
        );
        let after_position = window
            .read_with(cx, |app, _cx| app.active_diff_block_position())
            .expect("read block position after next");
        assert_eq!(
            after_position,
            Some((0, 3)),
            "next stepped onto the first block rather than skipping it"
        );
    }

    #[gpui::test]
    async fn binary_diff_hides_the_change_block_footer(cx: &mut TestAppContext) {
        let (dir, oid_hex) = init_repo_with_binary_file();
        let path = dir.path().to_path_buf();
        let window = add_app_window(cx);

        window
            .update(cx, |app, window, cx| {
                app.open_repository_at(path, window, cx);
                app.select_single_commit(oid_hex, cx);
                app.open_changeset(window, cx);
            })
            .expect("open binary changeset");

        cx.run_until_parked();

        let mut visual = VisualTestContext::from_window(*window, cx);
        let row_bounds = visual
            .debug_bounds("changed-file-row-0")
            .expect("changed file row debug bounds");
        visual.simulate_click(row_bounds.center(), Modifiers::none());
        cx.run_until_parked();

        visual
            .debug_bounds("file-diff-binary")
            .expect("binary diff placeholder");
        assert!(
            visual.debug_bounds("change-block-footer").is_none(),
            "a binary diff has no change blocks and shows no footer"
        );
    }
}
