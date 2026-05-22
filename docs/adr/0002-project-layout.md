# ADR-0002: Project layout — single binary crate, by-feature modules

**Status:** Accepted

**Date:** 2026-05-22

---

## Context and Problem Statement

ADR-0001 fixed Greviewer as an MIT-licensed single Rust desktop binary built on `gpui` and `gpui-component`. That decision constrains a lot of the project layout but leaves three open questions: whether to start as a single crate or a Cargo workspace, how to organize modules within the source tree, and where automated tests live so they can be discovered and added to consistently.

The codebase will be authored by AI agents through the Claude Code harness. Layout decisions therefore matter for two audiences: future agents who need a predictable place to put new code, and the project owner who reads the diff. A clear, conventional, by-feature layout reduces the rate at which agents have to invent placement decisions on every change.

The companion ADR-0003 commits the testing *strategy*; this ADR commits the testing *layout* (where test files live), so the two ADRs together produce one coherent picture.

## Requirements

* Predictable, idiomatic Rust layout that AI agents recognize without instruction.
* Module boundaries oriented around product features, not architectural layers, so a single feature change touches one folder.
* Cheap to refactor while the codebase is small; expensive layout decisions deferred until the code reveals real boundaries.
* Test layout that supports inline unit tests, integration tests, gpui view tests, and an end-to-end smoke test (per ADR-0003).
* Single source of truth for the verification command so agents and CI cannot diverge.

## Options

### Option A: Single binary crate, by-feature modules under `src/`

One `Cargo.toml` at the repository root. A thin `src/main.rs` calls `greviewer::run()` from `src/lib.rs`. Feature modules under `src/` named after product surfaces (`repo/`, `graph/`, `tree/`, `diff/`, `viewer/`, `selection/`, `changeset/`, `ui/`). Tests follow standard Rust: inline `#[cfg(test)] mod tests` blocks for unit tests, `tests/` directory for integration and smoke tests.

* **Pros:**
  * Smallest possible setup overhead; `cargo new` plus a few directories.
  * Idiomatic Rust for a single-binary app at this scale; AI agents recognize the shape immediately.
  * Refactoring a module is a folder-level operation; renaming or splitting is cheap.
  * `lib.rs` enables clean integration testing without ceremony.
  * Feature-oriented modules align with how review work and bug fixes actually arrive (one feature at a time).

* **Cons:**
  * No hard compile-time enforcement of module boundaries. A determined agent can reach across modules and create coupling that wouldn't survive a workspace boundary.
  * Single `target/` directory grows monolithically as the codebase grows.

### Option B: Single binary crate, by-layer modules (`domain/`, `services/`, `ui/`)

Same crate structure as Option A, but modules organized by architectural layer rather than feature.

* **Pros:**
  * Clear layered architecture; pure logic separated from rendering.

* **Cons:**
  * Misaligned with how features arrive: a single change to "the file tree" touches `domain/file_tree.rs`, `services/file_tree_service.rs`, and `ui/file_tree_view.rs` rather than a single `tree/` folder.
  * Less common in Rust desktop applications; idiomatic for layered server applications, not UI tools.
  * Encourages premature service abstractions before they're justified.

### Option C: Cargo workspace from day one

Multiple crates from the start: `crates/repo/`, `crates/diff/`, `crates/ui/`, etc., each with its own `Cargo.toml`. The application binary depends on each.

* **Pros:**
  * Hard compile-time enforcement of module public APIs. A crate cannot reach into another crate's private types.
  * Per-crate dependency declarations clarify what each part of the codebase actually uses.
  * Easy to publish a sub-crate later if it becomes useful elsewhere.

* **Cons:**
  * Premature: we don't yet know where the real boundaries lie; workspace splits done before that knowledge exists frequently get redrawn anyway.
  * Adding a new feature means deciding which crate it lives in (or creating a new one), updating multiple `Cargo.toml` files, threading dependencies. The ceremony is the cost.
  * Cross-crate refactoring in Rust is materially more painful than within-crate refactoring.

## Decision

Chosen option: **Option A — single binary crate, by-feature modules under `src/`**.

This is the smallest layout that supports our actual needs and the most common shape for a Rust desktop application of this size. Workspace promotion remains available when a real boundary emerges (a reusable diff engine; a CLI companion that shares Git plumbing with the GUI). That promotion is itself an ADR-worthy decision so it does not happen by accident.

### Implementation Guidance

**Directory layout shipped with the first scaffolding:**

