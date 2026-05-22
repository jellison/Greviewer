# Greviewer

A desktop code-review tool for Git commits, written in Rust on top of [`gpui`](https://www.gpui.rs/).

Greviewer lets you select a single commit or a contiguous range from the commit graph, browse the rollup change set, and inspect each changed file in a side-by-side diff view.

## Status

Early scaffolding. The repository currently builds a runnable but non-functional shell: a window opens with a placeholder ("No repository open") and exits cleanly when closed. No product behavior from `docs/specs/review/workflow.md` is wired up yet.

## Running

Requires a recent stable Rust toolchain.

```
cargo run
```

A 1280×800 window titled "Greviewer" opens with the placeholder root view. Closing the window exits the process.

## Documentation

- `docs/adr/` — Architecture Decision Records. Start with ADR-0001, ADR-0002, and ADR-0003.
- `docs/specs/` — Feature specifications written in PM voice. The v1 contract lives at `docs/specs/review/workflow.md`.
- `docs/guides/` — Operational reference for how this project is built and written.
- `docs/research/zed/` — Research notes on Zed's source code that inform implementation choices.

## Verification

```
bin/check
```

Wraps `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Run before declaring any change done.

## License

MIT. See `LICENSE`.

## Repository

`git@gitlab.cicd.dc:justin.ellison/greviewer.git`
