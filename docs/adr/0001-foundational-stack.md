# ADR-0001: Foundational Stack — gpui + gpui-component + permissive Rust crates

**Status:** Accepted

**Date:** 2026-05-22

---

## Context and Problem Statement

Greviewer is a desktop code-review tool for Git commits, written in Rust, that will be one hundred percent built by AI agents. The first foundational decision is the technology stack: which UI framework, which component library, which Git/diff/syntax-highlighting libraries, and what posture to take toward Zed's source code (which the project owner explicitly wants to draw from for aesthetic and structural inspiration).

Two forces shape this decision.

First, **AI-agent productivity.** The codebase will be authored by AI agents working through the Claude Code harness. Frameworks with documented public APIs, broad training data, and stable surfaces produce materially better agent output than frameworks where agents must guess at API shapes from sparse examples. This argues for "agent-friendly" stack choices.

Second, **license posture.** Zed is dual-licensed: the `gpui` crate is published to crates.io as Apache-2.0, while most of Zed's application crates (`editor`, `ui`, `theme`, `project_panel`, `git_graph`, `buffer_diff`, `git`, `git_ui`) are GPL-3.0-or-later. Porting any GPL-3 code into Greviewer flips Greviewer to GPL-3-or-later. For a tool used at a commercial workplace this introduces real policy considerations that go beyond simple license attribution.

The project owner expressed a preference for the Zed aesthetic and an interest in porting Zed components directly to short-circuit implementation effort. That option was investigated in detail before this ADR was drafted; findings are recorded in `docs/research/zed/`.

## Requirements

* The UI toolkit must be a published, depend-able Rust crate with a public API.
* Code rendering quality must support a code review workflow: smooth scrolling, syntax highlighting, side-by-side diff layouts, line-level visual structure.
* AI agents must have enough reference material to write code without continuous human course-correction.
* License posture should keep Greviewer's own license unconstrained for as long as possible.
* The stack must support, in v1, a commit graph, a file tree, a read-only syntax-highlighted file viewer, and a side-by-side diff viewer.
* No constraint on aesthetic match to Zed beyond "code-editor feel"; a close match is desirable but not essential.
* Maintenance burden should be proportional to the team size: one user plus AI agents.

## Options

### Option A: Apache-2.0 stack on gpui + gpui-component

Build Greviewer on `gpui` (Apache-2.0, crates.io) and `gpui-component` (Apache-2.0, crates.io) as the UI foundation. Use permissive Rust crates for the rest of the stack: `git2` (or `gix`) for Git plumbing, `similar` for diff computation, `tree-sitter` plus published grammars for syntax highlighting. Build app-specific surfaces (commit graph, file tree, file viewer, diff viewer) as fresh code on top of these primitives. Treat Zed's source under `~/code/zed` as a reference codebase: read it for idioms and patterns, do not copy from it.

* **Pros:**
  * Zero GPL contamination; Greviewer's license stays unconstrained.
  * No company OSS-policy entanglement beyond what running Zed already involves.
  * Pure crates.io dependencies; no Git pinning, no fork branches, no upgrade-merge cycles.
  * `gpui-component` is a mature published library (~58k LOC) with shadcn-influenced primitives, code editor with tree-sitter highlighting, tables, dock layout, and charts; Glinqpad uses it in production today.
  * AI agents have rich reference material: Zed's own source as exemplary gpui usage, plus the research notes in `docs/research/zed/`.
  * Stable upgrade story: bump versions in `Cargo.toml`, run the verification command, fix breakage.

* **Cons:**
  * Largest implementation cost for app-specific surfaces — file tree, commit graph, diff viewer, and file viewer are all built from scratch (research estimate: 8–12 weeks combined).
  * Aesthetic match to Zed is approximate; `gpui-component` skews shadcn rather than Zed-native. The owner has acknowledged this trade-off.
  * No leverage on Zed's polished commit-graph rendering in `git_graph`; we rebuild that surface ourselves.

### Option B: Apache stack with selective GPL vendor

Same dependency base as Option A, but vendor a small number of Zed source files (status indicators, diff badges, possibly the diff hunk computation in `buffer_diff`) under a `vendor/zed/` directory with provenance headers and a manifest tracking source path and commit. Each vendored file flips its component to GPL-3-or-later; the project as a whole becomes GPL-3-or-later if any GPL code ships.

* **Pros:**
  * Modest savings on a few small surfaces (status indicators, diff stat formatting).
  * Visual fidelity to Zed in the borrowed elements.

* **Cons:**
  * Greviewer becomes GPL-3-or-later; the entire commercial-policy concern reappears for what is, on inspection, a marginal code saving.
  * Manifest workflow and upgrade procedure must be built and maintained from day one, before there is real evidence the savings are worth the overhead.
  * Research showed that the GPL crates with the highest leverage (`git_graph`, `project_panel`, the editor crate) are too tightly coupled to Zed's workspace and project model to be cheap to vendor. The surfaces left where vendoring genuinely helps are small enough to reimplement.

### Option C: Tauri with a web frontend

Rust handles Git plumbing and IPC; the UI is a web application (React, Svelte, or similar) running in a webview. Diff display via Monaco or CodeMirror; commit graph via a JavaScript library or custom SVG.

* **Pros:**
  * Largest training-data surface for AI agents; web UI patterns are deeply represented in agent capability.
  * Mature off-the-shelf code-editor and diff-renderer libraries in JavaScript.
  * Cross-platform support is robust and well-documented.

* **Cons:**
  * Two-language codebase; coordination overhead between Rust and TypeScript layers.
  * Not native; visual feel is "web app in a frame," which conflicts with the owner's stated preference for the Zed aesthetic.
  * IPC ceremony for every UI/Git interaction.
  * Forfeits the gpui rendering quality the owner specifically asked for.

