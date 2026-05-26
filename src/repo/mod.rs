//! Repository data layer.
//!
//! Wraps `git2::Repository` to produce small snapshot values the UI can render
//! without holding a live libgit2 handle. The snapshot is a one-shot read at
//! open time; live updates come in a later slice.

use std::{
    fmt, io,
    path::{Path, PathBuf},
    str,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub commit_sha: String,
    pub base_sha: Option<String>,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub content: FileDiffContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDiffContent {
    Single { side: DiffSide, text: String },
    SideBySide { old_text: String, new_text: String },
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    Old,
    New,
}

#[derive(Debug)]
pub enum OpenError {
    NotARepository,
    Io(io::Error),
    Git(git2::Error),
}

#[derive(Debug)]
pub enum ChangeSetError {
    Open(OpenError),
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

impl fmt::Display for ChangeSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeSetError::Open(err) => write!(f, "{err}"),
            ChangeSetError::Git(err) => write!(f, "Couldn't read that changeset: {err}."),
        }
    }
}

impl std::error::Error for ChangeSetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ChangeSetError::Open(err) => Some(err),
            ChangeSetError::Git(err) => Some(err),
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

pub fn changeset_for_single_commit(path: &Path, sha: &str) -> Result<ChangeSet, ChangeSetError> {
    let canonical = path
        .canonicalize()
        .map_err(|err| ChangeSetError::Open(OpenError::Io(err)))?;
    let repo = git2::Repository::open(&canonical)
        .map_err(classify_open_error)
        .map_err(ChangeSetError::Open)?;
    let oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
    let commit = repo.find_commit(oid).map_err(ChangeSetError::Git)?;
    let commit_tree = commit.tree().map_err(ChangeSetError::Git)?;
    let base_commit = if commit.parent_count() > 0 {
        Some(commit.parent(0).map_err(ChangeSetError::Git)?)
    } else {
        None
    };
    let base_sha = base_commit.as_ref().map(|commit| commit.id().to_string());
    let base_tree = base_commit
        .as_ref()
        .map(|commit| commit.tree())
        .transpose()
        .map_err(ChangeSetError::Git)?;

    let mut diff_options = git2::DiffOptions::new();
    let mut diff = repo
        .diff_tree_to_tree(
            base_tree.as_ref(),
            Some(&commit_tree),
            Some(&mut diff_options),
        )
        .map_err(ChangeSetError::Git)?;

    let mut find_options = git2::DiffFindOptions::new();
    find_options.renames(true);
    diff.find_similar(Some(&mut find_options))
        .map_err(ChangeSetError::Git)?;

    let mut files = diff
        .deltas()
        .filter_map(changed_file_from_delta)
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.old_path.cmp(&right.old_path))
    });

    Ok(ChangeSet {
        commit_sha: commit.id().to_string(),
        base_sha,
        files,
    })
}

pub fn file_diff_for_changed_file(
    path: &Path,
    sha: &str,
    file: &ChangedFile,
) -> Result<FileDiff, ChangeSetError> {
    let canonical = path
        .canonicalize()
        .map_err(|err| ChangeSetError::Open(OpenError::Io(err)))?;
    let repo = git2::Repository::open(&canonical)
        .map_err(classify_open_error)
        .map_err(ChangeSetError::Open)?;
    let oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
    let commit = repo.find_commit(oid).map_err(ChangeSetError::Git)?;
    let commit_tree = commit.tree().map_err(ChangeSetError::Git)?;
    let base_tree = first_parent_tree(&commit)?;

    let content = match file.kind {
        ChangeKind::Added => {
            let text = read_text_blob(&repo, &commit_tree, &file.path)?;
            single_file_content(DiffSide::New, text)
        }
        ChangeKind::Deleted => {
            let base_tree = required_base_tree(base_tree.as_ref())?;
            let old_path = file.old_path.as_deref().unwrap_or(&file.path);
            let text = read_text_blob(&repo, base_tree, old_path)?;
            single_file_content(DiffSide::Old, text)
        }
        ChangeKind::Modified => {
            let base_tree = required_base_tree(base_tree.as_ref())?;
            let old_text = read_text_blob(&repo, base_tree, &file.path)?;
            let new_text = read_text_blob(&repo, &commit_tree, &file.path)?;
            side_by_side_file_content(old_text, new_text)
        }
        ChangeKind::Renamed => {
            let base_tree = required_base_tree(base_tree.as_ref())?;
            let old_path = file.old_path.as_deref().ok_or_else(|| {
                ChangeSetError::Git(git2::Error::from_str("Missing rename source path"))
            })?;
            let old_text = read_text_blob(&repo, base_tree, old_path)?;
            let new_text = read_text_blob(&repo, &commit_tree, &file.path)?;
            side_by_side_file_content(old_text, new_text)
        }
    };

    Ok(FileDiff {
        path: file.path.clone(),
        old_path: file.old_path.clone(),
        kind: file.kind,
        content,
    })
}

