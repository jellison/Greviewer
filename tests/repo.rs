//! Integration test for the production-side repository layer.

pub mod common;

use greviewer::repo::{
    changeset_for_pending, changeset_for_single_commit, file_diff_for_changed_file, open_at,
    ChangeKind, FileDiffContent, OpenError,
};

#[test]
fn open_at_reads_the_two_commits_fixture() {
    let dir = common::load_fixture("two-commits");

    let snapshot = open_at(dir.path()).expect("open succeeds");
    let head = snapshot.head.expect("head present");

    assert_eq!(head.short_sha.len(), 7);
    assert_eq!(head.summary, "Update hello.txt");
    assert_eq!(snapshot.commits.len(), 2);
    assert_eq!(snapshot.commits[0].summary, "Update hello.txt");
    assert_eq!(snapshot.commits[1].summary, "Add hello.txt");
    assert!(snapshot.commits[0].is_head);
    assert!(!snapshot.commits[1].is_head);
    assert_eq!(snapshot.commits[0].short_sha.len(), 7);
    assert!(!snapshot.commits[0].sha.is_empty());
    assert!(!snapshot.commits[0].author.is_empty());
    assert!(!snapshot.commits[0].authored_date.is_empty());
}

#[test]
fn changeset_for_fixture_head_lists_modified_hello_file() {
    let dir = common::load_fixture("two-commits");
    let snapshot = open_at(dir.path()).expect("open succeeds");

    let changeset =
        changeset_for_single_commit(dir.path(), &snapshot.commits[0].sha).expect("changeset");

    assert_eq!(changeset.commit_sha, snapshot.commits[0].sha);
    assert_eq!(
        changeset.base_sha.as_deref(),
        Some(snapshot.commits[1].sha.as_str()),
    );
    assert_eq!(changeset.files.len(), 1);
    assert_eq!(changeset.files[0].path, "hello.txt");
    assert_eq!(changeset.files[0].old_path, None);
    assert_eq!(changeset.files[0].kind, ChangeKind::Modified);
}

#[test]
fn file_diff_for_fixture_head_reads_modified_hello_content() {
    let dir = common::load_fixture("two-commits");
    let snapshot = open_at(dir.path()).expect("open succeeds");
    let changeset =
        changeset_for_single_commit(dir.path(), &snapshot.commits[0].sha).expect("changeset");

    let diff =
        file_diff_for_changed_file(dir.path(), &snapshot.commits[0].sha, &changeset.files[0])
            .expect("file diff");

    assert_eq!(
        diff.content,
        FileDiffContent::SideBySide {
            old_text: "hello\n".to_string(),
            new_text: "hello world\n".to_string(),
        },
    );
}

#[test]
fn open_at_rejects_a_non_repository_directory() {
    let dir = tempfile::tempdir().expect("create tempdir");

    let err = open_at(dir.path()).expect_err("open fails");

    assert!(
        matches!(err, OpenError::NotARepository),
        "expected NotARepository, got {err:?}"
    );
}

#[test]
fn pending_changeset_reports_staged_unstaged_and_untracked_files() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![
            common::FileChange {
                path: "staged.txt".to_string(),
                content: "one\n".to_string(),
            },
            common::FileChange {
                path: "unstaged.txt".to_string(),
                content: "two\n".to_string(),
            },
        ],
    }]);

    // Unstaged edit to a tracked file.
    std::fs::write(dir.path().join("unstaged.txt"), "two changed\n").unwrap();
    // Staged edit to a tracked file.
    std::fs::write(dir.path().join("staged.txt"), "one changed\n").unwrap();
    let repo = git2::Repository::open(dir.path()).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("staged.txt")).unwrap();
    index.write().unwrap();
    // Untracked file.
    std::fs::write(dir.path().join("untracked.txt"), "new\n").unwrap();

    let changeset = changeset_for_pending(dir.path()).expect("pending changeset");

    assert_eq!(changeset.commit_sha, greviewer::repo::PENDING_SHA);
    let head_sha = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id()
        .to_string();
    assert_eq!(changeset.base_sha.as_deref(), Some(head_sha.as_str()));

    let paths: Vec<(&str, ChangeKind)> = changeset
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.kind))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("staged.txt", ChangeKind::Modified),
            ("unstaged.txt", ChangeKind::Modified),
            ("untracked.txt", ChangeKind::Added),
        ]
    );
    let untracked = changeset
        .files
        .iter()
        .find(|f| f.path == "untracked.txt")
        .unwrap();
    assert_eq!(untracked.line_stats.added, 1);
}

