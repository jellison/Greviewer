# UI Testability Design

This design defines the testability slice that must land before Greviewer ships more UI behavior. The immediate trigger is a real defect in the open-repository affordance: the menu item and `cmd-o` binding compile and pass the current suite, but they do not visibly dispatch in the running app. The defect is not just one broken handler. It exposed a verification gap: current tests call application methods directly instead of driving gpui's dispatch chain.

The goal of this slice is to make every existing UI affordance agent-testable through the same path a user activates. Direct method tests can still exist for focused state and repository behavior, but they cannot count as coverage for key bindings, menu registration, action dispatch, or picker flow.

## Requirements

This slice must codify the rule in an ADR, add the smallest reusable framework needed to test the current app shell, and backfill tests for the open-repository slice. It must stay within Greviewer's existing architecture: a single binary crate, by-feature modules, gpui and gpui-component as the UI stack, and no GPL-derived code.

The framework must let tests answer these questions without launching a real OS window or native folder dialog:

- Is the `OpenRepository` action bound to the expected keystroke?
- Did Greviewer register the expected application menu item for that action?
- Does dispatching the action through gpui invoke the folder picker?
- Do picker success, cancellation, picker failure, and invalid repository selections produce the expected app state and typed observable events?

The framework does not need to verify native menu-bar clicks, real OS folder dialogs, pixel output, window-manager behavior, or screenshot snapshots. Those are explicit residual QA gaps for now.

## Recommended Approach

Use explicit dependency injection for the folder picker, and keep menu observability as a lightweight snapshot of what Greviewer registers with gpui. This gives tests a stable observation point while preserving production behavior.

Two alternatives were rejected. Keeping picker calls directly inside `App::prompt_and_open_repository` would leave the OS dialog unmockable in gpui tests, which is the current blocker. Creating a broad top-level testability subsystem would overfit the repo while it has only one view and one shell action. The narrower app-shell design is enough for the current gap and can grow only when new surfaces prove they need it.

## Module Layout

Keep `src/app.rs` as the root view and app state owner. Add app-owned submodules under `src/app/`:

- `src/app/path_picker.rs` owns the `PathPicker` trait, the production gpui picker implementation, and picker result/error types.
- `src/app/menu.rs` owns menu construction plus `MenuSnapshot`.

Reusable black-box test helpers remain in `tests/common/mod.rs`. A queued test picker can live there because it implements the public picker trait and does not need crate-private access. Small `#[cfg(test)]` helpers may stay beside the app code when they need access to private state.

Avoid adding `src/test_support.rs` in this slice unless implementation shows that several view modules need the same crate-private helpers. A broad test-support module would create a central abstraction before there is enough UI surface area to justify it.

## Data Flow

`lib::run` remains responsible for production application wiring. It initializes gpui-component, registers key bindings, builds the menu and `MenuSnapshot` from one source, registers the `OpenRepository` action handler, opens the root window, and constructs `App` with the production picker.

`App` owns a picker supplied at construction time. `prompt_and_open_repository` asks that picker for one directory instead of calling `cx.prompt_for_paths` directly. The production picker wraps gpui's folder picker. The test picker returns queued outcomes and records the requested options so tests can prove that the app asked for a single directory selection.

The action handler placement is intentionally implementation-flexible. The current global `App::on_action` registration is suspected to be unreachable from the focused root view. The testability slice should first add a failing regression test that dispatches through gpui, then move the handler to the smallest location that gpui's dispatch chain actually reaches. The invariant is that the passing test exercises `simulate_keystrokes("cmd-o")` or the equivalent gpui action dispatch, not direct method calls.

Picker outcomes map to app behavior as follows:

- A selected path calls `open_repository_at`.
- Cancellation leaves the current mode unchanged and emits no `OpenFailed` event.
- Picker failure pushes an error notification, emits `OpenFailed`, and leaves the current mode unchanged.
- A selected path that is not a Git repository flows through `open_repository_at`, preserving the existing repository error behavior.

## Menu Snapshot

gpui does not expose a public reader for menu state after `cx.set_menus`. Greviewer will therefore build menus through a small app-owned function that returns both the gpui `Menu` values and a `MenuSnapshot`. Tests assert against the snapshot.

This snapshot proves Greviewer asked gpui to register `Greviewer -> Open Repository... -> OpenRepository`. It does not prove that the operating system rendered the menu bar or that native menu clicks work. ADR-0004 should name that limitation directly.

## Test Plan

The slice backfills tests for the current app shell and open-repository workflow:

- App boots in `NoRepo` and constructs the placeholder view.
- Opening a valid repository advances to `RepoOpen` and exposes head information.
- Opening a non-repository emits `OpenFailed`, pushes one notification, and preserves mode.
- The live keymap contains `cmd-o` bound to `OpenRepository`.
- The menu snapshot contains `Greviewer -> Open Repository... -> OpenRepository`.
- Dispatching `OpenRepository` through gpui invokes the injected picker.
- A picked valid repository advances to `RepoOpen`.
- A picked non-repository emits `OpenFailed` and preserves mode.
- A cancelled picker preserves mode and emits no `OpenFailed`.
- If gpui supports it cleanly, direct `dispatch_action(&OpenRepository)` follows the same app path as the keystroke.

The direct `open_repository_at` tests remain valuable because they isolate repository success and failure behavior. They are not sufficient for UI wiring coverage.

## ADR-0004

This slice must add `docs/adr/0004-ui-testability.md`. The ADR should make the new rule binding: user-facing UI affordances must have tests that drive gpui's dispatch chain or an equivalent public interaction boundary. Calling private or app-internal methods is allowed for focused unit coverage, but it does not satisfy UI affordance coverage.

The ADR should also record the accepted gaps: real native menu clicks, real OS dialogs, real display rendering, and pixel snapshots remain outside the automated suite until the app has enough visual complexity or failure history to justify them.

## Risks

The main implementation risk is gpui action-dispatch placement. If gpui requires a different handler location than expected, the design permits that change as long as the test goes through gpui dispatch and would have caught the committed bug.

The picker trait may need to mirror gpui's task or receiver shape instead of returning a plain enum. The contract matters more than the exact signature: tests must be able to queue success, cancellation, and failure, and production must still use gpui's folder picker.

The menu snapshot is only a registration snapshot. It is intentionally not a native UI automation layer.

The framework should remain narrow. This slice adds picker injection, menu snapshotting, dispatch assertions, and typed observable events. It does not add screenshot testing, external automation, or a generalized UI-test framework.

## Definition of Done

The slice is complete when ADR-0004 is accepted, the open-repository UI wiring is tested through gpui dispatch, the currently broken action path is fixed under that regression test, all existing app-shell behavior remains covered, and `bin/check` passes.
