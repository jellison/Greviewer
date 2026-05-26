//! Integration test for the production-side repository layer.

pub mod common;

use greviewer::repo::{open_at, OpenError};

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
fn open_at_rejects_a_non_repository_directory() {
    let dir = tempfile::tempdir().expect("create tempdir");

    let err = open_at(dir.path()).expect_err("open fails");

    assert!(
        matches!(err, OpenError::NotARepository),
        "expected NotARepository, got {err:?}"
    );
}
