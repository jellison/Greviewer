//! Repository data layer.
//!
//! Wraps `git2::Repository` to produce small snapshot values the UI can render
//! without holding a live libgit2 handle. The snapshot is a one-shot read at
//! open time; live updates come in a later slice.

use std::{
    collections::BTreeMap,
    fmt, fs, io,
    path::{Path, PathBuf},
    str,
};

pub const INITIAL_COMMIT_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct OpenRepository {
    pub path: PathBuf,
    pub head: Option<HeadInfo>,
    pub commits: Vec<CommitInfo>,
    pub has_more_commits: bool,
    pub branches: Vec<Branch>,
}

/// How a branch ref is scoped: a local branch under `refs/heads/`, or a
/// remote-tracking branch under `refs/remotes/` belonging to a configured
/// remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchKind {
    Local,
    Remote { remote: String },
}

/// A branch snapshot taken at open time. `name` is the qualified display
/// name (`main`, or `origin/main` for a remote-tracking branch). `is_head`
/// is true only for the checked-out local branch; a detached HEAD marks no
/// branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub tip_sha: String,
    pub is_head: bool,
    pub kind: BranchKind,
}

/// A branch name attached to a commit row, with the kind the renderer needs
/// to style it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchLabel {
    pub name: String,
    pub kind: BranchKind,
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
    pub parent_shas: Vec<String>,
    pub branch_labels: Vec<BranchLabel>,
    pub parent_count: usize,
    pub is_head: bool,
}

#[derive(Debug, Clone)]
pub struct CommitPage {
    pub commits: Vec<CommitInfo>,
    pub has_more: bool,
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
    pub is_binary: bool,
    pub line_stats: LineStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineStats {
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryFile {
    pub path: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContent {
    pub path: String,
    pub content: FileContentBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContentBody {
    Text(String),
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
    let checked_out_branch_oid = read_checked_out_branch_oid(&repo)?;
    let branches = read_branches(&repo)?;
    let branch_labels_by_oid = branch_labels_by_oid(&branches)?;
    let page = read_commit_page(
        &repo,
        head_oid,
        checked_out_branch_oid,
        &branch_labels_by_oid,
        None,
        INITIAL_COMMIT_LIMIT,
    )?;

    Ok(OpenRepository {
        path: canonical,
        head,
        commits: page.commits,
        has_more_commits: page.has_more,
        branches,
    })
}

/// The git repositories sitting alongside `current` in its parent folder,
/// including `current` itself. Scans the parent's direct children (non-recursive)
/// and keeps directories whose `.git` entry exists, which covers normal clones
/// and worktrees. Returned paths are canonicalized and sorted case-insensitively
/// by folder name. A `current` with no parent, or an unreadable parent, yields an
/// empty list.
pub fn sibling_repositories(current: &Path) -> Vec<PathBuf> {
    let Some(parent) = current.parent() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };

    let mut repositories: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(".git").exists())
        .filter_map(|path| path.canonicalize().ok())
        .collect();

    repositories.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    repositories
}

pub fn load_commits_after(path: &Path, after_sha: &str) -> Result<CommitPage, OpenError> {
    let canonical = path.canonicalize().map_err(OpenError::Io)?;
    let repo = git2::Repository::open(&canonical).map_err(classify_open_error)?;
    let head_commit = read_head_commit(&repo)?;
    let head_oid = head_commit.as_ref().map(|commit| commit.id());
    let checked_out_branch_oid = read_checked_out_branch_oid(&repo)?;
    let branches = read_branches(&repo)?;
    let branch_labels_by_oid = branch_labels_by_oid(&branches)?;
    let after_oid = git2::Oid::from_str(after_sha).map_err(OpenError::Git)?;

    read_commit_page(
        &repo,
        head_oid,
        checked_out_branch_oid,
        &branch_labels_by_oid,
        Some(after_oid),
        INITIAL_COMMIT_LIMIT,
    )
}

pub fn changeset_for_single_commit(path: &Path, sha: &str) -> Result<ChangeSet, ChangeSetError> {
    let repo = open_changeset_repository(path)?;
    let oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
    let commit = repo.find_commit(oid).map_err(ChangeSetError::Git)?;
    let commit_tree = commit.tree().map_err(ChangeSetError::Git)?;
    let base_commit = first_parent_commit(&commit)?;
    let base_sha = base_commit.as_ref().map(|commit| commit.id().to_string());
    let base_tree = base_commit
        .as_ref()
        .map(|commit| commit.tree())
        .transpose()
        .map_err(ChangeSetError::Git)?;

    changeset_between_trees(
        &repo,
        commit.id().to_string(),
        base_sha,
        base_tree.as_ref(),
        &commit_tree,
    )
}

pub fn changeset_for_commit_range(
    path: &Path,
    oldest_sha: &str,
    newest_sha: &str,
) -> Result<ChangeSet, ChangeSetError> {
    let repo = open_changeset_repository(path)?;
    let oldest_oid = git2::Oid::from_str(oldest_sha).map_err(ChangeSetError::Git)?;
    let newest_oid = git2::Oid::from_str(newest_sha).map_err(ChangeSetError::Git)?;

    if oldest_oid != newest_oid
        && !repo
            .graph_descendant_of(newest_oid, oldest_oid)
            .map_err(ChangeSetError::Git)?
    {
        return Err(ChangeSetError::Git(git2::Error::from_str(
            "Range newest commit does not descend from oldest commit",
        )));
    }

    let oldest_commit = repo.find_commit(oldest_oid).map_err(ChangeSetError::Git)?;
    let newest_commit = repo.find_commit(newest_oid).map_err(ChangeSetError::Git)?;
    let newest_tree = newest_commit.tree().map_err(ChangeSetError::Git)?;
    let base_commit = first_parent_commit(&oldest_commit)?;
    let base_sha = base_commit.as_ref().map(|commit| commit.id().to_string());
    let base_tree = base_commit
        .as_ref()
        .map(|commit| commit.tree())
        .transpose()
        .map_err(ChangeSetError::Git)?;

    changeset_between_trees(
        &repo,
        newest_commit.id().to_string(),
        base_sha,
        base_tree.as_ref(),
        &newest_tree,
    )
}

fn changeset_between_trees(
    repo: &git2::Repository,
    commit_sha: String,
    base_sha: Option<String>,
    base_tree: Option<&git2::Tree<'_>>,
    target_tree: &git2::Tree<'_>,
) -> Result<ChangeSet, ChangeSetError> {
    let files = changed_files_between_trees(repo, base_tree, target_tree)?;

    Ok(ChangeSet {
        commit_sha,
        base_sha,
        files,
    })
}

fn changed_files_between_trees(
    repo: &git2::Repository,
    base_tree: Option<&git2::Tree<'_>>,
    target_tree: &git2::Tree<'_>,
) -> Result<Vec<ChangedFile>, ChangeSetError> {
    let mut diff_options = git2::DiffOptions::new();
    let mut diff = repo
        .diff_tree_to_tree(base_tree, Some(target_tree), Some(&mut diff_options))
        .map_err(ChangeSetError::Git)?;

    let mut find_options = git2::DiffFindOptions::new();
    find_options.renames(true);
    diff.find_similar(Some(&mut find_options))
        .map_err(ChangeSetError::Git)?;

    let mut files = Vec::new();
    for (delta_index, delta) in diff.deltas().enumerate() {
        let Some(mut file) = changed_file_from_delta(delta) else {
            continue;
        };
        file.is_binary = changed_file_is_binary(repo, base_tree, target_tree, &file)?;
        file.line_stats = line_stats_for_delta(&diff, delta_index)?;
        files.push(file);
    }
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.old_path.cmp(&right.old_path))
    });

    Ok(files)
}

