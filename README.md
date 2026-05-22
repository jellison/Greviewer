# Greviewer

A desktop code-review tool for Git commits, written in Rust on top of [`gpui`](https://www.gpui.rs/).

Greviewer lets you select a single commit or a contiguous range from the commit graph, browse the rollup change set, and inspect each changed file in a side-by-side diff view.

## Status

Pre-implementation. The repository currently holds the architectural decisions, feature specs, and reference research that will guide construction. See `docs/adr/` for the foundational decisions.

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
