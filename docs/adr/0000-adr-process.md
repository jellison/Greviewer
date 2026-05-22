# ADR-0000: Architecture Decision Records Process and Guidance

**Status:** Accepted

**Date:** 2026-05-22

---

## Context and Problem Statement

We need a consistent, lightweight way to capture architectural decisions and developer guidance so future contributors (humans and AI agents) understand why choices were made and how to apply them. Industry practice recommends keeping ADRs close to the codebase and using them both to log decisions and to share best practices.

## Requirements

* Central, versioned decision log stored with the codebase
* Single, predictable template and status vocabulary
* Encourages concise rationale, alternatives, and consequences
* Supports guidance-only ADRs (e.g., standards, playbooks) in addition to choice-driven decisions
* Easy for any contributor to propose, review, and discover
* Clear supersession links so readers can trace history

## Options

### Option 1: Adopt repository-hosted ADRs using the shared template (docs/adr)

Store all ADRs in `docs/adr/` using the existing `xxxx-adr-template.md`, with sequential numbering, PR review, and explicit statuses. Allow both decision-comparison ADRs and guidance-only ADRs.

* **Pros:**
  * Co-located with code; versioned and reviewable
  * Consistent format improves readability and searchability
  * Works for both decision rationale and ongoing guidance
  * Clear lifecycle via status and supersession links

* **Cons:**
  * Requires contributor discipline to author and review
  * Adds lightweight overhead for small changes

### Option 2: Document decisions in an external wiki or shared doc space

Use a wiki/Doc for decisions; link from the repo when helpful.

* **Pros:**
  * Low friction for non-engineering stakeholders
  * WYSIWYG editing can feel approachable

* **Cons:**
  * Not versioned with code; context can drift
  * Harder to review alongside changes
  * Links can rot; discoverability relies on search

### Option 3: Rely on implicit knowledge and ad hoc PR notes

Do not maintain formal ADRs; depend on commit messages and PR descriptions.

* **Pros:**
  * Zero upfront effort
  * No new process to learn

* **Cons:**
  * Decision rationale is easily lost ("decision amnesia")
  * Onboarding and incident analysis become harder
  * No consistent guidance channel for developers

## Decision

Chosen option: **Adopt repository-hosted ADRs using the shared template**. This keeps decision history co-located with the code, supports both rationale and guidance, and provides a durable, reviewable log with clear lifecycle states.

### Implementation Guidance

* **Location and numbering:** Place ADRs in `docs/adr/` with zero-padded sequential numbers (`0000-title.md`) and the heading `ADR-XXXX: <title>`.
* **Template:** Start from `docs/adr/xxxx-adr-template.md`; keep the sections unless a section is explicitly not applicable. Keep ADRs concise (aim for ≤2 pages).
* **Statuses:** Use `Proposed`, `Accepted`, `Deprecated`, or `Superseded by ADR-YYYY`. Superseding ADRs must link both directions.
* **Scope:** One ADR per decision or guidance topic. Guidance-only ADRs (e.g., code style, logging conventions) are encouraged when they clarify expectations without comparing options.
* **When to write:** Create an ADR when making or standardizing an architecturally significant choice, when guidance needs durable visibility, or when alternatives were considered and rejected.
* **Review:** Submit ADRs via PR. At least one reviewer should confirm clarity of context, options, decision, and consequences. If confidence is low, state that and capture follow-up triggers.
* **Discoverability:** Reference relevant ADR IDs in README sections, package docs, and PRs that implement or depend on them.
* **AI/developer usability:** Prefer actionable language, checklists, and explicit rules over prose-only rationale. Include examples when guidance affects code structure or style.

### Migration Plan

* This ADR retroactively establishes the process; all existing ADRs remain valid.
* For future decisions, authors must start from the shared template and follow numbering.
* Backfill as time allows: write ADRs for past significant decisions lacking coverage (prioritize areas with active work or recurring questions).
* Mark any process deviations (e.g., wiki-only records) as deprecated or superseded once equivalent ADRs exist.

---

## References

* Google Cloud Architecture Center — Architecture decision records overview (https://cloud.google.com/architecture/architecture-decision-records)
* ADR GitHub organization — background and templates (https://adr.github.io/)
* GDS "Documenting architecture decisions" guidance (https://technology.blog.gov.uk/2018/03/02/technology-architecture-and-why-its-important/)

## Notes

This ADR is intentionally broad to cover both decision rationale and prescriptive guidance. Specific domains (e.g., code style, logging) should record detailed rules in their own ADRs and reference this process ADR for lifecycle and format.