fn classify_open_error(err: git2::Error) -> OpenError {
    use git2::ErrorCode;
    match err.code() {
        ErrorCode::NotFound => OpenError::NotARepository,
        _ => OpenError::Git(err),
    }
}

fn first_parent_tree<'repo>(
    commit: &git2::Commit<'repo>,
) -> Result<Option<git2::Tree<'repo>>, ChangeSetError> {
    if commit.parent_count() == 0 {
        return Ok(None);
    }

    commit
        .parent(0)
        .and_then(|parent| parent.tree())
        .map(Some)
        .map_err(ChangeSetError::Git)
}

fn required_base_tree<'a>(
    base_tree: Option<&'a git2::Tree<'a>>,
) -> Result<&'a git2::Tree<'a>, ChangeSetError> {
    base_tree.ok_or_else(|| ChangeSetError::Git(git2::Error::from_str("Missing base commit tree")))
}

fn read_text_blob(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    path: &str,
) -> Result<Option<String>, ChangeSetError> {
    let entry = tree
        .get_path(Path::new(path))
        .map_err(ChangeSetError::Git)?;
    let object = entry.to_object(repo).map_err(ChangeSetError::Git)?;
    let blob = object.as_blob().ok_or_else(|| {
        ChangeSetError::Git(git2::Error::from_str("Tree entry is not a file blob"))
    })?;
    let bytes = blob.content();

    if bytes.contains(&0) {
        return Ok(None);
    }

    str::from_utf8(bytes)
        .map(|text| Some(text.to_string()))
        .or(Ok(None))
}

fn single_file_content(side: DiffSide, text: Option<String>) -> FileDiffContent {
    match text {
        Some(text) => FileDiffContent::Single { side, text },
        None => FileDiffContent::Binary,
    }
}

