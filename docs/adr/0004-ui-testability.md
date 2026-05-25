# ADR-0004: UI testability — dispatch-level coverage for user affordances

**Status:** Accepted

**Date:** 2026-05-25

---

## Context and Problem Statement

Greviewer has already shipped a concrete UI wiring defect: the open-repository menu item and `cmd-o` binding compile, pass the test suite, and still do not visibly dispatch in the running app. The failure was possible because tests called application methods directly instead of exercising gpui's action and key-dispatch chain.

In an AI-authored desktop app, that gap is architectural. Method-level tests prove state transitions, but they do not prove that a user can reach those transitions through the rendered UI. Agents need a binding rule that distinguishes focused state tests from affordance tests.

## Decision

Every user-facing UI affordance must have automated coverage that drives gpui's public interaction boundary: keystroke dispatch, action dispatch, typed events, or rendered-view interaction. Calling private helper methods or app-internal methods is allowed for focused unit coverage, but it does not satisfy affordance coverage.

For the current open-repository slice, the required coverage is:

- the live keymap binds `cmd-o` to `OpenRepository`;
- Greviewer registers the `Greviewer -> Open Repository...` menu item for `OpenRepository`;
- dispatching `OpenRepository` through gpui invokes the folder picker;
- picker success, cancellation, picker failure, and invalid repository selections preserve or update app state as specified.

## Implementation Guidance

Use dependency injection for OS or platform services that gpui's test platform cannot drive directly. The first required abstraction is `PathPicker`, because `prompt_for_paths` is unimplemented in gpui's test platform.

Use typed app events as public observability for UI side effects that third-party widgets do not expose. The existing `OpenFailed` event is the pattern for notification verification because `gpui-component` keeps notification fields private.

Use registration snapshots only where gpui does not expose state after registration. The first snapshot is `MenuSnapshot`, built from the same source that produces the gpui menus.

## Accepted Gaps

The automated suite does not claim to verify real native menu clicks, real OS folder dialogs, real display rendering, window-manager behavior, or pixel snapshots. Those remain manual QA gaps until the app has enough visual complexity or failure history to justify a new ADR.

## Consequences

Adding a view, menu item, key binding, command, or other UI affordance without dispatch-level test coverage is a review defect. Direct method tests can remain as lower-level coverage, but they cannot be cited as proof that the UI wiring works.

---

## References

- ADR-0003: Testing strategy
- `docs/superpowers/specs/2026-05-25-ui-testability-design.md`
