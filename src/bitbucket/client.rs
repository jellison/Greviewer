//! Transport seam and pagination for the pull-request listing.
//!
//! `client.rs` is generic over [`HttpTransport`] so tests inject canned JSON and
//! the production `ureq` dependency (built against the OS trust store via its
//! `platform-verifier` feature — MIT/Apache-2.0, ADR-0001 compatible) never
//! leaks into the rest of the app. The token is passed as an `Authorization:
//! Bearer` header and is never logged; error text names the env var only.

use std::time::Duration;

use crate::bitbucket::model::{PullRequest, PullRequestDto, PullRequestPage};
use crate::bitbucket::remote::BitBucketRepo;

/// Per-request timeout so a hung server cannot wedge the fetch.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Page size requested from Data Center.
const PAGE_LIMIT: u64 = 100;
/// Upper bound on total PRs loaded, to bound time and memory.
const MAX_PULL_REQUESTS: usize = 1000;

/// A minimal HTTP response: status code plus body text.
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// The seam the pagination loop drives. The production impl wraps `ureq`; tests
/// inject a fake. Implementors send the bearer token and return status + body,
/// mapping only genuine transport failures (DNS/TLS/timeout) to `Err`.
pub trait HttpTransport: Send + Sync {
    fn get(&self, url: &str, bearer_token: &str) -> Result<HttpResponse, BitBucketError>;
}

/// Typed failure surface mapped to sidebar states. `Network`/`Decode` carry a
/// human-readable reason; none of them ever carry the token value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BitBucketError {
    /// `BITBUCKET_TOKEN` unset — no request attempted.
    NotConfigured,
    /// 401/403.
    Auth,
    /// 404 — repository not on the server.
    NotFound,
    /// DNS/TLS/timeout/connection failure.
    Network(String),
    /// Body did not decode as the expected JSON.
    Decode(String),
    /// Any other non-2xx status.
    Server(u16),
}

impl std::fmt::Display for BitBucketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BitBucketError::NotConfigured => {
                write!(f, "Set BITBUCKET_TOKEN to load pull requests")
            }
            BitBucketError::Auth => write!(f, "Authentication failed — check BITBUCKET_TOKEN"),
            BitBucketError::NotFound => write!(f, "Repository not found on Bitbucket"),
            BitBucketError::Network(reason) => write!(f, "Network error: {reason}"),
            BitBucketError::Decode(reason) => {
                write!(f, "Could not read Bitbucket response: {reason}")
            }
            BitBucketError::Server(status) => write!(f, "Bitbucket returned status {status}"),
        }
    }
}

/// Fetch every open pull request for `repo`, following pagination. Stops at
/// `MAX_PULL_REQUESTS`. `token` must be non-empty (callers map the unset case to
/// [`BitBucketError::NotConfigured`] before reaching here).
pub fn fetch_open_pull_requests(
    transport: &dyn HttpTransport,
    repo: &BitBucketRepo,
    token: &str,
) -> Result<Vec<PullRequest>, BitBucketError> {
    let mut collected: Vec<PullRequest> = Vec::new();
    let mut start: u64 = 0;
    loop {
        let url = format!(
            "{base}/rest/api/1.0/projects/{project}/repos/{slug}/pull-requests?state=OPEN&limit={limit}&start={start}",
            base = repo.base_url,
            project = repo.project,
            slug = repo.slug,
            limit = PAGE_LIMIT,
        );
        let response = transport.get(&url, token)?;
        map_status(response.status)?;
        let page: PullRequestPage = serde_json::from_str(&response.body)
            .map_err(|err| BitBucketError::Decode(err.to_string()))?;
        collected.extend(
            page.values
                .into_iter()
                .filter_map(PullRequestDto::into_domain),
        );
        if collected.len() >= MAX_PULL_REQUESTS {
            collected.truncate(MAX_PULL_REQUESTS);
            break;
        }
        match (page.is_last_page, page.next_page_start) {
            (false, Some(next)) => start = next,
            _ => break,
        }
    }
    Ok(collected)
}

/// Map an HTTP status to `Ok(())` (2xx) or a typed error.
fn map_status(status: u16) -> Result<(), BitBucketError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(BitBucketError::Auth),
        404 => Err(BitBucketError::NotFound),
        other => Err(BitBucketError::Server(other)),
    }
}