### Option D: egui or Iced

Pure-Rust UI with simpler programming models than gpui. Well-documented, smaller learning surface for agents.

* **Pros:**
  * Excellent documentation and community examples.
  * Simpler than gpui; smaller chance of agents getting stuck.

* **Cons:**
  * UI ceiling is materially lower; egui has a "dev-tool" feel; Iced is Elm-style with native widgets that don't match the editor aesthetic.
  * Code-rendering primitives are weaker; we'd build syntax-highlighted text rendering from lower-level pieces than gpui already provides.
  * Forfeits the Zed aesthetic the owner specifically asked for.

## Decision

Chosen option: **Option A — Apache-2.0 stack on gpui + gpui-component**.

This stack delivers the gpui rendering quality the owner asked for, keeps Greviewer's license unconstrained, and avoids the maintenance complexity of vendoring GPL forks. The implementation cost is real — most app-specific UI is built from scratch — but the alternatives that would reduce that cost each forfeit something the owner explicitly wants. Option B looked attractive at first glance but the research showed the high-leverage GPL crates (`git_graph`, the editor, `project_panel`) are not cheap to vendor; the GPL flip would buy minor cosmetic borrows, not real implementation savings. Option C forfeits aesthetic and rendering quality. Option D forfeits aesthetic and code-rendering ceiling.

### Implementation Guidance

**Initial dependency set.** Refine versions when scaffolding lands; the names and roles are stable.

* `gpui = "0.2"` — Zed's UI framework (Apache-2.0, crates.io). The latest published `0.2.x` at scaffold time.
* `gpui-component = "0.5"` with the `tree-sitter-languages` feature — Apache-2.0 component library (Longbridge).
* `git2 = "0.20"` — libgit2 bindings (permissive). The choice between `git2` and `gix` (gitoxide, pure-Rust) is deferred until the first real Git operation lands; either is acceptable, both are permissively licensed.
* `similar = "2"` — permissive diff library; replaces any reliance on Zed's `buffer_diff`.
* `tree-sitter = "0.24"` and grammar crates as needed — permissive.
* `anyhow`, `serde` (with `derive`), and an async runtime aligned with gpui's executor expectations (typically `smol` or a compatible facade) per Rust convention.

**License rule (binding for v1).**

* Greviewer is licensed under the MIT License. The project's `LICENSE` file ships MIT with the initial scaffolding.
* No file under Greviewer may be copied, vendored, or directly derived from any GPL-3.0-or-later source. This explicitly includes Zed's `editor`, `ui`, `theme`, `project_panel`, `git_graph`, `buffer_diff`, `git`, `git_ui`, and any other GPL-3 Zed crate.
* "Reading Zed's source for idioms" is allowed and encouraged. Agents must produce fresh code. Matching obvious helper-function signatures or domain vocabulary is fine; copying file contents — even small fragments — is not.
* If a future need to vendor a specific GPL-3 file genuinely emerges, that decision is its own ADR. The follow-up ADR must document the license-flip implications, the vendor manifest format, and the upgrade workflow before any vendored file ships.

**Reference material (required reading for agents working in this stack).**

* `docs/research/zed/gpui-primer.md` — gpui public API, idioms, hello-world, layout, state, events, async, testing, common gotchas.
* `docs/research/zed/editor-architecture.md` — Zed editor crate's architecture; what a read-only viewer subset looks like; what to ignore.
* `docs/research/zed/git-and-diff-surfaces.md` — Git/diff/graph crate inventory; why they're not portable; how to build equivalent surfaces.
* `docs/research/zed/file-tree.md` — `project_panel` analysis and the build-on-gpui plan for our file tree.
* `docs/research/zed/ui-and-theme-reusability.md` — `gpui-component` capabilities and the case against Zed's `ui`/`theme` crates as dependencies.

**Verification command (provisional).** Codified in a project guide and wrapped as `bin/check` once Cargo is wired up:

```
cargo check && cargo clippy --all-targets -- -D warnings && cargo test && cargo fmt --check
```

### Migration Plan

Greviewer has no prior implementation; this ADR defines the starting stack rather than migrating from one. The migration considerations are forward-looking.

* Bumping `gpui` versions: routine Cargo upgrade. Read the gpui changelog, run the verification command, fix breakage.
* Bumping `gpui-component` versions: same pattern.
* Should a future component genuinely benefit from GPL vendoring: write a follow-up ADR establishing the vendor workflow and accepting the license flip before any code is copied.

---

## References

* `docs/research/zed/gpui-primer.md`
* `docs/research/zed/editor-architecture.md`
* `docs/research/zed/git-and-diff-surfaces.md`
* `docs/research/zed/file-tree.md`
* `docs/research/zed/ui-and-theme-reusability.md`
* gpui homepage: https://www.gpui.rs/
* gpui crate: https://crates.io/crates/gpui
* gpui-component repository: https://github.com/longbridge/gpui-component
* Zed source (reference, do not port): `~/code/zed`

## Notes

The 8–12 weeks of implementation work for v1 is the combined research-agent estimate across surfaces (file tree 40–60 hours, commit graph approximately two weeks, file viewer two to four weeks, diff viewer two to three weeks, plus integration and polish). Treat it as an order-of-magnitude figure rather than a commitment.

This ADR was Accepted on 2026-05-22 after research-backed review by the project owner. The first scaffolding implementation slice — the `Cargo.toml` manifest with the named dependencies and a runnable gpui window — exercises this decision; any friction encountered there should prompt an ADR amendment rather than a silent deviation.
