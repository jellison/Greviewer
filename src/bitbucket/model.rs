//! Domain value and wire DTOs for Bitbucket Data Center pull requests.
//!
//! The DTOs mirror the `/rest/api/1.0` JSON shapes; [`PullRequest`] is the lean
//! domain value the rest of the app consumes. `web_url` exists on the wire and is
//! read tolerantly, but is deliberately NOT promoted onto the domain value in V1
//! (nothing renders it yet); it graduates to the domain type when a post-V1
//! feature consumes it.

use serde::Deserialize;

/// A Bitbucket pull request, reduced to what V1 surfaces need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    /// The `#number` shown in every surface.
    pub id: u64,
    /// `title` — the PR's human-facing title.
    pub title: String,
    /// `fromRef.displayId` — the source branch short name.
    pub source_branch: String,
    /// `toRef.displayId` — the target branch short name.
    pub target_branch: String,
    /// `fromRef.latestCommit` — the join key to the local graph.
    pub source_tip_sha: String,
}

/// One page of the paginated pull-request listing.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestPage {
    #[serde(default)]
    pub values: Vec<PullRequestDto>,
    #[serde(default)]
    pub is_last_page: bool,
    #[serde(default)]
    pub next_page_start: Option<u64>,
}

/// The Data Center pull-request object. Unknown fields are ignored so additive
/// API changes do not break decoding.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestDto {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub from_ref: Option<RefDto>,
    #[serde(default)]
    pub to_ref: Option<RefDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RefDto {
    #[serde(default)]
    pub display_id: String,
    #[serde(default)]
    pub latest_commit: String,
}

impl PullRequestDto {
    /// Map the wire object to the domain value. Returns `None` when the source
    /// ref (the graph anchor) is missing its commit sha — such a PR cannot be
    /// anchored and is dropped from the snapshot.
    pub(crate) fn into_domain(self) -> Option<PullRequest> {
        let from = self.from_ref?;
        if from.latest_commit.is_empty() {
            return None;
        }
        let to = self.to_ref.unwrap_or_default();
        Some(PullRequest {
            id: self.id,
            title: self.title,
            source_branch: from.display_id,
            target_branch: to.display_id,
            source_tip_sha: from.latest_commit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE_JSON: &str = r#"{
        "size": 2,
        "isLastPage": false,
        "nextPageStart": 25,
        "values": [
            {
                "id": 42,
                "title": "Add widget",
                "fromRef": { "displayId": "feature/widget", "latestCommit": "aaaa1111" },
                "toRef": { "displayId": "main", "latestCommit": "bbbb2222" },
                "links": { "self": [ { "href": "https://bitbucket.cicd.dc/pr/42" } ] }
            },
            {
                "id": 7,
                "title": "Fix typo",
                "fromRef": { "displayId": "bugfix/typo", "latestCommit": "cccc3333" },
                "toRef": { "displayId": "main", "latestCommit": "bbbb2222" }
            }
        ]
    }"#;

    #[test]
    fn decodes_a_page_and_pagination_fields() {
        let page: PullRequestPage = serde_json::from_str(PAGE_JSON).expect("decode");
        assert!(!page.is_last_page);
        assert_eq!(page.next_page_start, Some(25));
        assert_eq!(page.values.len(), 2);
    }

    #[test]
    fn maps_dto_to_domain() {
        let page: PullRequestPage = serde_json::from_str(PAGE_JSON).expect("decode");
        let prs: Vec<PullRequest> = page
            .values
            .into_iter()
            .filter_map(PullRequestDto::into_domain)
            .collect();
        assert_eq!(
            prs[0],
            PullRequest {
                id: 42,
                title: "Add widget".to_string(),
                source_branch: "feature/widget".to_string(),
                target_branch: "main".to_string(),
                source_tip_sha: "aaaa1111".to_string(),
            }
        );
        assert_eq!(prs[1].id, 7);
        assert_eq!(prs[1].source_tip_sha, "cccc3333");
    }

    #[test]
    fn drops_pr_without_source_commit() {
        let dto: PullRequestDto = serde_json::from_str(
            r#"{ "id": 1, "fromRef": { "displayId": "x", "latestCommit": "" }, "toRef": null }"#,
        )
        .expect("decode");
        assert_eq!(dto.into_domain(), None);
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let page: PullRequestPage = serde_json::from_str(r#"{ "values": [] }"#).expect("decode");
        assert!(!page.is_last_page);
        assert_eq!(page.next_page_start, None);
        assert!(page.values.is_empty());
    }

    #[test]
    fn ignores_unknown_fields() {
        let dto: PullRequestDto = serde_json::from_str(
            r#"{ "id": 9, "unexpected": {"a":1}, "fromRef": { "latestCommit": "dd" } }"#,
        )
        .expect("decode");
        let pr = dto.into_domain().expect("maps");
        assert_eq!(pr.id, 9);
        assert_eq!(pr.source_branch, "");
        assert_eq!(pr.source_tip_sha, "dd");
    }
}