fn side_by_side_file_content(
    old_text: Option<String>,
    new_text: Option<String>,
) -> FileDiffContent {
    match (old_text, new_text) {
        (Some(old_text), Some(new_text)) => FileDiffContent::SideBySide { old_text, new_text },
        _ => FileDiffContent::Binary,
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

fn changed_file_from_delta(delta: git2::DiffDelta<'_>) -> Option<ChangedFile> {
    match delta.status() {
        git2::Delta::Added | git2::Delta::Copied => Some(ChangedFile {
            path: diff_path(delta.new_file())?,
            old_path: None,
            kind: ChangeKind::Added,
        }),
        git2::Delta::Deleted => Some(ChangedFile {
            path: diff_path(delta.old_file())?,
            old_path: None,
            kind: ChangeKind::Deleted,
        }),
        git2::Delta::Modified | git2::Delta::Typechange => Some(ChangedFile {
            path: diff_path(delta.new_file())?,
            old_path: None,
            kind: ChangeKind::Modified,
        }),
        git2::Delta::Renamed => Some(ChangedFile {
            path: diff_path(delta.new_file())?,
            old_path: Some(diff_path(delta.old_file())?),
            kind: ChangeKind::Renamed,
        }),
        git2::Delta::Unmodified
        | git2::Delta::Ignored
        | git2::Delta::Untracked
        | git2::Delta::Unreadable
        | git2::Delta::Conflicted => None,
    }
}

fn diff_path(file: git2::DiffFile<'_>) -> Option<String> {
    file.path().map(|path| path.to_string_lossy().into_owned())
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
    use std::{fs, path::Path};
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

    fn commit_workdir(repo: &Repository, message: &str, parent_oids: &[git2::Oid]) -> String {
        let mut index = repo.index().expect("open index");
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .expect("stage files");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let signature =
            Signature::now("Greviewer Tests", "tests@greviewer.invalid").expect("create signature");
        let parents = parent_oids
            .iter()
            .map(|oid| repo.find_commit(*oid).expect("find parent"))
            .collect::<Vec<_>>();
        let parent_refs = parents.iter().collect::<Vec<_>>();

        let oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parent_refs,
            )
            .expect("create commit");

        oid.to_string()
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
    fn changeset_for_root_commit_lists_added_files() {
        let (dir, oid_hex) = init_repo_with_one_commit();

        let changeset = changeset_for_single_commit(dir.path(), &oid_hex).expect("changeset");

        assert_eq!(changeset.commit_sha, oid_hex);
        assert_eq!(changeset.base_sha, None);
        assert_eq!(
            changeset.files,
            vec![ChangedFile {
                path: "hello.txt".to_string(),
                old_path: None,
                kind: ChangeKind::Added,
            }],
        );
    }

    #[test]
    fn file_diff_for_added_file_returns_new_text() {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let file = ChangedFile {
            path: "hello.txt".to_string(),
            old_path: None,
            kind: ChangeKind::Added,
        };

        let diff = file_diff_for_changed_file(dir.path(), &oid_hex, &file).expect("file diff");

        assert_eq!(diff.path, "hello.txt");
        assert_eq!(diff.old_path, None);
        assert_eq!(diff.kind, ChangeKind::Added);
        assert_eq!(
            diff.content,
            FileDiffContent::Single {
                side: DiffSide::New,
                text: "hello\n".to_string(),
            },
        );
    }

    #[test]
    fn changeset_for_commit_lists_deleted_files() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("obsolete.txt"), "obsolete\n").expect("write file");
        let root_oid = git2::Oid::from_str(&commit_workdir(&repo, "Add obsolete.txt", &[]))
            .expect("parse root oid");

        fs::remove_file(dir.path().join("obsolete.txt")).expect("delete file");
        let mut index = repo.index().expect("open index");
        index
            .remove_path(Path::new("obsolete.txt"))
            .expect("stage deletion");
        index.write().expect("write index");
        drop(index);

        let delete_oid = commit_workdir(&repo, "Delete obsolete.txt", &[root_oid]);

        let changeset = changeset_for_single_commit(dir.path(), &delete_oid).expect("changeset");

        assert_eq!(
            changeset.files,
            vec![ChangedFile {
                path: "obsolete.txt".to_string(),
                old_path: None,
                kind: ChangeKind::Deleted,
            }],
        );
    }

    #[test]
    fn file_diff_for_deleted_file_returns_old_text() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("obsolete.txt"), "obsolete\n").expect("write file");
        let root_oid = git2::Oid::from_str(&commit_workdir(&repo, "Add obsolete.txt", &[]))
            .expect("parse root oid");

        fs::remove_file(dir.path().join("obsolete.txt")).expect("delete file");
        let mut index = repo.index().expect("open index");
        index
            .remove_path(Path::new("obsolete.txt"))
            .expect("stage deletion");
        index.write().expect("write index");
        drop(index);

        let delete_oid = commit_workdir(&repo, "Delete obsolete.txt", &[root_oid]);
        let file = ChangedFile {
            path: "obsolete.txt".to_string(),
            old_path: None,
            kind: ChangeKind::Deleted,
        };

        let diff = file_diff_for_changed_file(dir.path(), &delete_oid, &file).expect("file diff");

        assert_eq!(
            diff.content,
            FileDiffContent::Single {
                side: DiffSide::Old,
                text: "obsolete\n".to_string(),
            },
        );
    }

    #[test]
    fn file_diff_for_modified_file_returns_old_and_new_text() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("hello.txt"), "before\n").expect("write file");
        let root_oid = git2::Oid::from_str(&commit_workdir(&repo, "Add hello.txt", &[]))
            .expect("parse root oid");

        fs::write(dir.path().join("hello.txt"), "after\n").expect("update file");
        let update_oid = commit_workdir(&repo, "Update hello.txt", &[root_oid]);
        let file = ChangedFile {
            path: "hello.txt".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
        };

        let diff = file_diff_for_changed_file(dir.path(), &update_oid, &file).expect("file diff");

        assert_eq!(
            diff.content,
            FileDiffContent::SideBySide {
                old_text: "before\n".to_string(),
                new_text: "after\n".to_string(),
            },
        );
    }

    #[test]
    fn changeset_for_commit_lists_renamed_files() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("old.txt"), "same content\n").expect("write old file");
        let root_oid = git2::Oid::from_str(&commit_workdir(&repo, "Add old.txt", &[]))
            .expect("parse root oid");

        fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).expect("rename file");
        let mut index = repo.index().expect("open index");
        index
            .remove_path(Path::new("old.txt"))
            .expect("stage old path removal");
        index
            .add_path(Path::new("new.txt"))
            .expect("stage new path");
        index.write().expect("write index");
        drop(index);

        let rename_oid = commit_workdir(&repo, "Rename old.txt", &[root_oid]);

        let changeset = changeset_for_single_commit(dir.path(), &rename_oid).expect("changeset");

        assert_eq!(
            changeset.files,
            vec![ChangedFile {
                path: "new.txt".to_string(),
                old_path: Some("old.txt".to_string()),
                kind: ChangeKind::Renamed,
            }],
        );
    }

    #[test]
    fn file_diff_for_renamed_file_uses_old_and_new_paths() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("old.txt"), "before rename\n").expect("write old file");
        let root_oid = git2::Oid::from_str(&commit_workdir(&repo, "Add old.txt", &[]))
            .expect("parse root oid");

        fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).expect("rename file");
        fs::write(dir.path().join("new.txt"), "after rename\n").expect("update new file");
        let mut index = repo.index().expect("open index");
        index
            .remove_path(Path::new("old.txt"))
            .expect("stage old path removal");
        index
            .add_path(Path::new("new.txt"))
            .expect("stage new path");
        index.write().expect("write index");
        drop(index);

        let rename_oid = commit_workdir(&repo, "Rename old.txt", &[root_oid]);
        let file = ChangedFile {
            path: "new.txt".to_string(),
            old_path: Some("old.txt".to_string()),
            kind: ChangeKind::Renamed,
        };

        let diff = file_diff_for_changed_file(dir.path(), &rename_oid, &file).expect("file diff");

        assert_eq!(diff.path, "new.txt");
        assert_eq!(diff.old_path.as_deref(), Some("old.txt"));
        assert_eq!(
            diff.content,
            FileDiffContent::SideBySide {
                old_text: "before rename\n".to_string(),
                new_text: "after rename\n".to_string(),
            },
        );
    }

    #[test]
    fn file_diff_for_non_utf8_file_returns_binary_placeholder() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("binary.dat"), b"\xff\xfe\0data").expect("write binary file");
        let oid_hex = commit_workdir(&repo, "Add binary.dat", &[]);
        let file = ChangedFile {
            path: "binary.dat".to_string(),
            old_path: None,
            kind: ChangeKind::Added,
        };

        let diff = file_diff_for_changed_file(dir.path(), &oid_hex, &file).expect("file diff");

        assert_eq!(diff.content, FileDiffContent::Binary);
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
