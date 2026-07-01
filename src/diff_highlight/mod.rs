//! Syntax highlighting and word-level diff emphasis for the file-diff view.
//!
//! Pure logic only: no gpui rendering. The Material palette is authored
//! from the user's OpenCode Material Dark Zed theme; per ADR-0001 nothing
//! here is copied from Zed's GPL repository.

use std::ops::Range;
use std::sync::OnceLock;

use gpui::HighlightStyle;
use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
use ropey::Rope;
use similar::{DiffTag, TextDiff};

static MATERIAL: OnceLock<HighlightTheme> = OnceLock::new();

/// The OpenCode Material Dark highlight theme used by the diff view.
pub fn material_theme() -> &'static HighlightTheme {
    MATERIAL.get_or_init(|| {
        serde_json::from_str(include_str!("material.json"))
            .expect("embedded Material theme JSON is valid")
    })
}

/// Computes syntax-highlight runs for every line of `text`.
///
/// The whole text is parsed once so multi-line constructs (docstrings,
/// block comments) highlight correctly. Each inner vec holds runs with
/// byte ranges relative to that line, matching the trimmed line content
/// produced by `content_lines` in app.rs. Unknown languages fall back to
/// the plain-text grammar, which emits a single default-style run per line
/// (no colors), so callers need no special plain-text path.
pub fn line_highlight_runs(text: &str, language: &str) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let theme = material_theme();
    let rope = Rope::from_str(text);
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &rope);

    line_byte_ranges(text)
        .into_iter()
        .map(|line_range| {
            let line_start = line_range.start;
            highlighter
                .styles(&line_range, theme)
                .into_iter()
                .filter_map(|(range, style)| {
                    let clamped = range.start.max(line_range.start)..range.end.min(line_range.end);
                    (clamped.start < clamped.end)
                        .then(|| (clamped.start - line_start..clamped.end - line_start, style))
                })
                .collect()
        })
        .collect()
}

/// Byte range of each line's content (line endings excluded), aligned with
/// `content_lines` in app.rs: empty text is one empty line, and a trailing
/// newline does not produce a final empty line.
fn line_byte_ranges(text: &str) -> Vec<Range<usize>> {
    if text.is_empty() {
        return vec![Range::default()];
    }

    let mut ranges = Vec::new();
    let mut offset = 0;
    for chunk in text.split_inclusive('\n') {
        let content = chunk.trim_end_matches(['\n', '\r']);
        ranges.push(offset..offset + content.len());
        offset += chunk.len();
    }
    ranges
}

/// Maps a repository path to the language hint for `SyntaxHighlighter`.
/// gpui-component resolves extensions like "py"/"rs" directly and falls
/// back to plain text for anything it does not recognize.
pub fn language_for_path(path: &str) -> &str {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
}

/// Byte ranges of the segments that differ between two aligned lines,
/// computed word-by-word. Returns (ranges in old line, ranges in new line).
pub fn inline_diff_ranges(
    old_line: &str,
    new_line: &str,
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let diff = TextDiff::from_words(old_line, new_line);
    let old_offsets = token_byte_offsets(diff.old_slices());
    let new_offsets = token_byte_offsets(diff.new_slices());

    let mut old_ranges: Vec<Range<usize>> = Vec::new();
    let mut new_ranges: Vec<Range<usize>> = Vec::new();
    for op in diff.ops() {
        if op.tag() == DiffTag::Equal {
            continue;
        }
        push_merged(&mut old_ranges, token_span(&old_offsets, op.old_range()));
        push_merged(&mut new_ranges, token_span(&new_offsets, op.new_range()));
    }

    (old_ranges, new_ranges)
}

/// Prefix-sum byte offsets for diff tokens: offsets[i] is where token i
/// starts; the final entry is the total length.
fn token_byte_offsets(tokens: &[&str]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(tokens.len() + 1);
    let mut offset = 0;
    offsets.push(0);
    for token in tokens {
        offset += token.len();
        offsets.push(offset);
    }
    offsets
}

fn token_span(offsets: &[usize], token_range: Range<usize>) -> Range<usize> {
    offsets[token_range.start]..offsets[token_range.end]
}

fn push_merged(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = ranges.last_mut() {
        if last.end == range.start {
            last.end = range.end;
            return;
        }
    }
    ranges.push(range);
}

