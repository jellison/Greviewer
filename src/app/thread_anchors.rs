//! Converting between diff selections and persisted thread anchors.
//!
//! A persisted anchor names 1-based file line numbers on one diff side plus
//! the quoted text; at runtime we resolve it back onto flat diff-row indices
//! (`DiffPoint`s) against the currently prepared diff, falling back to a
//! quoted-text search when the line numbers no longer match.

use crate::app::diff_selection::{self, DiffPoint, DiffSelection, DiffSideContent};
use crate::repo;
use crate::reviews::{ThreadAnchor, ThreadSide};

pub(crate) fn thread_side(side: repo::DiffSide) -> ThreadSide {
    match side {
        repo::DiffSide::Old => ThreadSide::Old,
        repo::DiffSide::New => ThreadSide::New,
    }
}

pub(crate) fn diff_side(side: ThreadSide) -> repo::DiffSide {
    match side {
        ThreadSide::Old => repo::DiffSide::Old,
        ThreadSide::New => repo::DiffSide::New,
    }
}

/// An anchor resolved onto the current diff: ordered flat-row points on
/// `side`, in the same row space as `DiffSelection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedAnchor {
    pub side: repo::DiffSide,
    pub start: DiffPoint,
    pub end: DiffPoint,
}

/// Builds the persisted anchor for a (non-caret) selection. `None` when the
/// selection is a bare caret or its endpoints sit on rows with no line
/// number on `side` (alignment gaps).
pub(crate) fn anchor_from_selection(
    content: &DiffSideContent,
    side: repo::DiffSide,
    selection: &DiffSelection,
) -> Option<ThreadAnchor> {
    if selection.is_caret() {
        return None;
    }
    let (start, end) = selection.range();
    let start_line = content.cell(start.row).line_number?;
    let end_line = content.cell(end.row).line_number?;
    Some(ThreadAnchor {
        side: thread_side(side),
        start_line,
        start_col: start.column,
        end_line,
        end_col: end.column,
        quoted_text: diff_selection::selection_text(content, selection),
    })
}

/// Resolves a persisted anchor onto the current diff rows. Line-number match
/// first; when the numbers no longer name rows containing the quoted text,
/// falls back to searching for the quoted text's first line.
pub(crate) fn resolve_anchor(
    content: &DiffSideContent,
    anchor: &ThreadAnchor,
) -> Option<ResolvedAnchor> {
    let side = diff_side(anchor.side);
    if let Some(resolved) = resolve_by_line_numbers(content, anchor, side) {
        return Some(resolved);
    }
    resolve_by_quoted_text(content, anchor, side)
}

fn row_for_line(content: &DiffSideContent, line: usize) -> Option<usize> {
    (0..content.len()).find(|&row| content.cell(row).line_number == Some(line))
}

fn clamped_point(content: &DiffSideContent, row: usize, column: usize) -> DiffPoint {
    content.clamp(DiffPoint { row, column })
}

fn resolve_by_line_numbers(
    content: &DiffSideContent,
    anchor: &ThreadAnchor,
    side: repo::DiffSide,
) -> Option<ResolvedAnchor> {
    let start_row = row_for_line(content, anchor.start_line)?;
    let end_row = row_for_line(content, anchor.end_line)?;
    let start = clamped_point(content, start_row, anchor.start_col);
    let end = clamped_point(content, end_row, anchor.end_col);
    // Verify the quoted text still lives there; otherwise let the fallback run.
    let selection = DiffSelection {
        side,
        anchor: start,
        head: end,
        goal_x: None,
    };
    (diff_selection::selection_text(content, &selection) == anchor.quoted_text
        || anchor.quoted_text.is_empty())
    .then_some(ResolvedAnchor { side, start, end })
}

fn resolve_by_quoted_text(
    content: &DiffSideContent,
    anchor: &ThreadAnchor,
    side: repo::DiffSide,
) -> Option<ResolvedAnchor> {
    let first_line = anchor.quoted_text.lines().next()?;
    if first_line.is_empty() {
        return None;
    }
    let line_span = anchor.end_line.saturating_sub(anchor.start_line);
    let start_row = (0..content.len())
        .find(|&row| content.is_selectable(row) && content.cell(row).text.contains(first_line))?;
    let start_col = content.cell(start_row).text.find(first_line)?;
    let end_row = (start_row + line_span).min(content.len().saturating_sub(1));
    let end_col = if line_span == 0 {
        start_col + first_line.len()
    } else {
        anchor.end_col
    };
    Some(ResolvedAnchor {
        side,
        start: clamped_point(content, start_row, start_col),
        end: clamped_point(content, end_row, end_col),
    })
}

