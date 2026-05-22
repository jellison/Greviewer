# Writing Guide

## Purpose

This guide defines how we write new documents in `docs/` so the writing quality matches the quality bar for the software itself. The goal is not decorative prose. The goal is better decisions, faster alignment, and fewer defects caused by unclear thinking.

Greviewer writing is narrative-first. We state the recommendation, explain the problem and stakes, walk through alternatives and trade-offs, and then present precise implementation detail. Readers should be able to understand why a decision is correct before they inspect field-level or API-level details.

## Scope and Adoption

This guide applies to newly created documents under `docs/`.

Existing documents are not required to be rewritten to conform to this guide. They can be upgraded later when there is a clear reason to do so.

## The Narrative Standard

Write prose that carries an argument, not disconnected fragments. A good document reads as a coherent sequence of claims supported by evidence and constraints.

Establish enough context and decision drivers before introducing a solution. Readers should understand the problem, requirements, and constraints before they evaluate a recommendation.

Work backwards from user, operator, or developer impact. Explain what breaks, slows down, or remains risky in the current state.

Name alternatives explicitly. A decision without alternatives is usually a hidden assumption. Explain what was considered, why it was rejected, and which trade-offs we are accepting.

Separate argument from specification. The narrative body explains _why_ and _how_. Structured sections and appendices define exactly _what_.

## Document Flow

Use this default narrative arc for most docs.

1. Reader context and current-state problem.
2. Requirements, constraints, and non-goals.
3. Recommendation with rationale and intended outcome.
4. Normative detail (schema, invariants, interfaces, operational procedures).
5. Rollout, verification, and follow-up.

For material decisions, include a distinct alternatives section before the recommendation. For small or low-risk docs with one viable path, state that condition explicitly and proceed.

If a document needs a different order, explain that choice briefly in the opening.

## Bullet Discipline

Bullets are a tool, not a default writing mode.

Use bullets when the content is truly list-shaped: invariants, field definitions, stepwise procedures, acceptance criteria, or concise option comparisons.

Do not use bullets to avoid writing paragraphs that carry reasoning. If the section is making an argument, write prose.

When you use bullets, keep each item concrete and parallel. If a list grows long, break it into narrative plus a smaller list.

## Doc-Type Profiles

All new docs in `docs/` follow the same narrative standard, but each category has a slightly different center of gravity.

### `docs/adr/*`

ADRs are durable records of architectural choices. They must make options and consequences unmistakable so future contributors can understand not only what we chose, but why competing choices were rejected.

### `docs/guides/*`

Guides are operational reference for how we work today. They should prefer explanatory prose with concrete examples, then use lists for rules and checklists that need high scanability.

### `docs/specs/*`

Specs are contracts. They still begin with narrative framing, but they should quickly transition into unambiguous normative statements and verification expectations. See `docs/specs/README.md` for the spec voice rules.

### `docs/research/*`

Research notes capture findings from investigation. They are reference artifacts, not contracts. Lead with a TL;DR and back it with cited evidence (file paths, commit hashes, links). Avoid recommending action; the research informs ADRs and specs, which carry the action.

## Quality Rubric

A document is ready for review when all required checks pass.

### Required for all new docs

1. The opening states the document intent and decision question.
2. The problem statement is specific and evidence-based.
3. Requirements and constraints are explicit.
4. The recommendation or guidance is explicitly tied to requirements and constraints.
5. Normative details are precise and grouped coherently.
6. Verification expectations are explicit enough for review.

### Recommended for decision-heavy docs

1. Alternatives are explicit, with rationale for rejection.
2. Trade-offs and residual risks are named directly.
3. Argument sections are prose-first.
4. Bullet usage is deliberate and limited to list-shaped information.

## AI-Assisted Drafting

AI accelerates drafting, but first-pass output is usually stronger on completeness than on voice.

Use a two-pass workflow for AI-authored or AI-assisted documents.

1. Pass one: technical completeness and factual correctness.
2. Pass two: narrative rewrite for clarity, flow, and bullet discipline.

Do not accept first-pass AI prose when it reads like a generic checklist without argumentation.

## Definition of Done for New Docs

A new document in `docs/` is done when it is technically correct, narratively coherent, and reviewable by a reader who did not author it.

That means the reader can answer three questions after a single read: what decision is being made, why this is the right decision now, and what exact contract or behavior follows from that decision.