```
greviewer/
├── Cargo.toml                  # Single binary crate
├── Cargo.lock
├── LICENSE                     # MIT (per ADR-0001)
├── README.md
├── CLAUDE.md
├── .claude/
│   └── settings.json
├── .gitignore
├── bin/
│   └── check                   # Wraps the verification command from ADR-0001
├── docs/
│   ├── adr/
│   ├── guides/
│   ├── research/
│   ├── specs/
│   └── superpowers/
├── src/
│   ├── main.rs                 # Thin: `fn main() -> Result<()> { greviewer::run() }`
│   ├── lib.rs                  # Module declarations and `run()`
│   ├── app.rs                  # Top-level app entity and wiring
│   ├── repo/                   # Git plumbing (open repo, list commits, file at commit)
│   ├── graph/                  # Commit graph: data model and view
│   ├── selection/              # Commit/range selection state
│   ├── changeset/              # Rollup change-set computation across a range
│   ├── tree/                   # File tree (changed-only and all-files modes)
│   ├── diff/                   # Diff computation and side-by-side view
│   ├── viewer/                 # Read-only file viewer
│   └── ui/                     # Shared UI primitives, theme, layout shells
└── tests/
    ├── common/
    │   └── mod.rs              # Shared test helpers (fixture-repo builders, etc.)
    ├── fixtures/               # Checked-in small Git repos used by integration tests
    └── smoke.rs                # End-to-end app smoke test (per ADR-0003)
```

**Module conventions:**

* Each `src/<feature>/` is a folder, not a single file. A folder gives room to grow and signals "this is a feature area, expect submodules" without requiring an immediate split.
* Use `mod.rs` or the `<name>.rs` + `<name>/` form per Rust convention; pick one and stay consistent within the project. Default to the `<name>.rs` + `<name>/` form (Rust 2018 edition style).
* Public surface from each module is declared explicitly in its top-level file. Avoid `pub use *` re-exports that obscure where types come from.
* The module list above is a starting seam map, not a contract. Modules may be merged, renamed, or split as the code reveals real boundaries; agents should propose those changes in PR descriptions rather than silently restructuring.

**File-size discipline:**

* If a single `.rs` file grows past roughly 800 lines, that is a signal to split. Split decisions belong in the PR that adds the offending code, not deferred to a cleanup later.
* No hard limit; this is a guideline, not a CI check.

**Test layout:**

* Inline unit tests under `#[cfg(test)] mod tests { ... }` at the bottom of each `.rs` file that owns pure logic. Test pure functions and small types.
* Integration tests in `tests/<topic>.rs` for behavior that crosses modules but does not need a window.
* gpui view tests inline alongside the view they exercise (per ADR-0003).
* `tests/smoke.rs` for the single end-to-end app smoke test.
* `tests/fixtures/` holds checked-in fixture repos used by tests; add fixture-builder helpers in `tests/common/mod.rs` so tests that need synthetic repos do not hand-craft them.

**Verification command (`bin/check`):**

```
#!/usr/bin/env bash
set -euo pipefail
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`bin/check` is the single source of truth for "is this branch ready." The CLAUDE.md standards-of-work section requires running it to completion before declaring any work done.

**Workspace promotion criteria:**

The single-crate layout is durable until at least one of the following becomes true:

* A logically separate companion artifact ships from the same repository (a CLI tool, a daemon, a library someone else consumes).
* A part of the codebase has materially different dependency requirements from the rest (e.g., a heavyweight optional feature gated behind a non-default dependency tree).
* Compile-time module-boundary enforcement becomes load-bearing because cross-module discipline has visibly degraded and project-level conventions cannot recover it.

When one of those triggers fires, write a follow-up ADR proposing the workspace split before any code moves.

### Migration Plan

Greviewer has no prior layout; this ADR defines the starting structure. The forward-looking migration consideration is the workspace-promotion criteria above: any later move from single crate to workspace is itself an ADR.

---

## References

* ADR-0001: Foundational stack
* ADR-0003: Testing strategy
* `docs/research/zed/gpui-primer.md` — gpui usage idioms for `lib.rs`/`main.rs` separation

## Notes

The starting module list (`repo`, `graph`, `tree`, `diff`, `viewer`, `selection`, `changeset`, `ui`) is informed by the v1 spec at `docs/specs/review/workflow.md` and may evolve as that spec is implemented. Mismatches between this ADR's seam map and the eventual code structure are expected; the ADR's job is to set the starting shape, not to predict every refactor.
