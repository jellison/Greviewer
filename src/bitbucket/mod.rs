//! Read-only Bitbucket Data Center pull-request integration.
//!
//! Everything that touches the network, JSON, or the `BITBUCKET_TOKEN`
//! credential is confined to this module. The rest of the app sees only the
//! typed `PullRequest` value, the `BitBucketSession` entity, and its events.
//! The HTTP client (`ureq`) is reached exclusively through the `HttpTransport`
//! trait so the crate choice never leaks and tests inject a fake transport (no
//! live network in tests).
//!
//! Submodules (`remote`, `model`, `client`, `session`) and their public
//! re-exports are added by subsequent implementation tasks.

mod client;
mod model;
mod remote;
mod session;

pub use client::{
    fetch_open_pull_requests, BitBucketError, HttpResponse, HttpTransport, UreqTransport,
};
pub use model::PullRequest;
pub use remote::{parse_origin, BitBucketRepo};
pub use session::{BitBucketSession, BitBucketSessionEvent, LoadState};
