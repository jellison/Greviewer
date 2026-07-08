# ADR-0005: AI Integration via Headless Claude CLI Sessions

**Status:** Accepted

**Date:** 2026-07-03

---

## Context and Problem Statement

Greviewer is adding AI assistance to the review workflow. The v1 feature set is
three read-only, informational capabilities: a full AI review of the open
changeset that produces structured findings, highlight-a-selection-and-ask
conversations anchored to diff locations, and an AI-written changeset summary.
AI never modifies files or repository state — the whole application is a
read-only lens on a repo, and AI inherits that contract.

Two environmental facts shape the decision. First, the operating company
exposes Claude only through a corporate gateway that speaks the standard
Anthropic Messages API; clients reach it via the `ANTHROPIC_BASE_URL` /
`ANTHROPIC_AUTH_TOKEN` (bearer auth) environment variables plus custom
headers. Any tool that honors those variables — the Claude CLI does — works
against the gateway unchanged. Second, the useful behaviors ("review this
changeset", "research how this function is used") are *agentic*: they require
a harness that gives the model tools (read file, grep, run git), loops on tool
calls, and manages context. The model alone, reached directly over HTTP, is
text-in/text-out and cannot inspect a repository.

The problem is therefore where the harness comes from. Building even a
"small" one in Greviewer — assembling diff context into prompts, deciding
truncation policy for enormous changesets, tuning what to include — is an
open-ended trial-and-error maintenance burden the project explicitly does not
want to carry.

## Requirements

* All AI capabilities are read-only and must be *enforced* as read-only, not
  merely requested via prompts. Spawned sessions must never modify the
  working tree, move HEAD, or otherwise mutate repository state.
* Works against the corporate gateway with zero Greviewer-side credential or
  endpoint configuration in v1.
* Ask-AI is conversational: multi-turn threads with memory, anchored to a
  file/line-range/diff-side selection within a changeset.
* Review findings are the same thread objects with extra metadata (severity
  and similar), not a separate mechanism.
* Multiple AI sessions run concurrently; every session is cancellable at any
  time; closing the changeset or quitting the app terminates all sessions —
  no orphaned processes.
* Threads are ephemeral in v1 (discarded with the changeset, like tabs and
  splits) but modeled as self-contained data so a future "start a review"
  feature can persist them.
* Sessions must reason about the *selected commit range*, which is generally
  not what is checked out; the working tree is not the review subject.
* No homegrown context-assembly/truncation layer; no heavyweight new
  dependencies (ADR-0001 licensing and stack constraints apply).
* Target users (personal + in-company) already run a configured Claude CLI;
  requiring it is acceptable. AI is a toggleable feature with a clear error
  when the CLI is missing or misconfigured.

## Options

### Option 1: Direct Messages API client in Rust

Call the gateway's `/v1/messages` endpoint from Greviewer (e.g. via
`reqwest`), assembling prompts from diffs and repo content ourselves;
optionally build a tool-use loop for agentic tasks.

* **Pros:**
  * Lowest per-request latency; no external binary dependency
  * Full control over requests, streaming, and cost
* **Cons:**
  * Greviewer becomes the harness: context selection, truncation policy for
    oversized changesets, and tool-loop plumbing all land in this codebase —
    exactly the maintenance burden ruled out by requirements
  * Agentic features (full review, codebase research) would mean rebuilding a
    meaningful fraction of Claude Code

### Option 2: Two-track — direct API for simple tasks, CLI for agentic ones

Use raw API calls for summaries and Q&A, and the Claude CLI for reviews.

* **Pros:**
  * Fast path for lightweight requests
* **Cons:**
  * Two integrations, two failure surfaces, and the direct-API track still
    inherits all of Option 1's context-assembly burden for its features

### Option 3: Claude Agent SDK

Embed Anthropic's agent SDK for programmatic harness access.

* **Pros:**
  * Official, deeply integrable harness API
* **Cons:**
  * TypeScript/Python only; embedding from a single-binary Rust app means
    bundling a Node runtime or running a sidecar service — disqualifying

### Option 4: Headless Claude CLI sessions for everything (chosen)

