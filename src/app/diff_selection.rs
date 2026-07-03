//! Selection model for the diff view: a caret or text selection on one side
//! of one file's diff, plus the pure motion/extraction logic that operates
//! on it. See docs/superpowers/specs/2026-07-02-diff-selection-design.md.

use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use gpui::{HighlightStyle, Hsla, Pixels};

use crate::repo;

use super::diff_view::{DiffLineCell, DiffLineStatus, PreparedFileDiff};

pub(crate) fn move_left(content: &DiffSideContent, point: DiffPoint) -> DiffPoint {
    let text = &content.cell(point.row).text;
    if point.column > 0 {
        let mut column = point.column - 1;
        while column > 0 && !text.is_char_boundary(column) {
            column -= 1;
        }
        return DiffPoint {
            row: point.row,
            column,
        };
    }
    match content.prev_selectable(point.row) {
        Some(row) => DiffPoint {
            row,
            column: content.cell(row).text.len(),
        },
        None => point,
    }
}

pub(crate) fn move_right(content: &DiffSideContent, point: DiffPoint) -> DiffPoint {
    let text = &content.cell(point.row).text;
    if point.column < text.len() {
        let mut column = point.column + 1;
        while column < text.len() && !text.is_char_boundary(column) {
            column += 1;
        }
        return DiffPoint {
            row: point.row,
            column,
        };
    }
    match content.next_selectable(point.row) {
        Some(row) => DiffPoint { row, column: 0 },
        None => point,
    }
}

pub(crate) fn line_start(point: DiffPoint) -> DiffPoint {
    DiffPoint {
        row: point.row,
        column: 0,
    }
}

pub(crate) fn line_end(content: &DiffSideContent, point: DiffPoint) -> DiffPoint {
    DiffPoint {
        row: point.row,
        column: content.cell(point.row).text.len(),
    }
}

/// The first selectable position, or None when the content has no
/// selectable rows (empty or binary content, or gaps only).
pub(crate) fn document_start(content: &DiffSideContent) -> Option<DiffPoint> {
    let row = if content.is_selectable(0) {
        Some(0)
    } else {
        content.next_selectable(0)
    }?;
    Some(DiffPoint { row, column: 0 })
}

/// The last selectable position, or None when nothing is selectable.
pub(crate) fn document_end(content: &DiffSideContent) -> Option<DiffPoint> {
    let last = content.len().checked_sub(1)?;
    let row = if content.is_selectable(last) {
        Some(last)
    } else {
        content.prev_selectable(last)
    }?;
    Some(DiffPoint {
        row,
        column: content.cell(row).text.len(),
    })
}