pub fn commits_share_linear_ancestry(
    path: &Path,
    first_sha: &str,
    second_sha: &str,
) -> Result<bool, ChangeSetError> {
    let canonical = path
        .canonicalize()
        .map_err(|err| ChangeSetError::Open(OpenError::Io(err)))?;
    let repo = git2::Repository::open(&canonical)
        .map_err(classify_open_error)
        .map_err(ChangeSetError::Open)?;
    let first = git2::Oid::from_str(first_sha).map_err(ChangeSetError::Git)?;
    let second = git2::Oid::from_str(second_sha).map_err(ChangeSetError::Git)?;

    if first == second {
        return Ok(true);
    }

    let first_descends_from_second = repo
        .graph_descendant_of(first, second)
        .map_err(ChangeSetError::Git)?;
    let second_descends_from_first = repo
        .graph_descendant_of(second, first)
        .map_err(ChangeSetError::Git)?;

    Ok(first_descends_from_second || second_descends_from_first)
}

pub fn file_diff_for_changed_file(
    path: &Path,
    sha: &str,
    file: &ChangedFile,
) -> Result<FileDiff, ChangeSetError> {
    let repo = open_changeset_repository(path)?;
    let target_oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
    let target_commit = repo.find_commit(target_oid).map_err(ChangeSetError::Git)?;
    let base_commit = first_parent_commit(&target_commit)?;
    let base_sha = base_commit.as_ref().map(|commit| commit.id().to_string());

    file_diff_for_changed_file_in_repo(&repo, sha, base_sha.as_deref(), file)
}

pub fn file_diff_for_changed_file_between(
    path: &Path,
    target_sha: &str,
    base_sha: Option<&str>,
    file: &ChangedFile,
) -> Result<FileDiff, ChangeSetError> {
    let repo = open_changeset_repository(path)?;

    file_diff_for_changed_file_in_repo(&repo, target_sha, base_sha, file)
}

pub fn files_at_commit(path: &Path, sha: &str) -> Result<Vec<RepositoryFile>, ChangeSetError> {
    let repo = open_changeset_repository(path)?;
    let oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
    let commit = repo.find_commit(oid).map_err(ChangeSetError::Git)?;
    let tree = commit.tree().map_err(ChangeSetError::Git)?;
    let mut files = Vec::new();

    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            let name = String::from_utf8_lossy(entry.name_bytes());
            files.push(RepositoryFile {
                path: format!("{root}{name}"),
            });
        }

        git2::TreeWalkResult::Ok
    })
    .map_err(ChangeSetError::Git)?;

    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

pub fn file_content_at_commit(
    path: &Path,
    sha: &str,
    file_path: &str,
) -> Result<FileContent, ChangeSetError> {
    let repo = open_changeset_repository(path)?;
    let oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
    let commit = repo.find_commit(oid).map_err(ChangeSetError::Git)?;
    let tree = commit.tree().map_err(ChangeSetError::Git)?;
    let content = match read_text_blob(&repo, &tree, file_path)? {
        Some(text) => FileContentBody::Text(text),
        None => FileContentBody::Binary,
    };

    Ok(FileContent {
        path: file_path.to_string(),
        content,
    })
}

