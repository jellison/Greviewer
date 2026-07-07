//! The gpui entity that owns one repository's pull-request fetch lifecycle.
//!
//! Fetching runs on `cx.background_executor()` so the UI never blocks (mirrors
//! the AI module's off-foreground pattern, ADR-0005). Each fetch carries a
//! monotonic request id; a response from a superseded request is dropped, so a
//! slow first fetch that lands after a manual refresh cannot clobber fresh data.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{Context, EventEmitter};

use crate::bitbucket::client::{fetch_open_pull_requests, BitBucketError, HttpTransport};
use crate::bitbucket::model::PullRequest;
use crate::bitbucket::remote::BitBucketRepo;

/// The session's current load state, read by the sidebar to pick its content.
#[derive(Debug, Clone)]
pub enum LoadState {
    Idle,
    NotConfigured,
    Loading,
    Loaded(Rc<Vec<PullRequest>>),
    Failed(BitBucketError),
}

/// Emitted as the fetch lifecycle advances. `App` subscribes and reacts.
#[derive(Debug, Clone)]
pub enum BitBucketSessionEvent {
    Loading,
    Loaded(Rc<Vec<PullRequest>>),
    Failed(BitBucketError),
}

/// How the session reads the credential. Injected so tests need no real env.
pub trait TokenSource: 'static {
    fn token(&self) -> Option<String>;
}

/// Production token source: reads `BITBUCKET_TOKEN` from the environment.
pub struct EnvToken;

impl TokenSource for EnvToken {
    fn token(&self) -> Option<String> {
        match std::env::var("BITBUCKET_TOKEN") {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => None,
        }
    }
}

/// Owns the fetch lifecycle for one Bitbucket repository.
pub struct BitBucketSession {
    repo: BitBucketRepo,
    transport: Arc<dyn HttpTransport>,
    token_source: Box<dyn TokenSource>,
    state: LoadState,
    request_generation: u64,
}

impl EventEmitter<BitBucketSessionEvent> for BitBucketSession {}

impl BitBucketSession {
    /// Create a session for `repo` using the production transport and env token.
    pub fn new(repo: BitBucketRepo) -> Self {
        Self::with_dependencies(
            repo,
            Arc::new(crate::bitbucket::client::UreqTransport::new()),
            Box::new(EnvToken),
        )
    }

    /// Construct with injected transport and token source (test seam).
    pub fn with_dependencies(
        repo: BitBucketRepo,
        transport: Arc<dyn HttpTransport>,
        token_source: Box<dyn TokenSource>,
    ) -> Self {
        BitBucketSession {
            repo,
            transport,
            token_source,
            state: LoadState::Idle,
            request_generation: 0,
        }
    }

    pub fn state(&self) -> &LoadState {
        &self.state
    }