Spawn `claude -p … --output-format stream-json` in the repo directory for
every AI task. Prompts are pointers ("summarize the changes in
`abc123..def456`"), not payloads: the harness reads files, runs git, and
manages context itself. Conversations resume via the CLI's session storage
(`--resume <session-id>`), one fresh process per turn.

* **Pros:**
  * Zero context-assembly logic in Greviewer; the harness owns exploration,
    truncation, and tool use — including full agentic reviews
  * Gateway configuration inherited from the environment for free
  * One integration and one failure surface for all three features
  * Spawn-per-turn makes process lifecycle trivially correct: cancellation is
    `kill`, concurrency is another spawn, crashes are isolated per thread
* **Cons:**
  * Runtime dependency on an installed, authenticated `claude` binary whose
    version and JSON output format we do not control
  * Every task pays harness startup plus agentic exploration latency
    (tens of seconds where a surgical API call would take a few)
  * Higher token consumption than surgical API calls

## Decision

Chosen option: **Option 4 — headless Claude CLI sessions for everything**,
because it is the only option that satisfies the no-homegrown-harness
requirement while delivering the agentic capabilities the features need, and
it collapses the integration to a single subprocess-management problem Rust
handles well. The latency and token costs are accepted; if a specific feature
later proves too slow, a direct-API fast path can be added behind the same
module boundary as an optimization (that would supersede part of this ADR).

A long-lived interactive process per conversation (`--input-format
stream-json` over stdin) was considered within Option 4 and rejected:
spawn-per-turn with `--resume` buys identical conversation memory from the
CLI's own session storage while avoiding keep-alive, liveness detection, and
restart logic — only a ~1s respawn cost, which is noise next to model latency.

### Implementation Guidance

**Module layout (per ADR-0002).** Everything lives in `src/ai/`:
`mod.rs` (public surface: the session-manager entity and domain types),
`thread.rs` (conversation data model), `cli.rs` (spawning, stream-json
parsing, kill handling), `prompts.rs` (task prompt templates). `ai` depends
on `repo` and nothing UI-side; UI layers depend on `ai`. Nothing outside
`src/ai` touches a subprocess or a raw JSON event.

**Domain model.** A `Thread` is one conversation: id, kind (`Review` /
`Ask` / `Summary`), optional anchor (file path, line range, diff side,
changeset identity), transcript of turns, CLI session id once known, and
status (`Idle` / `Running` / `Failed` / `Cancelled`). Review findings are
threads carrying finding metadata. Threads are plain data with no UI or
process coupling, so the future "start a review" feature can persist them.

**Session manager.** A single gpui entity owns the thread registry and all
process handles. It enforces a concurrency cap, exposes cancel-one and
kill-all (invoked on changeset close and app quit; SIGTERM then SIGKILL after
a grace period), and emits typed events that views subscribe to.

**Spawning.** One CLI invocation per turn: working directory is the repo
root, environment inherited untouched, `-p <prompt>` with
`--output-format stream-json` (plus `--verbose`, required by stream-json).
The first turn captures the session id from the CLI's init event; follow-up
turns pass `--resume <session-id>`.

**Read-only enforcement.** Sessions launch with tool allowlisting: read and
search tools permitted, `Edit`/`Write` and all mutating tools denied, `Bash`
restricted to read-only git inspection, and non-interactive permission mode
that denies anything not allowlisted rather than prompting. `git checkout`,
`stash`, and similar state mutations are excluded by the Bash restriction.
The exact flag set is pinned against the installed CLI version during
implementation and covered by a test, since this is the safety-critical
surface. Prompts additionally state the read-only expectation, but prompts
are not the enforcement mechanism.

**Changeset targeting.** Prompts always name the selected commit range and
instruct the session to inspect it through git (`git show`, `git diff`),
never the working tree, which generally reflects a different checkout.

**Event flow.** A background task per running turn reads stdout line by
line and reduces stream-json events to a small internal enum
(`SessionStarted`, `AssistantText` deltas, `ToolActivity`, `Completed`,
`Failed`). The manager applies them to the thread on the main thread and
re-renders. Tool activity is surfaced so long tasks show progress.

**Structured findings.** Review sessions must end with a machine-parseable
JSON findings document, preferably via the CLI's native structured-output
support, otherwise prompt-enforced JSON with a tolerant parser. A parse
failure is a visible thread failure, never a silent one.

**Failure surfaces.** Spawn failure (binary missing) and immediate auth
errors are distinguishable states so the UI can say "Claude CLI not found /
not configured"; this same probe backs the feature-enable toggle. Unexpected
child exit is `Failed` with stderr captured.

**Testing (per ADR-0003).** The CLI boundary is faked for tests: a stub
binary (or injected command) that replays canned stream-json transcripts
exercises the parser, the manager's concurrency/cancellation behavior, and
view rendering without network or a real CLI. Every view module added for AI
ships at least one `#[gpui::test]`. A smoke-level test validates the
read-only flag set against the real CLI where available.

### Migration Plan

Greenfield — no existing AI integration to migrate. The feature ships behind
an enable/disable toggle; disabled remains the default until the integration
is complete. Deferred follow-ups, in no committed order: user-editable
configuration UI (CLI path, model, caps), thread persistence via the "start
a review" feature, and any direct-API fast path (each would arrive as its
own ADR or spec update).

**Amendment (2026-07-08).** The first AI feature (the review guide) has
shipped, so the scaffolding-phase default above has served its purpose: AI
assistance is now enabled by default, and the setting remains only as an
opt-out kill switch. A missing or misconfigured Claude CLI surfaces as a
visible failure state on the AI surfaces themselves — the failure message is
the setup instruction — rather than as silently absent features a new user
could never discover.

---

## References

* ADR-0001 — Foundational stack (dependency and licensing constraints)
* ADR-0002 — Project layout (by-feature module placement)
* ADR-0003 — Testing strategy (view-test obligation, test levels)
* `docs/specs/review/workflow.md` — the review-mode behavior AI attaches to
* Claude Code headless mode: `claude -p`, `--output-format stream-json`,
  `--resume` (https://docs.claude.com/en/docs/claude-code)

## Notes

The corporate gateway speaks the standard Anthropic Messages API with bearer
auth; the CLI works against it purely through inherited `ANTHROPIC_*`
environment variables, which is what makes "no Greviewer-side configuration"
achievable in v1. Feature UI (finding badges, thread sidebars, inline
markers) is deliberately out of scope here; those decisions belong to specs
under `docs/specs/` once the integration exists.