/// Overlays an emphasis background onto syntax runs, producing a flat,
/// non-overlapping, sorted run list suitable for `StyledText`.
///
/// Syntax runs are assumed sorted and non-overlapping (tree-sitter highlight
/// output is). Lines are short, so the per-segment linear scans beat fancier
/// interval structures.
pub fn merge_emphasis(
    syntax: &[(Range<usize>, HighlightStyle)],
    emphasis: &[Range<usize>],
    emphasis_background: gpui::Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut bounds: Vec<usize> = Vec::new();
    for (range, _) in syntax {
        bounds.push(range.start);
        bounds.push(range.end);
    }
    for range in emphasis {
        bounds.push(range.start);
        bounds.push(range.end);
    }
    bounds.sort_unstable();
    bounds.dedup();

    let mut merged: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for pair in bounds.windows(2) {
        let segment = pair[0]..pair[1];
        let syntax_style = syntax
            .iter()
            .find(|(range, _)| range.start <= segment.start && segment.end <= range.end)
            .map(|(_, style)| *style);
        let emphasized = emphasis
            .iter()
            .any(|range| range.start <= segment.start && segment.end <= range.end);

        if syntax_style.is_none() && !emphasized {
            continue;
        }
        let mut style = syntax_style.unwrap_or_default();
        if emphasized {
            style.background_color = Some(emphasis_background);
        }

        if let Some((last_range, last_style)) = merged.last_mut() {
            if last_range.end == segment.start && *last_style == style {
                last_range.end = segment.end;
                continue;
            }
        }
        merged.push((segment, style));
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_theme_parses_from_embedded_json() {
        let theme = material_theme();
        assert_eq!(theme.name, "OpenCode Material Dark");
        assert!(
            theme.style.editor_foreground.is_some(),
            "editor.foreground should deserialize"
        );
        assert!(
            theme.style.syntax.keyword.is_some(),
            "syntax.keyword should deserialize"
        );
    }

    #[test]
    fn python_source_yields_per_line_highlight_runs() {
        let runs = line_highlight_runs("def foo():\n    return 1\n", "py");
        assert_eq!(runs.len(), 2);
        assert!(
            !runs[0].is_empty(),
            "a Python def line should carry at least one syntax run"
        );
        // Runs are line-relative: nothing may extend past the line's length.
        assert!(runs[0]
            .iter()
            .all(|(range, _)| range.end <= "def foo():".len()));
    }

    #[test]
    fn empty_text_yields_a_single_empty_line() {
        assert_eq!(line_highlight_runs("", "py").len(), 1);
    }

    #[test]
    fn line_count_matches_content_lines_for_trailing_newline() {
        // Mirrors content_lines() in app.rs: "a\nb\n" is two lines, not three.
        assert_eq!(line_highlight_runs("a\nb\n", "py").len(), 2);
    }

    #[test]
    fn language_for_path_extracts_the_extension() {
        assert_eq!(language_for_path("src/cli.py"), "py");
        assert_eq!(language_for_path("Makefile"), "");
    }

    #[test]
    fn unknown_language_emits_only_unstyled_runs() {
        let runs = line_highlight_runs("hello world\n", "zzz-not-a-language");
        assert!(
            runs[0].iter().all(|(_, style)| style.color.is_none()),
            "plain-text fallback must not invent colors"
        );
    }

    #[test]
    fn inline_diff_ranges_isolate_the_changed_token() {
        let (old, new) = inline_diff_ranges("threshold = 10", "threshold = None");
        assert_eq!(old, vec![12..14]);
        assert_eq!(new, vec![12..16]);
    }

    #[test]
    fn inline_diff_ranges_keep_unchanged_whitespace_unemphasized() {
        // "b" and "c" change but the space between them does not; each word
        // gets its own precise range rather than one merged block.
        let (old, new) = inline_diff_ranges("a b c", "a x y");
        assert_eq!(old, vec![2..3, 4..5]);
        assert_eq!(new, vec![2..3, 4..5]);
    }

    #[test]
    fn merge_emphasis_overlays_background_on_syntax_runs() {
        let syntax = vec![(
            0..4,
            HighlightStyle {
                color: Some(gpui::red()),
                ..Default::default()
            },
        )];
        let merged = merge_emphasis(&syntax, std::slice::from_ref(&(2..6)), gpui::blue());

        // Three segments: syntax only, syntax + emphasis, emphasis only.
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].0, 0..2);
        assert!(merged[0].1.background_color.is_none());
        assert_eq!(merged[1].0, 2..4);
        assert_eq!(merged[1].1.color, Some(gpui::red()));
        assert_eq!(merged[1].1.background_color, Some(gpui::blue()));
        assert_eq!(merged[2].0, 4..6);
        assert!(merged[2].1.color.is_none());
    }

    #[test]
    fn merge_emphasis_coalesces_adjacent_equal_style_segments() {
        let syntax = vec![(
            0..6,
            HighlightStyle {
                color: Some(gpui::red()),
                ..Default::default()
            },
        )];
        let merged = merge_emphasis(&syntax, &[2..3, 3..5], gpui::blue());

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].0, 2..5);
        assert_eq!(merged[1].1.background_color, Some(gpui::blue()));
    }

    #[test]
    fn inline_diff_ranges_are_byte_ranges_for_multibyte_text() {
        // "héllo" is 6 bytes; the changed second word starts at byte 7.
        let (old, new) = inline_diff_ranges("héllo wörld", "héllo wxrld");
        assert_eq!(old, vec![7..13]);
        assert_eq!(new, vec![7..12]);
    }
}
