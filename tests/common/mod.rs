//! Test helpers shared across integration tests.
//!
//! The `build_repo` builder constructs a fresh Git repository in a temp
//! directory with a deterministic author/committer signature and the requested
//! commit shape. Tests that care about specific Git topology should prefer this
//! builder over checked-in fixtures because it is self-documenting.

use git2::{IndexAddOption, Repository, Signature};
use std::{fs, path::Path};
use tempfile::TempDir;

pub struct CommitSpec {
    pub message: String,
    pub changes: Vec<FileChange>,
}

pub struct FileChange {
    pub path: String,
    pub content: String,
}

pub fn build_repo(commits: &[CommitSpec]) -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = Repository::init(dir.path()).expect("init repo");

    let signature =
        Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");

    let mut parent_oid = None;
    for commit in commits {
        for change in &commit.changes {
            let path = dir.path().join(&change.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent dirs");
            }
            fs::write(&path, &change.content).expect("write file");
        }

        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let parents: Vec<_> = match parent_oid {
            Some(oid) => vec![repo.find_commit(oid).expect("find parent")],
            None => vec![],
        };
        let parents: Vec<&_> = parents.iter().collect();

        let oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                &commit.message,
                &tree,
                &parents,
            )
            .expect("create commit");
        parent_oid = Some(oid);
    }

    drop(repo);
    dir
}

pub fn load_fixture(name: &str) -> TempDir {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let source_dot_git = fixture_root.join("dot-git");
    assert!(
        source_dot_git.is_dir(),
        "fixture not found: {}",
        source_dot_git.display()
    );

    let dest = tempfile::tempdir().expect("create tempdir for fixture");
    copy_dir_recursive(&source_dot_git, &dest.path().join(".git"));
    dest
}

fn copy_dir_recursive(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dest dir");
    for entry in fs::read_dir(from).expect("read source dir") {
        let entry = entry.expect("read dir entry");
        let file_type = entry.file_type().expect("read file type");
        let dest = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).expect("copy file");
        }
    }
}
