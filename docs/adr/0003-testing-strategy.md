# ADR-0003: Testing strategy — four automated levels with binding view-test rule

**Status:** Accepted

**Date:** 2026-05-22

---

## Context and Problem Statement

Greviewer's source code is authored by AI agents through the Claude Code harness. That changes the role of automated tests in a way human-built projects sometimes overlook: tests are not only the regression net, they are the primary way the project owner can verify that an agent's work actually functions. An agent can produce code that compiles cleanly and passes a type check while failing to behave as specified. Without a thorough automated test suite that exercises real behavior — including the UI — every "completed" claim from an agent has to be manually re-verified, which negates the time savings AI agents are meant to provide.

End-to-end UI testing is therefore not optional for this project. It is the load-bearing verification layer.

`gpui` ships a test infrastructure (`#[gpui::test]`, deterministic scheduling, virtualized window) that is used heavily inside Zed itself. This makes "live UI testing" achievable without external automation tools, accessibility APIs, or real-display rendering. The strategy below is built around it.

This ADR commits the *strategy* — what kinds of tests exist, what each level covers, and what behavioral rules bind agents. The exact API patterns and helper conventions live in `docs/guides/testing.md`, which is written when the first tests land so it reflects real code rather than speculation.

## Requirements

* Catch behavioral regressions reliably across modules and across UI surfaces.
* Run fast enough to execute on every change; agents should never be tempted to skip the suite.
* Run headlessly on developer machines and in CI without requiring a real display.
* Exercise the actual view code for every UI feature, not only the pure-logic helpers underneath it.
* Enforce a binding rule that AI agents cannot rationalize past: if a view exists, it has at least one test.
* Acknowledge honestly what automated tests cannot verify so the human knows what to look at.

## Options

### Option A: Four-level layered automated suite

Four levels of tests in the same project, each with a defined scope and a defined location:

1. **Pure unit tests.** Inline `#[cfg(test)] mod tests` in each module. Pure functions and small data types.
2. **Integration tests.** `tests/<topic>.rs` files exercising cross-module behavior that does not need a window (e.g., "open repo, compute changeset, assert file list").
3. **gpui view tests.** `#[gpui::test]` tests inline alongside the view they exercise. Boot the view in the gpui test harness, drive events, assert on view state. Headless, deterministic, fast.
4. **App smoke test.** A single `tests/smoke.rs` running the whole app against a fixture repo via `#[gpui::test]`, walking the v1 spec's golden path end to end.

Combined with a binding rule: every view module ships at least one `#[gpui::test]` covering its primary interaction.

* **Pros:**
  * Each level catches a distinct class of regression; they are complementary.
  * Levels 3 and 4 use the same gpui test harness Zed itself relies on, so the approach is proven at scale.
  * Headless and fast; suitable for every-change execution.
  * The binding rule makes "I added a view but no test" a reviewable defect rather than a judgment call.

* **Cons:**
  * Cannot catch real-display rendering bugs (font shaping at the actual DPI, scroll feel, color contrast on the display in use). The project owner exercises the app personally and notices these during regular use; this is acknowledged out-of-band rather than papered over by a brittle automated check.
  * The fixture-repo and helper infrastructure must be built early so tests are cheap to write.

### Option B: Unit and integration only; no UI tests

Tests cover Levels 1 and 2 above. UI is verified by humans during use.

* **Pros:**
  * Smallest test infrastructure investment.
  * Tests run fastest because no view machinery is involved.

* **Cons:**
  * Largest defect-discovery latency for UI bugs: they ship and are caught only when a human happens to use the affected feature.
  * Misaligned with the AI-agent context: the most common agent failure mode in UI code is "the wires don't connect" — an event handler that compiles but never fires, a view that renders without subscribing to its data source. Pure-logic tests cannot catch this.
  * Forces the project owner into the role of every-PR manual QA, which negates much of the time savings from AI authorship.

### Option C: External UI automation (accessibility APIs, OS-level drivers)

Drive a real Greviewer window through the OS's accessibility layer (macOS Accessibility, Linux AT-SPI, Windows UIA) or through a screen-capture-and-OCR tool.

* **Pros:**
  * Tests run against the real rendered application.

* **Cons:**
  * Slow: tests take seconds each; the suite cannot run on every change.
  * Flaky: timing, focus, and rendering variations make tests brittle.
  * `gpui` does not expose first-class accessibility integration today; building this layer ourselves is a significant project of its own.
  * Forfeits the gpui test harness, which is the actual cheapest path to live UI testing.

## Decision

Chosen option: **Option A — four-level layered automated suite with the binding view-test rule**.