fn file_diff_for_changed_file_in_repo(
    repo: &git2::Repository,
    target_sha: &str,
    base_sha: Option<&str>,
    file: &ChangedFile,
) -> Result<FileDiff, ChangeSetError> {
    let target_oid = git2::Oid::from_str(target_sha).map_err(ChangeSetError::Git)?;
    let target_commit = repo.find_commit(target_oid).map_err(ChangeSetError::Git)?;
    let target_tree = target_commit.tree().map_err(ChangeSetError::Git)?;
    let base_commit = base_sha
        .map(|sha| {
            let oid = git2::Oid::from_str(sha).map_err(ChangeSetError::Git)?;
            repo.find_commit(oid).map_err(ChangeSetError::Git)
        })
        .transpose()?;
    let base_tree = base_commit
        .as_ref()
        .map(|commit| commit.tree())
        .transpose()
        .map_err(ChangeSetError::Git)?;

    let content = match file.kind {
        ChangeKind::Added => {
            let text = read_text_blob(repo, &target_tree, &file.path)?;
            single_file_content(DiffSide::New, text)
        }
        ChangeKind::Deleted => {
            let base_tree = required_base_tree(base_tree.as_ref())?;
            let old_path = file.old_path.as_deref().unwrap_or(&file.path);
            let text = read_text_blob(repo, base_tree, old_path)?;
            single_file_content(DiffSide::Old, text)
        }
        ChangeKind::Modified => {
            let base_tree = required_base_tree(base_tree.as_ref())?;
            let old_text = read_text_blob(repo, base_tree, &file.path)?;
            let new_text = read_text_blob(repo, &target_tree, &file.path)?;
            side_by_side_file_content(old_text, new_text)
        }
        ChangeKind::Renamed => {
            let base_tree = required_base_tree(base_tree.as_ref())?;
            let old_path = file.old_path.as_deref().ok_or_else(|| {
                ChangeSetError::Git(git2::Error::from_str("Missing rename source path"))
            })?;
            let old_text = read_text_blob(repo, base_tree, old_path)?;
            let new_text = read_text_blob(repo, &target_tree, &file.path)?;
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

fn open_changeset_repository(path: &Path) -> Result<git2::Repository, ChangeSetError> {
    let canonical = path
        .canonicalize()
        .map_err(|err| ChangeSetError::Open(OpenError::Io(err)))?;

    git2::Repository::open(&canonical)
        .map_err(classify_open_error)
        .map_err(ChangeSetError::Open)
}

fn classify_open_error(err: git2::Error) -> OpenError {
    use git2::ErrorCode;
    match err.code() {
        ErrorCode::NotFound => OpenError::NotARepository,
        _ => OpenError::Git(err),
    }
}

fn first_parent_commit<'repo>(
    commit: &git2::Commit<'repo>,
) -> Result<Option<git2::Commit<'repo>>, ChangeSetError> {
    if commit.parent_count() == 0 {
        return Ok(None);
    }

    commit.parent(0).map(Some).map_err(ChangeSetError::Git)
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

fn blob_has_text(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    path: &str,
) -> Result<bool, ChangeSetError> {
    read_text_blob(repo, tree, path).map(|text| text.is_some())
}

fn changed_file_is_binary(
    repo: &git2::Repository,
    base_tree: Option<&git2::Tree<'_>>,
    target_tree: &git2::Tree<'_>,
    file: &ChangedFile,
) -> Result<bool, ChangeSetError> {
    match file.kind {
        ChangeKind::Added => blob_has_text(repo, target_tree, &file.path).map(|has_text| !has_text),
        ChangeKind::Deleted => {
            let base_tree = required_base_tree(base_tree)?;
            let old_path = file.old_path.as_deref().unwrap_or(&file.path);
            blob_has_text(repo, base_tree, old_path).map(|has_text| !has_text)
        }
        ChangeKind::Modified => {
            let base_tree = required_base_tree(base_tree)?;
            let old_has_text = blob_has_text(repo, base_tree, &file.path)?;
            let new_has_text = blob_has_text(repo, target_tree, &file.path)?;
            Ok(!old_has_text || !new_has_text)
        }
        ChangeKind::Renamed => {
            let base_tree = required_base_tree(base_tree)?;
            let old_path = file.old_path.as_deref().ok_or_else(|| {
                ChangeSetError::Git(git2::Error::from_str("Missing rename source path"))
            })?;
            let old_has_text = blob_has_text(repo, base_tree, old_path)?;
            let new_has_text = blob_has_text(repo, target_tree, &file.path)?;
            Ok(!old_has_text || !new_has_text)
        }
    }
}

fn line_stats_for_delta(
    diff: &git2::Diff<'_>,
    delta_index: usize,
) -> Result<LineStats, ChangeSetError> {
    let Some(patch) = git2::Patch::from_diff(diff, delta_index).map_err(ChangeSetError::Git)?
    else {
        return Ok(LineStats::default());
    };
    let (_context, added, removed) = patch.line_stats().map_err(ChangeSetError::Git)?;

    Ok(LineStats { added, removed })
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

fn read_checked_out_branch_oid(repo: &git2::Repository) -> Result<Option<git2::Oid>, OpenError> {
    match repo.head() {
        Ok(reference) if reference.is_branch() => reference
            .peel_to_commit()
            .map(|commit| Some(commit.id()))
            .map_err(OpenError::Git),
        Ok(_) => Ok(None),
        Err(err) if err.code() == git2::ErrorCode::UnbornBranch => Ok(None),
        Err(err) if err.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(err) => Err(OpenError::Git(err)),
    }
}

fn read_commit_page(
    repo: &git2::Repository,
    head_oid: Option<git2::Oid>,
    checked_out_branch_oid: Option<git2::Oid>,
    branch_labels_by_oid: &BTreeMap<git2::Oid, Vec<BranchLabel>>,
    after_oid: Option<git2::Oid>,
    limit: usize,
) -> Result<CommitPage, OpenError> {
    if head_oid.is_none() && branch_labels_by_oid.is_empty() {
        return Ok(CommitPage {
            commits: Vec::new(),
            has_more: false,
        });
    }

    let mut revwalk = repo.revwalk().map_err(OpenError::Git)?;
    revwalk
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(OpenError::Git)?;
    for branch_oid in branch_labels_by_oid.keys() {
        revwalk.push(*branch_oid).map_err(OpenError::Git)?;
    }
    if head_oid.is_some() {
        revwalk.push_head().map_err(OpenError::Git)?;
    }

    let mut commits = Vec::new();
    let mut after_seen = after_oid.is_none();
    for oid in revwalk {
        let oid = oid.map_err(OpenError::Git)?;
        if !after_seen {
            after_seen = Some(oid) == after_oid;
            continue;
        }

        if commits.len() == limit {
            return Ok(CommitPage {
                commits,
                has_more: true,
            });
        }

        let commit = repo.find_commit(oid).map_err(OpenError::Git)?;
        commits.push(commit_info_from_commit(
            &commit,
            checked_out_branch_oid,
            branch_labels_by_oid
                .get(&commit.id())
                .cloned()
                .unwrap_or_default(),
        ));
    }

    if !after_seen {
        return Err(OpenError::Git(git2::Error::from_str(
            "Commit cursor was not found in commit history",
        )));
    }

    Ok(CommitPage {
        commits,
        has_more: false,
    })
}

/// Groups branch labels by tip oid. The branch list's local-first/name sort
/// carries into each commit's label order.
fn branch_labels_by_oid(
    branches: &[Branch],
) -> Result<BTreeMap<git2::Oid, Vec<BranchLabel>>, OpenError> {
    let mut labels_by_oid = BTreeMap::<git2::Oid, Vec<BranchLabel>>::new();
    for branch in branches {
        let oid = git2::Oid::from_str(&branch.tip_sha).map_err(OpenError::Git)?;
        labels_by_oid.entry(oid).or_default().push(BranchLabel {
            name: branch.name.clone(),
            kind: branch.kind.clone(),
        });
    }
    Ok(labels_by_oid)
}

/// Reads both local and remote-tracking branches. Local branches sort before
/// remote-tracking branches; within each group branches are sorted by name.
/// Refs with no direct target and the remote default-branch pointer
/// (`{remote}/HEAD`, whether symbolic or direct) are excluded.
fn read_branches(repo: &git2::Repository) -> Result<Vec<Branch>, OpenError> {
    let remote_names: Vec<String> = repo
        .remotes()
        .map_err(OpenError::Git)?
        .iter()
        .flatten()
        .map(str::to_string)
        .collect();

    let mut result = Vec::new();
    for entry in repo.branches(None).map_err(OpenError::Git)? {
        let (branch, branch_type) = entry.map_err(OpenError::Git)?;
        // Symbolic refs (a freshly cloned origin/HEAD) have no direct target.
        let Some(target_oid) = branch.get().target() else {
            continue;
        };
        let Some(name) = branch.name().map_err(OpenError::Git)? else {
            continue;
        };

        let kind = match branch_type {
            git2::BranchType::Local => BranchKind::Local,
            git2::BranchType::Remote => {
                let remote = remote_name_for(name, &remote_names);
                if name
                    .strip_prefix(remote.as_str())
                    .is_some_and(|rest| rest == "/HEAD")
                {
                    continue;
                }
                BranchKind::Remote { remote }
            }
        };

        result.push(Branch {
            name: name.to_string(),
            tip_sha: target_oid.to_string(),
            is_head: branch_type == git2::BranchType::Local && branch.is_head(),
            kind,
        });
    }

    result.sort_by(|left, right| {
        branch_kind_rank(&left.kind)
            .cmp(&branch_kind_rank(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(result)
}

/// Local branches order before remote-tracking branches everywhere a mixed
/// list is shown.
fn branch_kind_rank(kind: &BranchKind) -> u8 {
    match kind {
        BranchKind::Local => 0,
        BranchKind::Remote { .. } => 1,
    }
}

/// The configured remote a remote-tracking branch belongs to: the longest
/// remote name that prefixes the branch's qualified name. Falls back to the
/// first path segment when no configured remote matches (refs left behind
/// after a remote was removed).
fn remote_name_for(branch_name: &str, remote_names: &[String]) -> String {
    remote_names
        .iter()
        .filter(|remote| {
            branch_name
                .strip_prefix(remote.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|remote| remote.len())
        .cloned()
        .unwrap_or_else(|| {
            branch_name
                .split('/')
                .next()
                .unwrap_or(branch_name)
                .to_string()
        })
}

fn head_info_from_commit(commit: &git2::Commit<'_>) -> HeadInfo {
    HeadInfo {
        short_sha: short_sha(commit.id()),
        summary: commit.summary().unwrap_or("").trim_end().to_string(),
    }
}

fn commit_info_from_commit(
    commit: &git2::Commit<'_>,
    head_oid: Option<git2::Oid>,
    branch_labels: Vec<BranchLabel>,
) -> CommitInfo {
    let sha = commit.id().to_string();
    let author = commit.author();
    let author = match author.name().map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "Unknown author".to_string(),
    };
    let parent_shas = commit
        .parent_ids()
        .map(|parent_oid| parent_oid.to_string())
        .collect();

    CommitInfo {
        short_sha: short_sha(commit.id()),
        sha,
        summary: commit.summary().unwrap_or("").trim_end().to_string(),
        author,
        authored_timestamp: commit.time().seconds(),
        authored_date: format_authored_date(commit.time()),
        parent_shas,
        branch_labels,
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
            is_binary: false,
            line_stats: LineStats::default(),
        }),
        git2::Delta::Deleted => Some(ChangedFile {
            path: diff_path(delta.old_file())?,
            old_path: None,
            kind: ChangeKind::Deleted,
            is_binary: false,
            line_stats: LineStats::default(),
        }),
        git2::Delta::Modified | git2::Delta::Typechange => Some(ChangedFile {
            path: diff_path(delta.new_file())?,
            old_path: None,
            kind: ChangeKind::Modified,
            is_binary: false,
            line_stats: LineStats::default(),
        }),
        git2::Delta::Renamed => Some(ChangedFile {
            path: diff_path(delta.new_file())?,
            old_path: Some(diff_path(delta.old_file())?),
            kind: ChangeKind::Renamed,
            is_binary: false,
            line_stats: LineStats::default(),
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

    fn init_repo_with_linear_history(count: usize) -> (TempDir, Vec<String>) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");
        let mut newest_first = Vec::with_capacity(count);
        let mut parents = Vec::new();

        for index in 0..count {
            fs::write(dir.path().join("history.txt"), format!("commit {index}\n"))
                .expect("write history file");
            let oid =
                git2::Oid::from_str(&commit_workdir(&repo, &format!("Commit {index}"), &parents))
                    .expect("parse commit oid");
            newest_first.insert(0, oid.to_string());
            parents = vec![oid];
        }

        drop(repo);
        (dir, newest_first)
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

    /// Commits the current index tree to `reference` (or to no ref when
    /// `None`) with an explicit authored time, so tests control history
    /// interleaving deterministically.
    fn commit_tree_to_ref(
        repo: &Repository,
        reference: Option<&str>,
        message: &str,
        authored_seconds: i64,
        parent_oids: &[git2::Oid],
    ) -> git2::Oid {
        let mut index = repo.index().expect("open index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let signature = Signature::new(
            "Greviewer Tests",
            "tests@greviewer.invalid",
            &git2::Time::new(authored_seconds, 0),
        )
        .expect("create signature");
        let parents = parent_oids
            .iter()
            .map(|oid| repo.find_commit(*oid).expect("find parent"))
            .collect::<Vec<_>>();
        let parent_refs = parents.iter().collect::<Vec<_>>();

        repo.commit(
            reference,
            &signature,
            &signature,
            message,
            &tree,
            &parent_refs,
        )
        .expect("create commit")
    }

    #[test]
    fn open_at_includes_commits_from_unmerged_local_branches() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        let main_tip = commit_tree_to_ref(&repo, Some("HEAD"), "Main tip", 200, &[root]);
        let feature_tip = commit_tree_to_ref(
            &repo,
            Some("refs/heads/feature"),
            "Feature tip",
            300,
            &[root],
        );
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        let shas = snapshot
            .commits
            .iter()
            .map(|commit| commit.sha.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            shas,
            vec![
                feature_tip.to_string(),
                main_tip.to_string(),
                root.to_string(),
            ],
            "unmerged branch commits interleave newest-first",
        );
        assert!(snapshot.commits[1].is_head, "HEAD stays on the main tip");
    }

    #[test]
    fn open_at_returns_local_branches_sorted_with_head_marked() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        let main_tip = commit_tree_to_ref(&repo, Some("HEAD"), "Main tip", 200, &[root]);
        let feature_tip = commit_tree_to_ref(
            &repo,
            Some("refs/heads/feature"),
            "Feature tip",
            300,
            &[root],
        );
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        let summary = snapshot
            .branches
            .iter()
            .map(|branch| {
                (
                    branch.name.as_str(),
                    branch.tip_sha.as_str(),
                    branch.is_head,
                )
            })
            .collect::<Vec<_>>();
        let feature_sha = feature_tip.to_string();
        let main_sha = main_tip.to_string();
        assert_eq!(
            summary,
            vec![
                ("feature", feature_sha.as_str(), false),
                ("master", main_sha.as_str(), true),
            ],
            "local branches sort by name and mark the checked-out branch",
        );
    }

    #[test]
    fn open_at_marks_no_branch_as_head_when_detached() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        let tip = commit_tree_to_ref(&repo, Some("HEAD"), "Tip", 200, &[root]);
        repo.set_head_detached(tip).expect("detach HEAD");
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(
            snapshot.branches.iter().all(|branch| !branch.is_head),
            "detached HEAD marks no branch",
        );
    }

    #[test]
    fn open_at_returns_empty_local_branches_for_an_unborn_repo() {
        let dir = init_unborn_repo();

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(snapshot.branches.is_empty());
    }

    #[test]
    fn open_at_includes_remote_only_commits_but_not_tag_commits() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        let remote_only = commit_tree_to_ref(&repo, None, "Remote only", 200, &[root]);
        let tag_only = commit_tree_to_ref(&repo, None, "Tag only", 300, &[root]);
        repo.remote("origin", "https://example.invalid/repo.git")
            .expect("configure remote");
        repo.reference(
            "refs/remotes/origin/topic",
            remote_only,
            true,
            "create remote-tracking ref",
        )
        .expect("create remote-tracking ref");
        repo.reference("refs/tags/v1", tag_only, true, "create tag")
            .expect("create tag");
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        let shas = snapshot
            .commits
            .iter()
            .map(|commit| commit.sha.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            shas,
            vec![remote_only.to_string(), root.to_string()],
            "remote-tracking commits join the graph; tag-only commits stay out",
        );
    }

    #[test]
    fn commit_labels_carry_branch_kind_with_locals_first() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let tip = commit_tree_to_ref(&repo, Some("HEAD"), "Tip", 100, &[]);
        repo.remote("origin", "https://example.invalid/repo.git")
            .expect("configure remote");
        repo.reference("refs/remotes/origin/master", tip, true, "remote master")
            .expect("create remote master");
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert_eq!(
            snapshot.commits[0].branch_labels,
            vec![
                BranchLabel {
                    name: "master".to_string(),
                    kind: BranchKind::Local,
                },
                BranchLabel {
                    name: "origin/master".to_string(),
                    kind: BranchKind::Remote {
                        remote: "origin".to_string()
                    },
                },
            ],
            "a commit at both tips labels the local branch before the remote one",
        );
    }

    #[test]
    fn open_at_includes_orphan_branch_commits() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        let orphan = commit_tree_to_ref(&repo, Some("refs/heads/orphan"), "Orphan", 200, &[]);
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        let shas = snapshot
            .commits
            .iter()
            .map(|commit| commit.sha.clone())
            .collect::<Vec<_>>();
        assert_eq!(shas, vec![orphan.to_string(), root.to_string()]);
    }

    #[test]
    fn open_at_with_unborn_head_still_lists_branch_commits() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let other = commit_tree_to_ref(&repo, Some("refs/heads/other"), "Other", 100, &[]);
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(snapshot.head.is_none(), "HEAD is still unborn");
        let shas = snapshot
            .commits
            .iter()
            .map(|commit| commit.sha.clone())
            .collect::<Vec<_>>();
        assert_eq!(shas, vec![other.to_string()]);
    }

    #[test]
    fn load_commits_after_pages_deterministically_across_multiple_tips() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 10, &[]);
        let main_a = commit_tree_to_ref(&repo, Some("HEAD"), "Main a", 20, &[root]);
        let feature_a =
            commit_tree_to_ref(&repo, Some("refs/heads/feature"), "Feature a", 25, &[root]);
        let main_b = commit_tree_to_ref(&repo, Some("HEAD"), "Main b", 30, &[main_a]);
        let _feature_b = commit_tree_to_ref(
            &repo,
            Some("refs/heads/feature"),
            "Feature b",
            35,
            &[feature_a],
        );
        let _main_c = commit_tree_to_ref(&repo, Some("HEAD"), "Main c", 40, &[main_b]);
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");
        assert_eq!(snapshot.commits.len(), 6, "all branch commits load");

        for (index, commit) in snapshot.commits.iter().enumerate() {
            let page = load_commits_after(dir.path(), &commit.sha).expect("load page");
            let page_shas = page
                .commits
                .iter()
                .map(|commit| commit.sha.clone())
                .collect::<Vec<_>>();
            let expected = snapshot.commits[index + 1..]
                .iter()
                .map(|commit| commit.sha.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                page_shas, expected,
                "resuming after any cursor continues the same walk",
            );
        }
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
    fn open_at_marks_more_commits_when_initial_page_is_limited() {
        let (dir, shas) = init_repo_with_linear_history(INITIAL_COMMIT_LIMIT + 2);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert_eq!(snapshot.commits.len(), INITIAL_COMMIT_LIMIT);
        assert!(snapshot.has_more_commits);
        assert_eq!(snapshot.commits[0].sha, shas[0]);
        assert_eq!(
            snapshot.commits.last().expect("last visible commit").sha,
            shas[INITIAL_COMMIT_LIMIT - 1]
        );
    }

    #[test]
    fn load_commits_after_returns_the_next_history_page() {
        let (dir, shas) = init_repo_with_linear_history(INITIAL_COMMIT_LIMIT + 2);
        let snapshot = open_at(dir.path()).expect("open succeeds");
        let last_visible_sha = snapshot
            .commits
            .last()
            .expect("last visible commit")
            .sha
            .clone();

        let page = load_commits_after(dir.path(), &last_visible_sha).expect("load next page");

        assert_eq!(page.commits.len(), 2);
        assert!(!page.has_more);
        assert_eq!(page.commits[0].sha, shas[INITIAL_COMMIT_LIMIT]);
        assert_eq!(page.commits[1].sha, shas[INITIAL_COMMIT_LIMIT + 1]);
    }

    #[test]
    fn open_at_returns_commit_parent_shas() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("history.txt"), "root\n").expect("write root file");
        let root_oid =
            git2::Oid::from_str(&commit_workdir(&repo, "Root", &[])).expect("parse root oid");

        fs::write(dir.path().join("history.txt"), "tip\n").expect("write tip file");
        let tip_oid =
            git2::Oid::from_str(&commit_workdir(&repo, "Tip", &[root_oid])).expect("parse tip oid");

        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert_eq!(snapshot.commits[0].sha, tip_oid.to_string());
        assert_eq!(snapshot.commits[0].parent_shas, vec![root_oid.to_string()]);
        assert_eq!(snapshot.commits[1].sha, root_oid.to_string());
        assert!(snapshot.commits[1].parent_shas.is_empty());
    }

    #[test]
    fn open_at_returns_local_branch_names_for_commits() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("history.txt"), "root\n").expect("write root file");
        let root_oid =
            git2::Oid::from_str(&commit_workdir(&repo, "Root", &[])).expect("parse root oid");

        fs::write(dir.path().join("history.txt"), "tip\n").expect("write tip file");
        let tip_oid =
            git2::Oid::from_str(&commit_workdir(&repo, "Tip", &[root_oid])).expect("parse tip oid");

        repo.reference(
            "refs/heads/feature",
            root_oid,
            true,
            "create feature branch",
        )
        .expect("create feature branch");
        repo.reference(
            "refs/heads/review/topic",
            tip_oid,
            true,
            "create topic branch",
        )
        .expect("create topic branch");

        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert_eq!(snapshot.commits[0].sha, tip_oid.to_string());
        assert_eq!(
            snapshot.commits[0].branch_labels,
            vec![
                BranchLabel {
                    name: "master".to_string(),
                    kind: BranchKind::Local,
                },
                BranchLabel {
                    name: "review/topic".to_string(),
                    kind: BranchKind::Local,
                },
            ]
        );
        assert_eq!(snapshot.commits[1].sha, root_oid.to_string());
        assert_eq!(
            snapshot.commits[1].branch_labels,
            vec![BranchLabel {
                name: "feature".to_string(),
                kind: BranchKind::Local,
            }]
        );
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
                is_binary: false,
                line_stats: LineStats {
                    added: 1,
                    removed: 0,
                },
            }],
        );
    }

    #[test]
    fn changeset_for_commit_range_rolls_up_oldest_parent_to_newest() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("rollup.txt"), "before\n").expect("write file");
        let root_oid = git2::Oid::from_str(&commit_workdir(&repo, "Add rollup.txt", &[]))
            .expect("parse root oid");

        fs::write(dir.path().join("rollup.txt"), "middle\n").expect("update file");
        let middle_oid = git2::Oid::from_str(&commit_workdir(&repo, "Middle", &[root_oid]))
            .expect("parse middle oid");

        fs::write(dir.path().join("rollup.txt"), "after\n").expect("update file again");
        let tip_oid = git2::Oid::from_str(&commit_workdir(&repo, "Tip", &[middle_oid]))
            .expect("parse tip oid");

        let root_sha = root_oid.to_string();
        let middle_sha = middle_oid.to_string();
        let tip_sha = tip_oid.to_string();

        let changeset =
            changeset_for_commit_range(dir.path(), &middle_sha, &tip_sha).expect("range changeset");

        assert_eq!(changeset.commit_sha, tip_sha);
        assert_eq!(changeset.base_sha, Some(root_sha.clone()));
        assert_eq!(
            changeset.files,
            vec![ChangedFile {
                path: "rollup.txt".to_string(),
                old_path: None,
                kind: ChangeKind::Modified,
                is_binary: false,
                line_stats: LineStats {
                    added: 1,
                    removed: 1,
                },
            }],
        );

        let diff = file_diff_for_changed_file_between(
            dir.path(),
            &changeset.commit_sha,
            Some(&root_sha),
            &changeset.files[0],
        )
        .expect("range file diff");

        assert_eq!(
            diff.content,
            FileDiffContent::SideBySide {
                old_text: "before\n".to_string(),
                new_text: "after\n".to_string(),
            },
        );
    }

    #[test]
    fn files_at_commit_lists_repository_tree_at_that_commit() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("changed.txt"), "before\n").expect("write changed file");
        fs::create_dir_all(dir.path().join("docs")).expect("create docs dir");
        fs::write(dir.path().join("docs/context.txt"), "context\n").expect("write context file");
        let root_oid =
            git2::Oid::from_str(&commit_workdir(&repo, "Initial", &[])).expect("parse root oid");

        fs::write(dir.path().join("changed.txt"), "after\n").expect("update changed file");
        let tip_sha = commit_workdir(&repo, "Update changed file", &[root_oid]);

        let files = files_at_commit(dir.path(), &tip_sha).expect("files at commit");

        assert_eq!(
            files,
            vec![
                RepositoryFile {
                    path: "changed.txt".to_string(),
                },
                RepositoryFile {
                    path: "docs/context.txt".to_string(),
                },
            ],
        );
    }

    #[test]
    fn file_content_at_commit_reads_text_from_that_commit() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("context.txt"), "before\n").expect("write context file");
        let root_oid =
            git2::Oid::from_str(&commit_workdir(&repo, "Initial", &[])).expect("parse root oid");

        fs::write(dir.path().join("context.txt"), "after\n").expect("update context file");
        let tip_sha = commit_workdir(&repo, "Update context file", &[root_oid]);

        let content =
            file_content_at_commit(dir.path(), &tip_sha, "context.txt").expect("file content");

        assert_eq!(content.path, "context.txt");
        assert_eq!(
            content.content,
            FileContentBody::Text("after\n".to_string()),
        );
    }

    #[test]
    fn file_diff_for_added_file_returns_new_text() {
        let (dir, oid_hex) = init_repo_with_one_commit();
        let file = ChangedFile {
            path: "hello.txt".to_string(),
            old_path: None,
            kind: ChangeKind::Added,
            is_binary: false,
            line_stats: LineStats::default(),
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
                is_binary: false,
                line_stats: LineStats {
                    added: 0,
                    removed: 1,
                },
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
            is_binary: false,
            line_stats: LineStats::default(),
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
            is_binary: false,
            line_stats: LineStats::default(),
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
                is_binary: false,
                line_stats: LineStats {
                    added: 0,
                    removed: 0,
                },
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
            is_binary: false,
            line_stats: LineStats::default(),
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
            is_binary: true,
            line_stats: LineStats::default(),
        };

        let diff = file_diff_for_changed_file(dir.path(), &oid_hex, &file).expect("file diff");

        assert_eq!(diff.content, FileDiffContent::Binary);
    }

    #[test]
    fn changeset_for_binary_file_marks_text_diff_unavailable() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        fs::write(dir.path().join("binary.dat"), b"\xff\xfe\0data").expect("write binary file");
        let oid_hex = commit_workdir(&repo, "Add binary.dat", &[]);

        let changeset = changeset_for_single_commit(dir.path(), &oid_hex).expect("changeset");

        assert_eq!(
            changeset.files,
            vec![ChangedFile {
                path: "binary.dat".to_string(),
                old_path: None,
                kind: ChangeKind::Added,
                is_binary: true,
                line_stats: LineStats {
                    added: 0,
                    removed: 0,
                },
            }],
        );
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

    #[test]
    fn sibling_repositories_lists_sibling_git_repos_sorted_including_current() {
        let parent = tempfile::tempdir().expect("create parent tempdir");

        for name in ["Gamma", "alpha", "beta"] {
            Repository::init(parent.path().join(name)).expect("init sibling repo");
        }
        fs::create_dir(parent.path().join("plain")).expect("create plain dir");

        let current = parent.path().join("beta");
        let siblings = sibling_repositories(&current);

        let expect = |name: &str| {
            parent
                .path()
                .join(name)
                .canonicalize()
                .expect("canonicalize expected sibling")
        };
        // Sorted case-insensitively by folder name; the current repo is included
        // and the non-repo "plain" directory is excluded.
        assert_eq!(
            siblings,
            vec![expect("alpha"), expect("beta"), expect("Gamma")]
        );
    }

    #[test]
    fn sibling_repositories_is_empty_for_a_path_without_a_parent() {
        assert!(sibling_repositories(Path::new("/")).is_empty());
    }

    #[test]
    fn open_at_returns_remote_branches_with_their_remote() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        let remote_tip = commit_tree_to_ref(&repo, None, "Remote tip", 200, &[root]);
        repo.remote("origin", "https://example.invalid/repo.git")
            .expect("configure remote");
        repo.reference(
            "refs/remotes/origin/topic",
            remote_tip,
            true,
            "create remote-tracking ref",
        )
        .expect("create remote-tracking ref");
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        let summary = snapshot
            .branches
            .iter()
            .map(|branch| (branch.name.as_str(), branch.kind.clone(), branch.is_head))
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                ("master", BranchKind::Local, true),
                (
                    "origin/topic",
                    BranchKind::Remote {
                        remote: "origin".to_string()
                    },
                    false,
                ),
            ],
            "remote branches carry their remote and sort after local branches",
        );
    }

    #[test]
    fn open_at_excludes_the_remote_head_ref() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let repo = Repository::init(dir.path()).expect("init repo");

        let root = commit_tree_to_ref(&repo, Some("HEAD"), "Root", 100, &[]);
        // origin: symbolic HEAD (excluded by the no-direct-target guard —
        // `branch.get().target()` returns None for symbolic refs).
        repo.remote("origin", "https://example.invalid/repo.git")
            .expect("configure remote");
        repo.reference("refs/remotes/origin/main", root, true, "remote main")
            .expect("create remote main");
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "symbolic remote HEAD",
        )
        .expect("create symbolic remote HEAD");
        // upstream: direct HEAD ref (excluded by the `rest == "/HEAD"` check
        // in read_branches, which runs after the symbolic guard).
        repo.remote("upstream", "https://example.invalid/upstream.git")
            .expect("configure upstream remote");
        repo.reference(
            "refs/remotes/upstream/main",
            root,
            true,
            "upstream remote main",
        )
        .expect("create upstream remote main");
        repo.reference(
            "refs/remotes/upstream/HEAD",
            root,
            true,
            "direct remote HEAD",
        )
        .expect("create direct upstream HEAD");
        drop(repo);

        let snapshot = open_at(dir.path()).expect("open succeeds");

        assert!(
            snapshot
                .branches
                .iter()
                .all(|branch| branch.name != "origin/HEAD"),
            "origin/HEAD (symbolic) never appears as a branch",
        );
        assert!(
            snapshot
                .branches
                .iter()
                .any(|branch| branch.name == "origin/main"),
            "origin/main still appears",
        );
        assert!(
            snapshot
                .branches
                .iter()
                .all(|branch| branch.name != "upstream/HEAD"),
            "upstream/HEAD (direct) never appears as a branch",
        );
        assert!(
            snapshot
                .branches
                .iter()
                .any(|branch| branch.name == "upstream/main"),
            "upstream/main still appears",
        );
    }

    #[test]
    fn remote_name_matching_prefers_the_longest_configured_remote() {
        let remotes = vec!["origin".to_string(), "origin/nested".to_string()];
        assert_eq!(remote_name_for("origin/main", &remotes), "origin");
        assert_eq!(
            remote_name_for("origin/nested/main", &remotes),
            "origin/nested",
            "a remote name containing '/' wins over its shorter prefix",
        );
        assert_eq!(
            remote_name_for("ghost/main", &remotes),
            "ghost",
            "an unmatched ref falls back to its first path segment",
        );
    }
}
