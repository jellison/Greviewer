//! Repository data layer.
//!
//! Wraps `git2::Repository` to produce small snapshot values the UI can render
//! without holding a live libgit2 handle. The snapshot is a one-shot read at
//! open time; live updates come in a later slice.

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

pub const INITIAL_COMMIT_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct OpenRepository {
    pub path: PathBuf,
    pub head: Option<HeadInfo>,
    pub commits: Vec<CommitInfo>,
}

#[derive(Debug, Clone)]
pub struct HeadInfo {
    pub short_sha: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub summary: String,
    pub author: String,
    pub authored_timestamp: i64,
    pub authored_date: String,
    pub parent_count: usize,
    pub is_head: bool,
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
    let head_commit = read_head_commit(&repo)?;
    let head_oid = head_commit.as_ref().map(|commit| commit.id());
    let head = head_commit.as_ref().map(head_info_from_commit);
    let commits = read_commits(&repo, head_oid, INITIAL_COMMIT_LIMIT)?;

    Ok(OpenRepository {
        path: canonical,
        head,
        commits,
    })
}

fn classify_open_error(err: git2::Error) -> OpenError {
    use git2::ErrorCode;
    match err.code() {
        ErrorCode::NotFound => OpenError::NotARepository,
        _ => OpenError::Git(err),
    }
}

fn read_head_commit(repo: &git2::Repository) -> Result<Option<git2::Commit<'_>>, OpenError> {
    match repo.head() {
        Ok(reference) => reference.peel_to_commit().map(Some).map_err(OpenError::Git),
        Err(err) if err.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(err) => Err(OpenError::Git(err)),
    }
}

fn read_commits(
    repo: &git2::Repository,
    head_oid: Option<git2::Oid>,
    limit: usize,
) -> Result<Vec<CommitInfo>, OpenError> {
    if head_oid.is_none() {
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk().map_err(OpenError::Git)?;
    revwalk
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(OpenError::Git)?;
    revwalk.push_head().map_err(OpenError::Git)?;

    let mut commits = Vec::new();
    for oid in revwalk.take(limit) {
        let oid = oid.map_err(OpenError::Git)?;
        let commit = repo.find_commit(oid).map_err(OpenError::Git)?;
        commits.push(commit_info_from_commit(&commit, head_oid));
    }

    Ok(commits)
}

fn head_info_from_commit(commit: &git2::Commit<'_>) -> HeadInfo {
    HeadInfo {
        short_sha: short_sha(commit.id()),
        summary: commit.summary().unwrap_or("").trim_end().to_string(),
    }
}

fn commit_info_from_commit(commit: &git2::Commit<'_>, head_oid: Option<git2::Oid>) -> CommitInfo {
    let sha = commit.id().to_string();
    let author = commit.author();
    let author = match author.name().map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "Unknown author".to_string(),
    };

    CommitInfo {
        short_sha: short_sha(commit.id()),
        sha,
        summary: commit.summary().unwrap_or("").trim_end().to_string(),
        author,
        authored_timestamp: commit.time().seconds(),
        authored_date: format_authored_date(commit.time()),
        parent_count: commit.parent_count(),
        is_head: Some(commit.id()) == head_oid,
    }
}

fn short_sha(oid: git2::Oid) -> String {
    oid.to_string().chars().take(7).collect()
}

fn format_authored_date(time: git2::Time) -> String {
    let local_seconds = time
        .seconds()
        .saturating_add(i64::from(time.offset_minutes()) * 60);
    let local_days = local_seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_unix_days(local_days);

    format!("{year:04}-{month:02}-{day:02}")
}

// Converts days since the Unix epoch to a civil date using Howard Hinnant's
// public-domain calendar algorithm.
fn civil_from_unix_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
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
    fn open_at_returns_commits_newest_first() {
        let (dir, oid_hex) = init_repo_with_one_commit();

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert_eq!(snapshot.commits.len(), 1);
        assert_eq!(snapshot.commits[0].sha, oid_hex);
        assert_eq!(snapshot.commits[0].summary, "Add hello.txt");
        assert!(snapshot.commits[0].is_head);
    }

    #[test]
    fn open_at_returns_no_head_for_an_unborn_repo() {
        let dir = init_unborn_repo();

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(snapshot.head.is_none(), "unborn HEAD yields head: None");
    }

    #[test]
    fn open_at_returns_empty_commits_for_an_unborn_repo() {
        let dir = init_unborn_repo();

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(snapshot.head.is_none());
        assert!(snapshot.commits.is_empty());
    }

    #[test]
    fn civil_date_formatting_handles_unix_epoch() {
        let date = format_authored_date(git2::Time::new(0, 0));

        assert_eq!(date, "1970-01-01");
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