#[test]
fn pending_changeset_honors_gitignore_and_is_empty_when_clean() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: ".gitignore".to_string(),
            content: "ignored.txt\n".to_string(),
        }],
    }]);

    assert!(changeset_for_pending(dir.path()).unwrap().files.is_empty());

    std::fs::write(dir.path().join("ignored.txt"), "hidden\n").unwrap();
    assert!(changeset_for_pending(dir.path()).unwrap().files.is_empty());
}

#[test]
fn pending_changeset_diffs_against_the_empty_tree_on_unborn_head() {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("first.txt"), "hello\n").unwrap();

    let changeset = changeset_for_pending(dir.path()).expect("pending changeset");

    assert_eq!(changeset.base_sha, None);
    assert_eq!(changeset.files.len(), 1);
    assert_eq!(changeset.files[0].kind, ChangeKind::Added);
}

#[test]
fn pending_changeset_reports_deleted_files() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: "doomed.txt".to_string(),
            content: "bye\n".to_string(),
        }],
    }]);
    std::fs::remove_file(dir.path().join("doomed.txt")).unwrap();

    let changeset = changeset_for_pending(dir.path()).expect("pending changeset");
    assert_eq!(changeset.files[0].kind, ChangeKind::Deleted);
    assert_eq!(changeset.files[0].line_stats.removed, 1);
}

#[test]
fn pending_summary_totals_files_and_lines() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: "a.txt".to_string(),
            content: "one\ntwo\n".to_string(),
        }],
    }]);
    std::fs::write(dir.path().join("a.txt"), "one\nthree\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();

    let summary = greviewer::repo::read_pending_summary(dir.path()).expect("pending summary");

    assert!(summary.is_dirty());
    assert_eq!(summary.file_count, 2);
    assert_eq!(summary.line_stats.added, 2); // "three" + "new"
    assert_eq!(summary.line_stats.removed, 1); // "two"
}

#[test]
fn pending_summary_is_clean_for_an_unmodified_tree() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: "a.txt".to_string(),
            content: "one\n".to_string(),
        }],
    }]);

    let summary = greviewer::repo::read_pending_summary(dir.path()).expect("pending summary");
    assert!(!summary.is_dirty());
    assert_eq!(summary, greviewer::repo::PendingSummary::default());
}

#[test]
fn pending_file_diff_reads_old_from_head_and_new_from_disk() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: "a.txt".to_string(),
            content: "old\n".to_string(),
        }],
    }]);
    std::fs::write(dir.path().join("a.txt"), "new\n").unwrap();

    let changeset = greviewer::repo::changeset_for_pending(dir.path()).unwrap();
    let file = &changeset.files[0];
    let diff = greviewer::repo::file_diff_for_pending_file(dir.path(), file).expect("pending diff");

    assert_eq!(
        diff.content,
        greviewer::repo::FileDiffContent::SideBySide {
            old_text: "old\n".to_string(),
            new_text: "new\n".to_string(),
        }
    );
}

#[test]
fn pending_file_diff_shows_untracked_files_full_width() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: "a.txt".to_string(),
            content: "x\n".to_string(),
        }],
    }]);
    std::fs::write(dir.path().join("fresh.txt"), "hello\n").unwrap();

    let changeset = greviewer::repo::changeset_for_pending(dir.path()).unwrap();
    let file = changeset
        .files
        .iter()
        .find(|f| f.path == "fresh.txt")
        .unwrap();
    let diff = greviewer::repo::file_diff_for_pending_file(dir.path(), file).unwrap();

    assert_eq!(
        diff.content,
        greviewer::repo::FileDiffContent::Single {
            side: greviewer::repo::DiffSide::New,
            text: "hello\n".to_string(),
        }
    );
}

#[test]
fn files_for_pending_adjusts_head_files_by_the_changeset() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![
            common::FileChange {
                path: "kept.txt".to_string(),
                content: "k\n".to_string(),
            },
            common::FileChange {
                path: "doomed.txt".to_string(),
                content: "d\n".to_string(),
            },
        ],
    }]);
    std::fs::remove_file(dir.path().join("doomed.txt")).unwrap();
    std::fs::write(dir.path().join("fresh.txt"), "f\n").unwrap();

    let files = greviewer::repo::files_for_pending(dir.path()).expect("pending files");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["fresh.txt", "kept.txt"]);
}

#[test]
fn file_content_in_worktree_reads_disk_state() {
    let dir = common::build_repo(&[common::CommitSpec {
        message: "seed".to_string(),
        changes: vec![common::FileChange {
            path: "a.txt".to_string(),
            content: "seed\n".to_string(),
        }],
    }]);

    let content = greviewer::repo::file_content_in_worktree(dir.path(), "a.txt").unwrap();
    assert_eq!(
        content.content,
        greviewer::repo::FileContentBody::Text("seed\n".to_string())
    );
}
