//! Repository data layer.
//!
//! Wraps `git2::Repository` to produce small snapshot values the UI can render
//! without holding a live libgit2 handle. The snapshot is a one-shot read at
//! open time; live updates come in a later slice.

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct OpenRepository {
    pub path: PathBuf,
    pub head: Option<HeadInfo>,
}

#[derive(Debug, Clone)]
pub struct HeadInfo {
    pub short_sha: String,
    pub summary: String,
}

#[derive(Debug)]
pub enum OpenError {
    NotARepository,
    Io(io::Error),
    Git(git2::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NotARepository => write!(f, "That folder isn't a Git repository."),
            OpenError::Io(err) => write!(f, "Couldn't open that folder: {err}."),
            OpenError::Git(err) => write!(f, "Couldn't read that repository: {err}."),
        }
    }
}

impl std::error::Error for OpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OpenError::NotARepository => None,
            OpenError::Io(err) => Some(err),
            OpenError::Git(err) => Some(err),
        }
    }
}

pub fn open_at(path: &Path) -> Result<OpenRepository, OpenError> {
    let canonical = path.canonicalize().map_err(OpenError::Io)?;

    let repo = git2::Repository::open(&canonical).map_err(classify_open_error)?;
    let head = read_head(&repo)?;

    Ok(OpenRepository {
        path: canonical,
        head,
    })
}

fn classify_open_error(err: git2::Error) -> OpenError {
    use git2::ErrorCode;
    match err.code() {
        ErrorCode::NotFound => OpenError::NotARepository,
        _ => OpenError::Git(err),
    }
}

fn read_head(repo: &git2::Repository) -> Result<Option<HeadInfo>, OpenError> {
    match repo.head() {
        Ok(reference) => {
            let commit = reference.peel_to_commit().map_err(OpenError::Git)?;
            let short_sha = commit.id().to_string()[..7].to_string();
            let summary = commit.summary().unwrap_or("").trim_end().to_string();
            Ok(Some(HeadInfo { short_sha, summary }))
        }
        Err(err) if err.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(err) => Err(OpenError::Git(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{IndexAddOption, Repository, Signature};
    use std::fs;
    use tempfile::TempDir;

    fn init_repo_with_one_commit() -> (TempDir, String) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");

        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let signature =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");

        let oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Add hello.txt",
                &tree,
                &[],
            )
            .expect("create commit");

        drop(tree);
        drop(index);
        drop(repo);
        (dir, oid.to_string())
    }

    fn init_unborn_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        Repository::init(dir.path()).expect("init repo");
        dir
    }

    #[test]
    fn open_at_returns_head_info_for_a_normal_repo() {
        let (dir, oid_hex) = init_repo_with_one_commit();

        let snapshot = open_at(dir.path()).expect("open succeeds");
        let head = snapshot.head.expect("head present");

        assert_eq!(head.short_sha.len(), 7, "short sha is 7 chars");
        assert_eq!(head.short_sha, &oid_hex[..7]);
        assert_eq!(head.summary, "Add hello.txt");
    }

    #[test]
    fn open_at_returns_no_head_for_an_unborn_repo() {
        let dir = init_unborn_repo();

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(snapshot.head.is_none(), "unborn HEAD yields head: None");
    }

    #[test]
    fn open_at_returns_not_a_repository_for_a_plain_directory() {
        let dir = tempfile::tempdir().expect("create tempdir");

        let err = open_at(dir.path()).expect_err("open fails");

        assert!(
            matches!(err, OpenError::NotARepository),
            "expected NotARepository, got {err:?}"
        );
    }

    #[test]
    fn open_at_rejects_a_missing_path() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let missing = dir.path().join("does-not-exist");

        let err = open_at(&missing).expect_err("open fails");

        assert!(
            matches!(err, OpenError::NotARepository | OpenError::Io(_)),
            "expected NotARepository or Io, got {err:?}"
        );
    }
}
