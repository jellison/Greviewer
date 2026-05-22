//! Exercises the synthetic-repo builder helper from `tests/common`.

mod common;

use common::{build_repo, CommitSpec, FileChange};

#[test]
fn builds_a_repo_with_two_commits() {
    let repo_dir = build_repo(&[
        CommitSpec {
            message: "Add hello.txt".to_string(),
            changes: vec![FileChange {
                path: "hello.txt".to_string(),
                content: "hello\n".to_string(),
            }],
        },
        CommitSpec {
            message: "Update hello.txt".to_string(),
            changes: vec![FileChange {
                path: "hello.txt".to_string(),
                content: "hello world\n".to_string(),
            }],
        },
    ]);

    let repo = git2::Repository::open(repo_dir.path()).expect("open built repo");
    let head = repo
        .head()
        .expect("HEAD exists")
        .peel_to_commit()
        .expect("HEAD commit");
    // git2::Repository::commit does not append trailing newlines; CLI `git commit` does.
    assert_eq!(head.message(), Some("Update hello.txt"));

    let mut walk = repo.revwalk().expect("revwalk");
    walk.push_head().expect("push head");
    let count = walk.count();
    assert_eq!(count, 2, "expected exactly two commits");
}

#[test]
fn load_fixture_yields_a_real_repo() {
    let repo_dir = common::load_fixture("two-commits");

    let repo = git2::Repository::open(repo_dir.path()).expect("open fixture repo");
    let head = repo
        .head()
        .expect("HEAD exists")
        .peel_to_commit()
        .expect("HEAD commit");
    assert_eq!(head.message(), Some("Update hello.txt\n"));

    let mut walk = repo.revwalk().expect("revwalk");
    walk.push_head().expect("push head");
    let count = walk.count();
    assert_eq!(count, 2, "expected exactly two commits");
}