/// The row a vertical step lands on, skipping alignment gaps. None at the
/// document edge — the caller keeps the caret where it is.
pub(crate) fn vertical_target_row(
    content: &DiffSideContent,
    row: usize,
    forward: bool,
) -> Option<usize> {
    if forward {
        content.next_selectable(row)
    } else {
        content.prev_selectable(row)
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

fn char_class(character: char) -> CharClass {
    if character.is_whitespace() {
        CharClass::Whitespace
    } else if character.is_alphanumeric() || character == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

/// The run of same-class characters containing `column`, for double-click
/// selection. A column at the line end takes the run before it.
pub(crate) fn word_range_at(text: &str, column: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let anchor = if column >= text.len() {
        text.char_indices()
            .last()
            .map(|(index, _)| index)
            .unwrap_or(0)
    } else {
        column
    };
    let class = char_class(text[anchor..].chars().next().unwrap_or(' '));
    let start = text[..anchor]
        .char_indices()
        .rev()
        .take_while(|(_, character)| char_class(*character) == class)
        .last()
        .map(|(index, _)| index)
        .unwrap_or(anchor);
    let end = text[anchor..]
        .char_indices()
        .take_while(|(_, character)| char_class(*character) == class)
        .last()
        .map(|(index, character)| anchor + index + character.len_utf8())
        .unwrap_or(anchor);
    start..end
}

pub(crate) fn move_word_right(content: &DiffSideContent, point: DiffPoint) -> DiffPoint {
    let text = &content.cell(point.row).text;
    if point.column >= text.len() {
        // Cross to the next line and consume its first word.
        return match content.next_selectable(point.row) {
            Some(row) => move_word_right(content, DiffPoint { row, column: 0 }),
            None => point,
        };
    }
    let rest = &text[point.column..];
    let mut chars = rest.char_indices().peekable();
    // Skip leading whitespace, then consume one run of a single class.
    let mut offset = 0;
    while let Some((index, character)) = chars.peek().copied() {
        if char_class(character) == CharClass::Whitespace {
            offset = index + character.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    let class = match rest[offset..].chars().next() {
        Some(character) => char_class(character),
        None => {
            return move_word_right(
                content,
                DiffPoint {
                    row: point.row,
                    column: text.len(),
                },
            )
        }
    };
    let mut end = offset;
    for (index, character) in rest[offset..].char_indices() {
        if char_class(character) == class {
            end = offset + index + character.len_utf8();
        } else {
            break;
        }
    }
    DiffPoint {
        row: point.row,
        column: point.column + end,
    }
}

pub(crate) fn move_word_left(content: &DiffSideContent, point: DiffPoint) -> DiffPoint {
    let text = &content.cell(point.row).text;
    if point.column == 0 {
        return match content.prev_selectable(point.row) {
            Some(row) => {
                let end = content.cell(row).text.len();
                move_word_left(content, DiffPoint { row, column: end })
            }
            None => point,
        };
    }
    let before = &text[..point.column];
    let mut column = point.column;
    // Skip trailing whitespace, then one run of a single class.
    let mut chars = before.char_indices().rev().peekable();
    while let Some((index, character)) = chars.peek().copied() {
        if char_class(character) == CharClass::Whitespace {
            column = index;
            chars.next();
        } else {
            break;
        }
    }
    if column == 0 {
        return move_word_left(
            content,
            DiffPoint {
                row: point.row,
                column: 0,
            },
        );
    }
    let class = char_class(text[..column].chars().last().unwrap());
    for (index, character) in text[..column].char_indices().rev() {
        if char_class(character) == class {
            column = index;
        } else {
            break;
        }
    }
    DiffPoint {
        row: point.row,
        column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::diff_view::read_only_file_cells;

    fn cells(lines: &[&str]) -> DiffSideContent {
        DiffSideContent::ReadOnly {
            cells: Rc::new(read_only_file_cells(&lines.join("\n"))),
        }
    }

    /// Content with a hatched gap: rows 0 and 2 are real, row 1 is Empty.
    fn cells_with_gap() -> DiffSideContent {
        let mut list = read_only_file_cells("alpha\nbeta");
        list.insert(1, super::super::diff_view::empty_diff_cell());
        DiffSideContent::ReadOnly {
            cells: Rc::new(list),
        }
    }

    #[test]
    fn points_order_by_row_then_column() {
        let early = DiffPoint { row: 1, column: 9 };
        let late = DiffPoint { row: 2, column: 0 };
        assert!(early < late);
        assert!(DiffPoint { row: 1, column: 0 } < early);
    }

    #[test]
    fn selection_range_orders_endpoints_and_spans_lines() {
        let selection = DiffSelection {
            side: repo::DiffSide::New,
            anchor: DiffPoint { row: 3, column: 2 },
            head: DiffPoint { row: 1, column: 4 },
            goal_x: None,
        };
        let (start, end) = selection.range();
        assert_eq!(start, DiffPoint { row: 1, column: 4 });
        assert_eq!(end, DiffPoint { row: 3, column: 2 });
        assert_eq!(selection.line_range(), 1..=3);
        assert!(!selection.is_caret());
        assert_eq!(selection.caret(), selection.head);
    }

    #[test]
    fn gap_rows_are_not_selectable_and_navigation_skips_them() {
        let content = cells_with_gap();
        assert!(content.is_selectable(0));
        assert!(!content.is_selectable(1));
        assert!(content.is_selectable(2));
        assert_eq!(content.next_selectable(0), Some(2));
        assert_eq!(content.prev_selectable(2), Some(0));
        assert_eq!(content.prev_selectable(0), None);
        assert_eq!(content.next_selectable(2), None);
    }

    #[test]
    fn clamp_snaps_to_char_boundary_and_line_end() {
        // "héllo" — 'é' is 2 bytes (offsets 1..3).
        let content = cells(&["héllo"]);
        let inside_e = content.clamp(DiffPoint { row: 0, column: 2 });
        assert_eq!(inside_e.column, 1, "mid-char snaps back to boundary");
        let past_end = content.clamp(DiffPoint { row: 0, column: 99 });
        assert_eq!(past_end.column, 6, "clamps to byte length of line");
        let past_last_row = content.clamp(DiffPoint { row: 9, column: 0 });
        assert_eq!(past_last_row.row, 0, "clamps to last selectable row");
    }

    #[test]
    fn selection_text_slices_single_line() {
        let content = cells(&["hello world"]);
        let selection = DiffSelection {
            side: repo::DiffSide::New,
            anchor: DiffPoint { row: 0, column: 6 },
            head: DiffPoint { row: 0, column: 11 },
            goal_x: None,
        };
        assert_eq!(selection_text(&content, &selection), "world");
    }

    #[test]
    fn selection_text_joins_lines_and_skips_gaps() {
        let content = cells_with_gap(); // rows: "alpha", <gap>, "beta"
        let selection = DiffSelection {
            side: repo::DiffSide::New,
            anchor: DiffPoint { row: 0, column: 2 },
            head: DiffPoint { row: 2, column: 2 },
            goal_x: None,
        };
        assert_eq!(selection_text(&content, &selection), "pha\nbe");
    }

    #[test]
    fn selection_text_for_bare_caret_is_empty() {
        let content = cells(&["hello"]);
        let selection =
            DiffSelection::caret_at(DiffPoint { row: 0, column: 3 }, repo::DiffSide::New);
        assert_eq!(selection_text(&content, &selection), "");
    }

    #[test]
    fn char_motion_crosses_line_boundaries_and_gaps() {
        let content = cells_with_gap(); // "alpha", <gap>, "beta"
                                        // Right from end of "alpha" lands at start of "beta", skipping the gap.
        let end_alpha = DiffPoint { row: 0, column: 5 };
        assert_eq!(
            move_right(&content, end_alpha),
            DiffPoint { row: 2, column: 0 }
        );
        // Left from start of "beta" lands at end of "alpha".
        let start_beta = DiffPoint { row: 2, column: 0 };
        assert_eq!(move_left(&content, start_beta), end_alpha);
        // At document edges, motion stays put.
        assert_eq!(
            move_left(&content, DiffPoint { row: 0, column: 0 }),
            DiffPoint { row: 0, column: 0 }
        );
        assert_eq!(
            move_right(&content, DiffPoint { row: 2, column: 4 }),
            DiffPoint { row: 2, column: 4 }
        );
    }

    #[test]
    fn char_motion_steps_whole_utf8_chars() {
        let content = cells(&["héllo"]);
        assert_eq!(
            move_right(&content, DiffPoint { row: 0, column: 1 }).column,
            3
        );
        assert_eq!(
            move_left(&content, DiffPoint { row: 0, column: 3 }).column,
            1
        );
    }

    #[test]
    fn line_and_document_ends() {
        let content = cells_with_gap();
        assert_eq!(
            line_start(DiffPoint { row: 2, column: 3 }),
            DiffPoint { row: 2, column: 0 }
        );
        assert_eq!(
            line_end(&content, DiffPoint { row: 0, column: 1 }).column,
            5
        );
        assert_eq!(
            document_start(&content).unwrap(),
            DiffPoint { row: 0, column: 0 }
        );
        assert_eq!(
            document_end(&content).unwrap(),
            DiffPoint { row: 2, column: 4 }
        );
    }

    #[test]
    fn document_ends_are_none_without_selectable_rows() {
        let empty = DiffSideContent::ReadOnly {
            cells: Rc::new(Vec::new()),
        };
        assert_eq!(document_start(&empty), None);
        assert_eq!(document_end(&empty), None);
        let gaps_only = DiffSideContent::ReadOnly {
            cells: Rc::new(vec![
                super::super::diff_view::empty_diff_cell(),
                super::super::diff_view::empty_diff_cell(),
            ]),
        };
        assert_eq!(document_start(&gaps_only), None);
        assert_eq!(document_end(&gaps_only), None);
    }

    #[test]
    fn vertical_target_skips_gaps_and_stops_at_edges() {
        let content = cells_with_gap();
        assert_eq!(vertical_target_row(&content, 0, true), Some(2));
        assert_eq!(vertical_target_row(&content, 2, false), Some(0));
        assert_eq!(vertical_target_row(&content, 0, false), None);
        assert_eq!(vertical_target_row(&content, 2, true), None);
    }

    #[test]
    fn word_range_at_picks_the_surrounding_run() {
        assert_eq!(word_range_at("foo_bar(baz)", 2), 0..7); // inside foo_bar
        assert_eq!(word_range_at("foo_bar(baz)", 7), 7..8); // on the paren
        assert_eq!(word_range_at("foo bar", 3), 3..4); // on the space
        assert_eq!(word_range_at("foo", 3), 0..3); // at end of line
        assert_eq!(word_range_at("", 0), 0..0);
    }

    #[test]
    fn word_motion_stops_at_class_transitions() {
        let content = cells(&["let x = a.b;"]);
        // From start: right lands after "let", then after "x", then after "=".
        let mut point = DiffPoint { row: 0, column: 0 };
        point = move_word_right(&content, point);
        assert_eq!(point.column, 3);
        point = move_word_right(&content, point);
        assert_eq!(point.column, 5);
        // Left from column 5 returns to the start of "x".
        assert_eq!(move_word_left(&content, point).column, 4);
    }

    #[test]
    fn word_motion_crosses_lines() {
        let content = cells_with_gap(); // "alpha", <gap>, "beta"
        let end_alpha = DiffPoint { row: 0, column: 5 };
        assert_eq!(
            move_word_right(&content, end_alpha),
            DiffPoint { row: 2, column: 4 }
        );
        let start_beta = DiffPoint { row: 2, column: 0 };
        assert_eq!(
            move_word_left(&content, start_beta),
            DiffPoint { row: 0, column: 0 }
        );
    }

    fn colored(range: Range<usize>, color: Hsla) -> (Range<usize>, HighlightStyle) {
        (
            range,
            HighlightStyle {
                color: Some(color),
                ..Default::default()
            },
        )
    }

    #[test]
    fn overlay_fills_unstyled_span() {
        let bg = gpui::hsla(0., 1., 0.5, 1.);
        let result = overlay_background(&[], 2..5, bg);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 2..5);
        assert_eq!(result[0].1.background_color, Some(bg));
        assert_eq!(result[0].1.color, None);
    }

    #[test]
    fn overlay_splits_a_straddling_syntax_run() {
        let fg = gpui::hsla(240., 1., 0.5, 1.);
        let bg = gpui::hsla(0., 1., 0.5, 1.);
        // Syntax run 0..6, selection 3..9.
        let result = overlay_background(&[colored(0..6, fg)], 3..9, bg);
        // Expect: 0..3 fg only; 3..6 fg+bg; 6..9 bg only — sorted, disjoint.
        assert_eq!(result[0].0, 0..3);
        assert_eq!(result[0].1.background_color, None);
        assert_eq!(result[1].0, 3..6);
        assert_eq!(result[1].1.color, Some(fg));
        assert_eq!(result[1].1.background_color, Some(bg));
        assert_eq!(result[2].0, 6..9);
        assert_eq!(result[2].1.background_color, Some(bg));
    }

    #[test]
    fn overlay_with_empty_span_changes_nothing() {
        let fg = gpui::hsla(240., 1., 0.5, 1.);
        let runs = vec![colored(0..4, fg)];
        assert_eq!(
            overlay_background(&runs, 2..2, gpui::hsla(0., 1., 0.5, 1.)),
            runs
        );
    }

    #[test]
    fn overlay_tolerates_unsorted_input_runs() {
        let fg = gpui::hsla(240., 1., 0.5, 1.);
        let bg = gpui::hsla(0., 1., 0.5, 1.);
        let result = overlay_background(&[colored(6..8, fg), colored(2..4, fg)], 1..10, bg);
        // Output must be sorted and non-overlapping regardless of input order.
        for pair in result.windows(2) {
            assert!(
                pair[0].0.end <= pair[1].0.start,
                "overlapping runs: {:?}",
                (&pair[0].0, &pair[1].0)
            );
        }
        // The whole span carries the background.
        assert_eq!(
            result
                .iter()
                .filter(|(_, style)| style.background_color == Some(bg))
                .map(|(range, _)| range.len())
                .sum::<usize>(),
            9
        );
    }
}

/// The selected characters as plain text: line slices in row order joined
/// with `\n`, alignment gaps skipped. No markers, no line numbers.
pub(crate) fn selection_text(content: &DiffSideContent, selection: &DiffSelection) -> String {
    let (start, end) = selection.range();
    let mut parts: Vec<&str> = Vec::new();
    for row in start.row..=end.row {
        if !content.is_selectable(row) {
            continue;
        }
        let text = &content.cell(row).text;
        let from = if row == start.row {
            start.column.min(text.len())
        } else {
            0
        };
        let to = if row == end.row {
            end.column.min(text.len())
        } else {
            text.len()
        };
        parts.push(&text[from..to]);
    }
    parts.join("\n")
}

/// Merge a selection span into a line's highlight runs: existing runs are
/// split at the span's edges, runs inside the span gain `background_color`,
/// and uncovered stretches inside the span get background-only runs. The
/// result is sorted and non-overlapping, as `StyledText` requires. Accepts
/// runs in any order; output is always sorted.
pub(crate) fn overlay_background(
    highlights: &[(Range<usize>, HighlightStyle)],
    span: Range<usize>,
    background: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if span.is_empty() {
        return highlights.to_vec();
    }
    let mut result: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    // Cursor over the span tracking which parts still need a bg-only run.
    let mut uncovered = span.start;

    let mut highlights = highlights.to_vec();
    highlights.sort_by_key(|(range, _)| range.start);

    for (range, style) in &highlights {
        // Part of the run before the span: unchanged.
        if range.start < span.start {
            let end = range.end.min(span.start);
            result.push((range.start..end, *style));
        }
        // Part inside the span: gains the background.
        let inside_start = range.start.max(span.start);
        let inside_end = range.end.min(span.end);
        if inside_start < inside_end {
            if uncovered < inside_start {
                result.push((
                    uncovered..inside_start,
                    HighlightStyle {
                        background_color: Some(background),
                        ..Default::default()
                    },
                ));
            }
            let mut style = *style;
            style.background_color = Some(background);
            result.push((inside_start..inside_end, style));
            uncovered = inside_end;
        }
        // Part after the span: unchanged.
        if range.end > span.end {
            let start = range.start.max(span.end);
            result.push((start..range.end, *style));
        }
    }
    if uncovered < span.end {
        result.push((
            uncovered..span.end,
            HighlightStyle {
                background_color: Some(background),
                ..Default::default()
            },
        ));
    }
    result.sort_by_key(|(range, _)| range.start);
    result
}

/// A caret position: a row index into the flat diff rows plus a UTF-8 byte
/// column within that row's line text, always on a `char` boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DiffPoint {
    pub(crate) row: usize,
    pub(crate) column: usize,
}

/// A selection on one side of one file's diff. The caret is `head`;
/// `anchor == head` is a bare caret. `goal_x` is the remembered horizontal
/// position for vertical motion (pixels within the code content, pan
/// included), set by vertical motion and cleared by anything horizontal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DiffSelection {
    pub(crate) side: repo::DiffSide,
    pub(crate) anchor: DiffPoint,
    pub(crate) head: DiffPoint,
    pub(crate) goal_x: Option<Pixels>,
}

impl DiffSelection {
    pub(crate) fn caret_at(point: DiffPoint, side: repo::DiffSide) -> Self {
        Self {
            side,
            anchor: point,
            head: point,
            goal_x: None,
        }
    }

    pub(crate) fn caret(&self) -> DiffPoint {
        self.head
    }

    pub(crate) fn is_caret(&self) -> bool {
        self.anchor == self.head
    }

    /// Endpoints in document order.
    pub(crate) fn range(&self) -> (DiffPoint, DiffPoint) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub(crate) fn line_range(&self) -> RangeInclusive<usize> {
        let (start, end) = self.range();
        start.row..=end.row
    }
}

/// One side's line cells, whichever way the view holds them: a prepared
/// diff (changed files) or a bare cell list (read-only files). Lets every
/// pure selection function work on both without cloning lines.
#[derive(Clone)]
pub(crate) enum DiffSideContent {
    Prepared {
        diff: Rc<PreparedFileDiff>,
        side: repo::DiffSide,
    },
    ReadOnly {
        cells: Rc<Vec<DiffLineCell>>,
    },
}

impl DiffSideContent {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Prepared { diff, .. } => match diff.as_ref() {
                PreparedFileDiff::Single { rows, .. }
                | PreparedFileDiff::SideBySide { rows, .. } => rows.len(),
                PreparedFileDiff::Binary => 0,
            },
            Self::ReadOnly { cells } => cells.len(),
        }
    }

    pub(crate) fn cell(&self, row: usize) -> &DiffLineCell {
        match self {
            Self::Prepared { diff, side } => {
                let rows = match diff.as_ref() {
                    PreparedFileDiff::Single { rows, .. }
                    | PreparedFileDiff::SideBySide { rows, .. } => rows,
                    PreparedFileDiff::Binary => unreachable!("binary diffs have no rows"),
                };
                match side {
                    repo::DiffSide::Old => &rows[row].old,
                    repo::DiffSide::New => &rows[row].new,
                }
            }
            Self::ReadOnly { cells } => &cells[row],
        }
    }

    /// Alignment gaps cannot host the caret.
    pub(crate) fn is_selectable(&self, row: usize) -> bool {
        row < self.len() && self.cell(row).status != DiffLineStatus::Empty
    }

    pub(crate) fn next_selectable(&self, row: usize) -> Option<usize> {
        ((row + 1)..self.len()).find(|&candidate| self.is_selectable(candidate))
    }

    pub(crate) fn prev_selectable(&self, row: usize) -> Option<usize> {
        (0..row)
            .rev()
            .find(|&candidate| self.is_selectable(candidate))
    }

    /// Snap a point onto real content: a selectable row, a column no larger
    /// than the line, and a `char` boundary.
    pub(crate) fn clamp(&self, point: DiffPoint) -> DiffPoint {
        let mut row = point.row.min(self.len().saturating_sub(1));
        if !self.is_selectable(row) {
            row = self
                .prev_selectable(row)
                .or_else(|| self.next_selectable(row))
                .unwrap_or(0);
        }
        let text = &self.cell(row).text;
        let mut column = point.column.min(text.len());
        while column > 0 && !text.is_char_boundary(column) {
            column -= 1;
        }
        DiffPoint { row, column }
    }
}