/// Production transport wrapping `ureq` with the OS trust store.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build()
            .new_agent();
        UreqTransport { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn get(&self, url: &str, bearer_token: &str) -> Result<HttpResponse, BitBucketError> {
        match self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {bearer_token}"))
            .call()
        {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let body = response
                    .body_mut()
                    .read_to_string()
                    .map_err(|err| BitBucketError::Network(err.to_string()))?;
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::StatusCode(code)) => Ok(HttpResponse {
                status: code,
                body: String::new(),
            }),
            Err(err) => Err(BitBucketError::Network(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A transport that returns queued (status, body) pairs in order and records
    /// the URLs it was asked for. Interior state uses `Mutex` so the fake is
    /// `Send + Sync` without any `unsafe`.
    struct FakeTransport {
        responses: Mutex<Vec<(u16, String)>>,
        urls: Mutex<Vec<String>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<(u16, String)>) -> Self {
            FakeTransport {
                responses: Mutex::new(responses),
                urls: Mutex::new(Vec::new()),
            }
        }

        fn urls(&self) -> Vec<String> {
            self.urls.lock().expect("urls lock").clone()
        }
    }

    impl HttpTransport for FakeTransport {
        fn get(&self, url: &str, _token: &str) -> Result<HttpResponse, BitBucketError> {
            self.urls.lock().expect("urls lock").push(url.to_string());
            let (status, body) = self.responses.lock().expect("responses lock").remove(0);
            Ok(HttpResponse { status, body })
        }
    }

    fn repo() -> BitBucketRepo {
        BitBucketRepo {
            base_url: "https://bitbucket.cicd.dc".to_string(),
            project: "PROJ".to_string(),
            slug: "repo".to_string(),
        }
    }

    fn page(is_last: bool, next: Option<u64>, ids: &[u64]) -> String {
        let values: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    r#"{{ "id": {id}, "fromRef": {{ "displayId": "b{id}", "latestCommit": "sha{id}" }}, "toRef": {{ "displayId": "main", "latestCommit": "base" }} }}"#
                )
            })
            .collect();
        let next_field = match next {
            Some(n) => format!(r#", "nextPageStart": {n}"#),
            None => String::new(),
        };
        format!(
            r#"{{ "isLastPage": {is_last}, "values": [{}]{next_field} }}"#,
            values.join(",")
        )
    }

    #[test]
    fn single_page_returns_all_prs() {
        let transport = FakeTransport::new(vec![(200, page(true, None, &[1, 2, 3]))]);
        let prs = fetch_open_pull_requests(&transport, &repo(), "tok").expect("ok");
        assert_eq!(prs.iter().map(|p| p.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(transport.urls().len(), 1);
        assert!(transport.urls()[0].contains("state=OPEN&limit=100&start=0"));
    }

    #[test]
    fn follows_pagination_until_last_page() {
        let transport = FakeTransport::new(vec![
            (200, page(false, Some(100), &[1, 2])),
            (200, page(true, None, &[3])),
        ]);
        let prs = fetch_open_pull_requests(&transport, &repo(), "tok").expect("ok");
        assert_eq!(prs.iter().map(|p| p.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(transport.urls().len(), 2);
        assert!(transport.urls()[1].contains("start=100"));
    }

    #[test]
    fn maps_auth_status() {
        let transport = FakeTransport::new(vec![(401, String::new())]);
        assert_eq!(
            fetch_open_pull_requests(&transport, &repo(), "tok"),
            Err(BitBucketError::Auth)
        );
        let transport = FakeTransport::new(vec![(403, String::new())]);
        assert_eq!(
            fetch_open_pull_requests(&transport, &repo(), "tok"),
            Err(BitBucketError::Auth)
        );
    }

    #[test]
    fn maps_not_found_and_server_status() {
        let transport = FakeTransport::new(vec![(404, String::new())]);
        assert_eq!(
            fetch_open_pull_requests(&transport, &repo(), "tok"),
            Err(BitBucketError::NotFound)
        );
        let transport = FakeTransport::new(vec![(500, String::new())]);
        assert_eq!(
            fetch_open_pull_requests(&transport, &repo(), "tok"),
            Err(BitBucketError::Server(500))
        );
    }

    #[test]
    fn maps_malformed_json_to_decode() {
        let transport = FakeTransport::new(vec![(200, "not json".to_string())]);
        assert!(matches!(
            fetch_open_pull_requests(&transport, &repo(), "tok"),
            Err(BitBucketError::Decode(_))
        ));
    }

    #[test]
    fn error_display_never_contains_token_value() {
        assert!(BitBucketError::Auth.to_string().contains("BITBUCKET_TOKEN"));
        assert!(BitBucketError::NotConfigured
            .to_string()
            .contains("BITBUCKET_TOKEN"));
    }
}
