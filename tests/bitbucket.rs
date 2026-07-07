//! Integration coverage for the Bitbucket PR module: origin parsing plus a
//! full fetch through a fake transport, asserting the snapshot and the
//! source-tip grouping. No live network.

pub mod common;

use greviewer::bitbucket::{
    fetch_open_pull_requests, parse_origin, BitBucketError, HttpResponse, HttpTransport,
};
use std::sync::Mutex;

struct FakeTransport {
    pages: Mutex<Vec<(u16, String)>>,
}

impl HttpTransport for FakeTransport {
    fn get(&self, _url: &str, _token: &str) -> Result<HttpResponse, BitBucketError> {
        let (status, body) = self.pages.lock().expect("lock").remove(0);
        Ok(HttpResponse { status, body })
    }
}

#[test]
fn parses_origin_then_fetches_and_groups_by_source_tip() {
    let repo = parse_origin("https://bitbucket.cicd.dc/scm/PROJ/repo.git").expect("bitbucket repo");
    assert_eq!(repo.project, "PROJ");
    assert_eq!(repo.slug, "repo");

    let body = r#"{
        "isLastPage": true,
        "values": [
            {"id": 3, "fromRef": {"displayId":"a","latestCommit":"tip1"}, "toRef": {"displayId":"main","latestCommit":"base"}},
            {"id": 8, "fromRef": {"displayId":"b","latestCommit":"tip1"}, "toRef": {"displayId":"main","latestCommit":"base"}},
            {"id": 5, "fromRef": {"displayId":"c","latestCommit":"tip2"}, "toRef": {"displayId":"main","latestCommit":"base"}}
        ]
    }"#;
    let transport = FakeTransport {
        pages: Mutex::new(vec![(200, body.to_string())]),
    };

    let prs = fetch_open_pull_requests(&transport, &repo, "token").expect("fetch ok");
    assert_eq!(prs.len(), 3);

    use std::collections::HashMap;
    let mut index: HashMap<&str, Vec<u64>> = HashMap::new();
    for pr in &prs {
        index
            .entry(pr.source_tip_sha.as_str())
            .or_default()
            .push(pr.id);
    }
    for ids in index.values_mut() {
        ids.sort();
    }
    assert_eq!(index["tip1"], vec![3, 8]);
    assert_eq!(index["tip2"], vec![5]);
}

#[test]
fn build_repo_with_origin_round_trips_through_origin_url_and_parse() {
    let (dir, shas) = common::build_repo_with_origin(
        &[common::CommitSpec {
            message: "init".to_string(),
            changes: vec![],
        }],
        "https://bitbucket.cicd.dc/scm/PROJ/repo.git",
    );
    assert_eq!(shas.len(), 1);
    let url = greviewer::repo::origin_url(dir.path()).expect("origin url");
    let repo = parse_origin(&url).expect("parses");
    assert_eq!(
        (repo.project.as_str(), repo.slug.as_str()),
        ("PROJ", "repo")
    );
}