/// "name.rs:12" for single-line anchors, "name.rs:12–14" (en dash) for
/// ranges: the file's basename joined to the anchor's persisted line
/// reference, as shown in a thread row's meta line.
pub(crate) fn thread_location_label(path: &str, anchor: &ThreadAnchor) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    if anchor.start_line == anchor.end_line {
        format!("{name}:{}", anchor.start_line)
    } else {
        format!("{name}:{}\u{2013}{}", anchor.start_line, anchor.end_line)
    }
}

#[cfg(test)]
mod tests {
    use crate::app::diff_selection::{DiffPoint, DiffSelection, DiffSideContent};
    use crate::app::diff_view;
    use crate::repo;
    use crate::reviews::{ThreadAnchor, ThreadSide};
    use std::rc::Rc;

    use super::*;

    fn content(text: &str) -> DiffSideContent {
        DiffSideContent::ReadOnly {
            cells: Rc::new(diff_view::read_only_file_cells(text)),
        }
    }

    #[test]
    fn selection_becomes_an_anchor_with_line_numbers_and_quoted_text() {
        let content = content("alpha\nbravo\ncharlie\n");
        let selection = DiffSelection {
            side: repo::DiffSide::New,
            anchor: DiffPoint { row: 1, column: 2 },
            head: DiffPoint { row: 2, column: 4 },
            goal_x: None,
        };
        let anchor = anchor_from_selection(&content, repo::DiffSide::New, &selection)
            .expect("range selection anchors");
        assert_eq!(anchor.side, ThreadSide::New);
        assert_eq!((anchor.start_line, anchor.start_col), (2, 2));
        assert_eq!((anchor.end_line, anchor.end_col), (3, 4));
        assert_eq!(anchor.quoted_text, "avo\nchar");
    }

    #[test]
    fn caret_selection_does_not_anchor() {
        let content = content("alpha\n");
        let selection = DiffSelection {
            side: repo::DiffSide::New,
            anchor: DiffPoint { row: 0, column: 1 },
            head: DiffPoint { row: 0, column: 1 },
            goal_x: None,
        };
        assert!(anchor_from_selection(&content, repo::DiffSide::New, &selection).is_none());
    }

    #[test]
    fn anchor_resolves_by_line_number() {
        let content = content("alpha\nbravo\ncharlie\n");
        let anchor = ThreadAnchor {
            side: ThreadSide::New,
            start_line: 2,
            start_col: 1,
            end_line: 2,
            end_col: 4,
            quoted_text: "rav".into(),
        };
        let resolved = resolve_anchor(&content, &anchor).expect("resolves");
        assert_eq!(resolved.start, DiffPoint { row: 1, column: 1 });
        assert_eq!(resolved.end, DiffPoint { row: 1, column: 4 });
    }

    #[test]
    fn anchor_falls_back_to_quoted_text_when_line_numbers_miss() {
        let content = content("alpha\nbravo\ncharlie\n");
        let anchor = ThreadAnchor {
            side: ThreadSide::New,
            start_line: 900, // drifted
            start_col: 1,
            end_line: 900,
            end_col: 4,
            quoted_text: "rav".into(),
        };
        let resolved = resolve_anchor(&content, &anchor).expect("quoted text rescues it");
        assert_eq!(resolved.start, DiffPoint { row: 1, column: 1 });
        assert_eq!(resolved.end, DiffPoint { row: 1, column: 4 });
    }

    #[test]
    fn unresolvable_anchor_returns_none() {
        let content = content("alpha\n");
        let anchor = ThreadAnchor {
            side: ThreadSide::New,
            start_line: 900,
            start_col: 0,
            end_line: 900,
            end_col: 3,
            quoted_text: "zzz".into(),
        };
        assert!(resolve_anchor(&content, &anchor).is_none());
    }

    #[test]
    fn thread_location_labels() {
        let single = ThreadAnchor {
            start_line: 60,
            end_line: 60,
            ..Default::default()
        };
        let multi = ThreadAnchor {
            start_line: 62,
            end_line: 63,
            ..Default::default()
        };
        assert_eq!(
            thread_location_label("src/deep/name.rs", &single),
            "name.rs:60"
        );
        assert_eq!(
            thread_location_label("name.rs", &multi),
            "name.rs:62\u{2013}63"
        );
    }
}
