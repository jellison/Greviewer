//! Window-chrome title bar: the `{repo} / {sha}` context segment shown in
//! changeset mode and the popover it opens. See
//! docs/specs/review/workflow.md and
//! docs/superpowers/specs/2026-06-07-titlebar-context-switcher-design.md.

use super::Selection;
use crate::repo::{ChangeSet, CommitInfo};

/// First seven characters of a full commit sha, matching the short form the
/// graph and the removed diff header used.
fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// Label for the title-bar pill. A single-commit changeset shows just the
/// short sha; a range shows the newest short sha plus the commit count.
fn context_pill_label(selection: &Selection, changeset: &ChangeSet) -> String {
    // In a well-formed call `changeset.commit_sha` is the newest commit, i.e.
    // `shas[0]` for a range, so the pill always shows the newest short sha.
    let newest = short_sha(&changeset.commit_sha);
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            format!("{newest} · {} commits", shas.len())
        }
        _ => newest,
    }
}

/// Oldest…newest short shas for a range changeset; `None` for a single commit.
/// `shas` is newest-first, so the first entry is the newest endpoint.
fn range_endpoints(selection: &Selection) -> Option<(String, String)> {
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            let newest = shas.first()?;
            let oldest = shas.last()?;
            Some((short_sha(oldest), short_sha(newest)))
        }
        _ => None,
    }
}

/// Popover header title. A range reads "Reviewing N commits"; a single commit
/// reads the commit summary, falling back to its short sha when the commit is
/// not in the loaded window.
fn popover_header_title(
    selection: &Selection,
    changeset: &ChangeSet,
    commits: &[CommitInfo],
) -> String {
    match selection {
        Selection::Range { shas, .. } if shas.len() > 1 => {
            format!("Reviewing {} commits", shas.len())
        }
        _ => commits
            .iter()
            .find(|commit| commit.sha == changeset.commit_sha)
            .map(|commit| commit.summary.clone())
            .unwrap_or_else(|| short_sha(&changeset.commit_sha)),
    }
}

/// Total added and removed lines across every file in the changeset.
fn changeset_line_totals(changeset: &ChangeSet) -> (usize, usize) {
    changeset.files.iter().fold((0, 0), |(added, removed), file| {
        (added + file.line_stats.added, removed + file.line_stats.removed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{ChangeKind, ChangedFile, ChangeSet, CommitInfo, LineStats};

    fn changeset_with(commit_sha: &str, files: Vec<ChangedFile>) -> ChangeSet {
        ChangeSet {
            commit_sha: commit_sha.to_string(),
            base_sha: None,
            files,
        }
    }

    fn file_with(added: usize, removed: usize) -> ChangedFile {
        ChangedFile {
            path: "file.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            is_binary: false,
            line_stats: LineStats { added, removed },
        }
    }

    fn commit_with(sha: &str, summary: &str) -> CommitInfo {
        CommitInfo {
            sha: sha.to_string(),
            short_sha: short_sha(sha),
            summary: summary.to_string(),
            author: "Tester".to_string(),
            authored_timestamp: 0,
            authored_date: "2026-06-07".to_string(),
            parent_shas: Vec::new(),
            branch_names: Vec::new(),
            parent_count: 0,
            is_head: false,
        }
    }

    #[test]
    fn pill_label_for_single_commit_is_the_short_sha() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(context_pill_label(&selection, &changeset), "abcdef1");
    }

    #[test]
    fn pill_label_for_range_appends_commit_count() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "1111111111111111".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(context_pill_label(&selection, &changeset), "abcdef1 · 3 commits");
    }

    #[test]
    fn range_endpoints_are_oldest_then_newest() {
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(
            range_endpoints(&selection),
            Some(("0000000".to_string(), "abcdef1".to_string()))
        );
    }

    #[test]
    fn range_endpoints_is_none_for_single_commit() {
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(range_endpoints(&selection), None);
    }

    #[test]
    fn header_title_for_range_counts_commits() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Range {
            start_sha: "0000000000000000".to_string(),
            end_sha: "abcdef1234567890".to_string(),
            shas: vec![
                "abcdef1234567890".to_string(),
                "1111111111111111".to_string(),
                "0000000000000000".to_string(),
            ],
        };
        assert_eq!(
            popover_header_title(&selection, &changeset, &[]),
            "Reviewing 3 commits"
        );
    }

    #[test]
    fn header_title_for_single_commit_uses_the_summary() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        let commits = vec![commit_with("abcdef1234567890", "feat: do the thing")];
        assert_eq!(
            popover_header_title(&selection, &changeset, &commits),
            "feat: do the thing"
        );
    }

    #[test]
    fn header_title_falls_back_to_short_sha_when_commit_missing() {
        let changeset = changeset_with("abcdef1234567890", vec![]);
        let selection = Selection::Single {
            sha: "abcdef1234567890".to_string(),
        };
        assert_eq!(popover_header_title(&selection, &changeset, &[]), "abcdef1");
    }

    #[test]
    fn line_totals_sum_every_file() {
        let changeset = changeset_with(
            "abcdef1234567890",
            vec![file_with(10, 2), file_with(5, 95)],
        );
        assert_eq!(changeset_line_totals(&changeset), (15, 97));
    }
}
