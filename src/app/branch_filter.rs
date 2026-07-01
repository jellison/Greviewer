//! Fuzzy filtering for the branch sidebar: a case-insensitive subsequence
//! matcher plus helpers that map whole-path matches onto the tree's
//! `/`-separated segments and into `StyledText` byte ranges. Pure and
//! dependency-free; see docs/adr/0002-project-layout.md.

/// Greedily match `query` against `haystack` left to right, case-insensitively,
/// consuming each query character at the earliest position that equals it.
/// Returns the char indices of `haystack` consumed, in order. Stops early when
/// the query is exhausted; if the query is not fully consumable, returns the
/// indices matched so far (a prefix of the query).
pub(crate) fn prefix_match_indices(haystack: &str, query: &str) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut query_iter = query.chars();
    let mut next_query = query_iter.next();
    for (index, hay_char) in haystack.chars().enumerate() {
        let Some(q) = next_query else { break };
        if chars_eq_ignore_case(hay_char, q) {
            indices.push(index);
            next_query = query_iter.next();
        }
    }
    indices
}

fn chars_eq_ignore_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Full subsequence match. `Some(indices)` only when every query character was
/// matched; an empty query yields `Some(vec![])`. `None` otherwise.
pub(crate) fn fuzzy_match(haystack: &str, query: &str) -> Option<Vec<usize>> {
    let indices = prefix_match_indices(haystack, query);
    if indices.len() == query.chars().count() {
        Some(indices)
    } else {
        None
    }
}

/// Given char-index matches into `path`, return those that fall within the
/// final `/`-separated segment, rebased so 0 is the first char of that segment.
pub(crate) fn final_segment_highlights(path: &str, match_indices: &[usize]) -> Vec<usize> {
    let total = path.chars().count();
    let final_len = path
        .rsplit('/')
        .next()
        .map(|s| s.chars().count())
        .unwrap_or(0);
    let offset = total - final_len;
    match_indices
        .iter()
        .filter(|&&i| i >= offset)
        .map(|&i| i - offset)
        .collect()
}

/// Convert sorted char indices within `text` into coalesced byte ranges.
pub(crate) fn highlight_byte_ranges(
    text: &str,
    char_indices: &[usize],
) -> Vec<std::ops::Range<usize>> {
    let byte_of_char: Vec<usize> = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for &ci in char_indices {
        if ci + 1 >= byte_of_char.len() {
            continue;
        }
        let start = byte_of_char[ci];
        let end = byte_of_char[ci + 1];
        if let Some(last) = ranges.last_mut() {
            if last.end == start {
                last.end = end;
                continue;
            }
        }
        ranges.push(start..end);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything_with_no_indices() {
        assert_eq!(fuzzy_match("origin/feature/login", ""), Some(vec![]));
        assert_eq!(prefix_match_indices("anything", ""), Vec::<usize>::new());
    }

    #[test]
    fn subsequence_match_returns_char_indices() {
        assert_eq!(fuzzy_match("login", "lgn"), Some(vec![0, 2, 4]));
    }

    #[test]
    fn non_subsequence_returns_none() {
        assert_eq!(fuzzy_match("login", "xyz"), None);
        assert_eq!(fuzzy_match("login", "lo!"), None);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(fuzzy_match("Login", "lOG"), Some(vec![0, 1, 2]));
    }

    #[test]
    fn match_spans_slash_boundaries() {
        // 'o'@0 (origin), 'f'@7 (feature), 'l'@15 (login)
        let path = "origin/feature/login";
        assert_eq!(fuzzy_match(path, "ofl"), Some(vec![0, 7, 15]));
    }

    #[test]
    fn prefix_match_returns_partial_consumption() {
        // Only 'o' and 'f' live in "origin/feature"; the trailing 'l' does not.
        assert_eq!(prefix_match_indices("origin/feature", "ofl"), vec![0, 7]);
    }

    #[test]
    fn final_segment_highlights_rebases_to_leaf() {
        let path = "origin/feature/login";
        // Full match "ofl" → indices [0, 7, 15]; leaf "login" starts at char 15.
        let leaf = final_segment_highlights(path, &[0, 7, 15]);
        assert_eq!(leaf, vec![0]); // 'l' at leaf-relative index 0
    }

    #[test]
    fn final_segment_highlights_for_a_folder_prefix() {
        // Folder "origin/feature": prefix match "of" → [0, 7]; final segment
        // "feature" starts at char 7, so index 7 rebases to 0.
        let indices = prefix_match_indices("origin/feature", "of");
        assert_eq!(
            final_segment_highlights("origin/feature", &indices),
            vec![0]
        );
    }

    #[test]
    fn highlight_byte_ranges_coalesces_adjacent_chars() {
        // "login", highlight chars 0,1,2 → one range 0..3.
        assert_eq!(highlight_byte_ranges("login", &[0, 1, 2]), vec![0..3]);
        // Non-adjacent chars 0 and 4 → two ranges.
        assert_eq!(highlight_byte_ranges("login", &[0, 4]), vec![0..1, 4..5]);
    }

    #[test]
    fn highlight_byte_ranges_handles_multibyte_chars() {
        // "café": c,a,f are 1 byte each (0,1,2); é starts at byte 3 and is
        // 2 bytes, so char index 3 maps to the byte range 3..5.
        assert_eq!(highlight_byte_ranges("café", &[3]), vec![3..5]);
        // Coalescing across a multi-byte boundary: chars 2 ("f") and 3 ("é")
        // are adjacent, yielding one contiguous range 2..5.
        assert_eq!(highlight_byte_ranges("café", &[2, 3]), vec![2..5]);
    }
}