    /// Start (or restart) a fetch. If the token is unset, transitions to
    /// `NotConfigured` and emits nothing over the network.
    ///
    /// Note the unset-token case sets `state` to `NotConfigured` but emits
    /// `Failed(NotConfigured)`; the state variant and the event payload differ,
    /// so subscribers should read [`state`](Self::state) on notify rather than
    /// reconstruct state from the event payload.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(token) = self.token_source.token() else {
            self.state = LoadState::NotConfigured;
            cx.emit(BitBucketSessionEvent::Failed(BitBucketError::NotConfigured));
            cx.notify();
            return;
        };

        self.request_generation += 1;
        let generation = self.request_generation;
        self.state = LoadState::Loading;
        cx.emit(BitBucketSessionEvent::Loading);
        cx.notify();

        let transport = Arc::clone(&self.transport);
        let repo = self.repo.clone();

        cx.spawn(async move |this, cx| {
            // The blocking fetch runs off the foreground; only owned, Send data
            // (`transport`, `repo`, `token`) crosses the await boundary. The
            // returned `Vec<PullRequest>` is Send; the non-Send `Rc` snapshot is
            // built afterward inside `this.update`, on the foreground.
            let result = cx
                .background_executor()
                .spawn(async move { fetch_open_pull_requests(transport.as_ref(), &repo, &token) })
                .await;

            // `Err` here means the entity was dropped; nothing more to do.
            this.update(cx, |session, cx| {
                // A superseded request must not clobber a newer one's result.
                if session.request_generation != generation {
                    return;
                }
                match result {
                    Ok(prs) => {
                        let snapshot = Rc::new(prs);
                        session.state = LoadState::Loaded(Rc::clone(&snapshot));
                        cx.emit(BitBucketSessionEvent::Loaded(snapshot));
                    }
                    Err(error) => {
                        session.state = LoadState::Failed(error.clone());
                        cx.emit(BitBucketSessionEvent::Failed(error));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitbucket::client::HttpResponse;
    use gpui::{AppContext as _, TestAppContext};
    use std::cell::RefCell;
    use std::sync::Mutex;

    struct QueuedTransport {
        responses: Mutex<Vec<Result<HttpResponse, BitBucketError>>>,
    }
    impl QueuedTransport {
        fn new(responses: Vec<Result<HttpResponse, BitBucketError>>) -> Arc<Self> {
            Arc::new(QueuedTransport {
                responses: Mutex::new(responses),
            })
        }
    }
    impl HttpTransport for QueuedTransport {
        fn get(&self, _url: &str, _token: &str) -> Result<HttpResponse, BitBucketError> {
            let mut queue = self.responses.lock().expect("responses lock");
            if queue.is_empty() {
                return Ok(HttpResponse {
                    status: 200,
                    body: r#"{"isLastPage":true,"values":[]}"#.to_string(),
                });
            }
            queue.remove(0)
        }
    }
    struct FixedToken(Option<String>);
    impl TokenSource for FixedToken {
        fn token(&self) -> Option<String> {
            self.0.clone()
        }
    }
    fn repo() -> BitBucketRepo {
        BitBucketRepo {
            base_url: "https://bitbucket.cicd.dc".to_string(),
            project: "PROJ".to_string(),
            slug: "repo".to_string(),
        }
    }
    fn ok_body(ids: &[u64]) -> String {
        let values: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":{id},"fromRef":{{"displayId":"b{id}","latestCommit":"sha{id}"}},"toRef":{{"displayId":"main","latestCommit":"base"}}}}"#
                )
            })
            .collect();
        format!(r#"{{"isLastPage":true,"values":[{}]}}"#, values.join(","))
    }

    #[gpui::test]
    async fn loaded_event_carries_the_snapshot(cx: &mut TestAppContext) {
        let transport = QueuedTransport::new(vec![Ok(HttpResponse {
            status: 200,
            body: ok_body(&[5, 6]),
        })]);
        let session = cx.new(|_| {
            BitBucketSession::with_dependencies(
                repo(),
                transport,
                Box::new(FixedToken(Some("tok".to_string()))),
            )
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&events);
        let _sub = cx.update(|cx| {
            cx.subscribe(&session, move |_, event: &BitBucketSessionEvent, _| {
                sink.borrow_mut().push(event.clone());
            })
        });
        session.update(cx, |session, cx| session.refresh(cx));
        cx.run_until_parked();
        let recorded = events.borrow();
        assert!(matches!(
            recorded.first(),
            Some(BitBucketSessionEvent::Loading)
        ));
        match recorded.last() {
            Some(BitBucketSessionEvent::Loaded(prs)) => {
                assert_eq!(prs.iter().map(|p| p.id).collect::<Vec<_>>(), vec![5, 6])
            }
            other => panic!("expected Loaded, got {other:?}"),
        }
        session.read_with(cx, |session, _| {
            assert!(matches!(session.state(), LoadState::Loaded(_)))
        });
    }

    #[gpui::test]
    async fn unset_token_stays_not_configured(cx: &mut TestAppContext) {
        let transport = QueuedTransport::new(vec![]);
        let session = cx.new(|_| {
            BitBucketSession::with_dependencies(repo(), transport, Box::new(FixedToken(None)))
        });
        session.update(cx, |session, cx| session.refresh(cx));
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(matches!(session.state(), LoadState::NotConfigured))
        });
    }

    #[gpui::test]
    async fn auth_failure_becomes_failed_state(cx: &mut TestAppContext) {
        let transport = QueuedTransport::new(vec![Ok(HttpResponse {
            status: 401,
            body: String::new(),
        })]);
        let session = cx.new(|_| {
            BitBucketSession::with_dependencies(
                repo(),
                transport,
                Box::new(FixedToken(Some("tok".to_string()))),
            )
        });
        session.update(cx, |session, cx| session.refresh(cx));
        cx.run_until_parked();
        session.read_with(cx, |session, _| {
            assert!(matches!(
                session.state(),
                LoadState::Failed(BitBucketError::Auth)
            ))
        });
    }

    #[gpui::test]
    async fn stale_response_is_dropped_after_a_newer_refresh(cx: &mut TestAppContext) {
        // gpui's test executor is a cooperative single-threaded scheduler: both
        // background fetch runnables are polled on the test thread in a
        // seed-randomized order (TestDispatcher::tick picks via an RNG), so we
        // cannot pin which generation's fetch *completes* first, and a blocking
        // channel latch would deadlock the test thread rather than park. Instead
        // we make the result deterministic by keying each response body to the
        // generation's token: `refresh` reads `token_source.token()` synchronously
        // in program order, so refresh#1 (gen 1) captures "g1" and refresh#2
        // (gen 2) captures "g2" before either fetch is spawned. Whichever order
        // the two fetches complete in, the generation guard keeps only gen 2's
        // result, and gen 2's body is deterministically `[2, 3]`.
        struct TokenKeyedTransport;
        impl HttpTransport for TokenKeyedTransport {
            fn get(&self, _url: &str, token: &str) -> Result<HttpResponse, BitBucketError> {
                let body = match token {
                    "g1" => ok_body(&[1]),
                    "g2" => ok_body(&[2, 3]),
                    other => panic!("unexpected token {other:?}"),
                };
                Ok(HttpResponse { status: 200, body })
            }
        }

        struct SequenceToken {
            tokens: Mutex<std::collections::VecDeque<String>>,
        }
        impl TokenSource for SequenceToken {
            fn token(&self) -> Option<String> {
                self.tokens.lock().expect("tokens lock").pop_front()
            }
        }

        let token_source = Box::new(SequenceToken {
            tokens: Mutex::new(["g1", "g2"].iter().map(|s| s.to_string()).collect()),
        });
        let session = cx.new(|_| {
            BitBucketSession::with_dependencies(repo(), Arc::new(TokenKeyedTransport), token_source)
        });
        session.update(cx, |session, cx| {
            session.refresh(cx);
            session.refresh(cx);
        });
        cx.run_until_parked();
        session.read_with(cx, |session, _| match session.state() {
            LoadState::Loaded(prs) => {
                assert_eq!(prs.iter().map(|p| p.id).collect::<Vec<_>>(), vec![2, 3])
            }
            other => panic!("expected Loaded from newest refresh, got {other:?}"),
        });
    }
}