This is the only option that catches UI wiring regressions cheaply, scales with the project, and fits the AI-agent authorship context. Option B accepts a class of defect we know agents produce frequently. Option C trades the cheap, deterministic gpui harness for slow, flaky external automation that does not yet exist.

### Implementation Guidance

**Where each level lives:**

| Level | Location | Mechanism |
|---|---|---|
| 1. Unit | Inline `#[cfg(test)] mod tests` in each `.rs` file | `cargo test` |
| 2. Integration | `tests/<topic>.rs` | `cargo test` |
| 3. View | Inline alongside the view, in a `#[cfg(test)] mod tests` block | `#[gpui::test]` macro |
| 4. Smoke | `tests/smoke.rs` | `#[gpui::test]` macro |

All four levels run through `cargo test` (invoked by `bin/check`). There is no separate UI test command.

**Binding rule (enforced in CLAUDE.md):**

* Every view module under `src/` must ship at least one `#[gpui::test]` covering its primary user interaction. "Trivial view" is not a valid exemption; agents will rationalize anything as trivial.
* The test must boot the actual view in the gpui test harness, dispatch at least one user-relevant event, and assert on observable state — not on internal implementation details.
* Adding a view without a corresponding test is a reviewable defect; the PR is not complete until the test exists.

**Fixture infrastructure:**

* `tests/fixtures/` holds checked-in small Git repositories used by integration, view, and smoke tests. These are real `.git` directories committed to the repo (via `tests/fixtures/<name>/` layouts that contain the bare repo contents).
* `tests/common/mod.rs` exposes builder helpers that construct synthetic Git repositories in a temp directory at test time, for tests that need specific commit shapes (e.g., a merge commit between two branches with a known file conflict). Tests should prefer builders over fixtures whenever the test cares about specific Git topology, because builders are self-documenting.
* The first scaffolding PR ships at least one fixture repo and one builder helper, even if no test uses them yet, so the conventions exist before the first feature lands.

**Smoke test scope:**

`tests/smoke.rs` walks the golden path of `docs/specs/review/workflow.md` end to end on a fixture repo:

1. Open the fixture repo.
2. Verify the commit graph populates with expected commits.
3. Select a single commit; verify the file tree shows the expected changed files.
4. Open a changed file; verify the side-by-side diff renders the expected before/after content.
5. Select a range; verify the rollup changeset is correct.
6. Toggle the all-files view; verify unchanged files appear.

The smoke test is a single test function; if it grows past one screenful, that is a signal to extract focused integration tests rather than to grow the smoke test.

**What is explicitly out of scope for the automated suite:**

* Real-display visual verification (font shaping, sub-pixel layout, color rendering on the actual display, scroll feel). The project owner exercises the application during regular use and surfaces visual issues as bug reports; the automated suite makes no claim to cover these.
* Snapshot or golden-image tests for visually-sensitive components. These are deferred. They become candidates for inclusion if a specific surface proves visually regression-prone in practice; that decision is its own ADR.
* Performance and load tests. Greviewer's user-facing performance envelope is "feels fast on a developer laptop reviewing a typical repository." Quantitative perf testing is unjustified at v1 and remains deferred.

**What `bin/check` runs (per ADR-0002):**

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The `cargo test` step covers all four levels. There is no separate "run the UI tests" invocation.

**Detail-level conventions deferred to `docs/guides/testing.md`:**

* The exact `#[gpui::test]` setup pattern (executor handling, app context construction).
* Helper APIs in `tests/common/mod.rs` (fixture-repo builders, app-bootstrap helpers, common assertion helpers).
* Idiomatic patterns for asserting on view state without coupling to private fields.
* Conventions for naming and organizing test functions inside view modules.

These crystallize once real tests exist; codifying them speculatively produces a guide that disagrees with practice. The first scaffolding PR creates the guide as a stub; subsequent feature PRs extend it as patterns emerge.

### Migration Plan

Greviewer has no prior tests; this ADR defines the starting strategy. Forward-looking considerations:

* If snapshot tests for a specific visual surface become justified, that is a follow-up ADR.
* If performance testing becomes justified, that is a follow-up ADR.
* If the binding "every view ships a gpui test" rule proves unworkable in some specific case, the answer is to amend this ADR with a documented exception, not to silently skip tests in PRs.

---

## References

* ADR-0001: Foundational stack
* ADR-0002: Project layout
* `docs/research/zed/gpui-primer.md` — `#[gpui::test]` macro, deterministic scheduling, view test patterns
* Zed source `crates/editor/` (reference, do not port) — extensive examples of `#[gpui::test]` view tests at scale

## Notes

The "binding rule" framing is deliberate. AI agents work by pattern-matching to the rules in the project; rules stated as preferences ("prefer to add a test") get rationalized past. Rules stated as binding requirements ("must ship a test; PR is not complete without it") are followed.
