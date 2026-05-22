# CLAUDE.md

Greviewer is an MIT-licensed desktop code-review tool built on `gpui`. The codebase is one hundred percent AI-authored.

## Standards of Work

Adopt the global Standards of Work. Specific to Greviewer:

- Run `bin/check` before declaring any work done. Zero tolerance for compiler errors, clippy warnings, or test failures. Suppression directives (`#[allow(...)]`, `#[ignore]`, etc.) require explicit user approval and a documented reason.
- Read before you write. ADRs, guides, and existing code define the conventions; follow them.
- When requirements are unclear, ask. A clarifying question costs seconds; a wrong assumption costs hours.

## Architecture Decision Records

Decisions live in `docs/adr/`. Three ADRs are load-bearing for every change:

- **ADR-0001 — Foundational stack.** gpui + gpui-component + permissive Rust crates. **No file in this repo may be copied or directly derived from any GPL-3.0 source.** That includes Zed's `editor`, `ui`, `theme`, `project_panel`, `git_graph`, `buffer_diff`, `git`, and `git_ui` crates. Reading Zed for idioms is allowed; copying is not.
- **ADR-0002 — Project layout.** Single binary crate, by-feature modules under `src/`, standard Rust test layout. `bin/check` is the verification command.
- **ADR-0003 — Testing strategy.** Four automated levels (unit / integration / view / smoke). **Every view module under `src/` must ship at least one `#[gpui::test]`.** No exemption for "trivial."

When a request conflicts with an ADR, surface the conflict and ask whether to follow the ADR or write a new one.

## Living Guides

Operational reference in `docs/guides/`:

- `writing.md` — voice and structure for any new document under `docs/`. Read before drafting prose.
- `git.md` — conventional-commits format, history hygiene, agent-ref cleanup. Read before committing.

Additional guides are added as the codebase grows.

## Feature Specs

Specs in `docs/specs/{area}/{spec}.md` are PM-voice contracts for product behavior. Read `docs/specs/README.md` for the spec-voice rules.

When working on any feature:

- Scan `docs/specs/` for specs covering the area you're touching.
- Treat the spec as the source of truth for behavior verification.
- Update the relevant spec in the same change whenever behavior is added, removed, or corrected.

## Reference: Zed Source Material

`docs/research/zed/` holds research notes on Zed's codebase. Read the relevant note before working in a related area:

- `gpui-primer.md` — gpui public API, idioms, hello-world, layout, state, events, async, testing.
- `editor-architecture.md` — Zed editor crate; what a read-only viewer subset looks like; what to ignore.
- `git-and-diff-surfaces.md` — Git/diff/graph crate inventory; build-vs-port verdicts.
- `file-tree.md` — `project_panel` analysis and the build-on-gpui plan.
- `ui-and-theme-reusability.md` — `gpui-component` capabilities and the case against Zed's `ui`/`theme` crates.

These are reference notes. Zed's source itself remains the ground truth at `~/code/zed`.

## Local File Safety

Files listed in `.gitignore`, plus local runtime configuration, can contain data that cannot be recovered. Treat all untracked, gitignored, and runtime configuration files as untouchable unless the user explicitly asks you to modify or remove a specific one.

Prohibited operations:

- **Never run `git clean`** in any form.
- **Never run `git stash -u`, `git stash --all`, or `git stash --include-untracked`**.
- **Never delete, overwrite, or move a file that is gitignored** unless the user explicitly requests it by name.
- **Never delete untracked files** to "clean up" or as part of any workflow.

When in doubt, ask.

## Verification Command

```
bin/check
```

Wraps `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Per ADR-0003, `cargo test` exercises all four test levels (unit, integration, view, smoke).
