//! Parse a Git `origin` URL into the Bitbucket Data Center coordinates
//! (`base_url`, `project`, `slug`) used to build REST paths. Data Center
//! serves repos under `/scm/PROJECT/slug.git` over HTTPS and
//! `PROJECT/slug.git` on the SSH port. A URL that does not match either shape
//! yields `None`, and the whole PR feature stays invisible.

/// Bitbucket Data Center repository coordinates derived from `origin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitBucketRepo {
    /// Scheme + host, no trailing slash, e.g. `https://bitbucket.cicd.dc`.
    pub base_url: String,
    /// Project key, e.g. `PROJ`.
    pub project: String,
    /// Repository slug, e.g. `repo`.
    pub slug: String,
}

/// Parse a Git `origin` URL into [`BitBucketRepo`], or `None` when the URL is
/// not a recognizable Bitbucket Data Center repository path.
pub fn parse_origin(url: &str) -> Option<BitBucketRepo> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // Two families: an explicit scheme (`https://`, `http://`, `ssh://`) which
    // has a `//host` authority, and scp-style `git@host:path` which does not.
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        // HTTPS/HTTP Data Center repos live under `/scm/PROJECT/slug`. The
        // `scm/` prefix is what distinguishes a Bitbucket repo path from any
        // other host's `org/repo`, so require it here.
        let (authority, path) = rest.split_once('/')?;
        let host = authority_host(authority)?;
        let path = path.trim_start_matches('/');
        let path = path.strip_prefix("scm/")?;
        let (project, slug) = split_project_slug(path)?;
        return Some(BitBucketRepo {
            base_url: format!("https://{host}"),
            project,
            slug,
        });
    }

    if let Some(rest) = url.strip_prefix("ssh://") {
        // rest = "[user@]host[:port]/path/to/repo[.git]". SSH URLs do not use
        // the `/scm/` prefix.
        let (authority, path) = rest.split_once('/')?;
        let host = authority_host(authority)?;
        let (project, slug) = split_project_slug(path)?;
        return Some(BitBucketRepo {
            base_url: format!("https://{host}"),
            project,
            slug,
        });
    }

    // scp-style: "[user@]host:path/to/repo[.git]" (no scheme, no `//`).
    let (authority, path) = url.split_once(':')?;
    let host = authority_host(authority)?;
    // A leading port like `7999/PROJ/...` is part of `path` here; strip it.
    let path = strip_leading_port(path);
    let (project, slug) = split_project_slug(path)?;
    Some(BitBucketRepo {
        base_url: format!("https://{host}"),
        project,
        slug,
    })
}

/// Extract the bare host from `[user@]host[:port]`.
fn authority_host(authority: &str) -> Option<String> {
    let after_user = authority.rsplit('@').next()?;
    let host = after_user.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Drop a leading `PORT/` segment from an scp-style path (`7999/PROJ/repo`).
fn strip_leading_port(path: &str) -> &str {
    match path.split_once('/') {
        Some((first, rest)) if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) => {
            rest
        }
        _ => path,
    }
}

/// From a repo path, pull the `PROJECT` and `slug`, tolerating a trailing
/// `.git`. Requires exactly a project and slug.
fn split_project_slug(path: &str) -> Option<(String, String)> {
    let path = path.trim_matches('/');
    let mut parts = path.split('/').filter(|s| !s.is_empty());
    let project = parts.next()?.to_string();
    let slug = parts.next()?;
    if parts.next().is_some() {
        // More than project/slug -> not a repo root path we understand.
        return None;
    }
    let slug = slug.strip_suffix(".git").unwrap_or(slug).to_string();
    if project.is_empty() || slug.is_empty() {
        return None;
    }
    Some((project, slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(base: &str, project: &str, slug: &str) -> Option<BitBucketRepo> {
        Some(BitBucketRepo {
            base_url: base.to_string(),
            project: project.to_string(),
            slug: slug.to_string(),
        })
    }

    #[test]
    fn parses_https_scm_url() {
        assert_eq!(
            parse_origin("https://bitbucket.cicd.dc/scm/PROJ/repo.git"),
            repo("https://bitbucket.cicd.dc", "PROJ", "repo")
        );
    }

    #[test]
    fn parses_https_without_dot_git() {
        assert_eq!(
            parse_origin("https://bitbucket.cicd.dc/scm/PROJ/repo"),
            repo("https://bitbucket.cicd.dc", "PROJ", "repo")
        );
    }

    #[test]
    fn parses_scheme_ssh_url_with_port() {
        assert_eq!(
            parse_origin("ssh://git@bitbucket.cicd.dc:7999/PROJ/repo.git"),
            repo("https://bitbucket.cicd.dc", "PROJ", "repo")
        );
    }

    #[test]
    fn parses_scp_style_ssh_url_with_port() {
        assert_eq!(
            parse_origin("git@bitbucket.cicd.dc:7999/PROJ/repo.git"),
            repo("https://bitbucket.cicd.dc", "PROJ", "repo")
        );
    }

    #[test]
    fn preserves_project_key_case() {
        assert_eq!(
            parse_origin("https://bitbucket.cicd.dc/scm/ProJ/Repo.git")
                .map(|r| (r.project, r.slug)),
            Some(("ProJ".to_string(), "Repo".to_string()))
        );
    }

    #[test]
    fn rejects_non_bitbucket_https_url() {
        assert_eq!(parse_origin("https://github.com/org/repo.git"), None);
    }

    #[test]
    fn rejects_missing_slug() {
        assert_eq!(parse_origin("https://bitbucket.cicd.dc/scm/PROJ"), None);
        assert_eq!(parse_origin("git@bitbucket.cicd.dc:7999/PROJ"), None);
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(parse_origin(""), None);
        assert_eq!(parse_origin("not a url"), None);
    }
}
