//! Integration test for the production-side repository layer.

pub mod common;

use greviewer::repo::{
    changeset_for_single_commit, file_diff_for_changed_file, open_at, ChangeKind, FileDiffContent,
    OpenError,
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
