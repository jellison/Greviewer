# Feature Specs

Specs are normative contracts for product behavior. Each file describes what a feature does from the user's vantage point, not how the system implements it. A reader should be able to understand the contract without opening the codebase or running the app.

## Voice

Write for a product manager. Describe what the user does and what the user observes. The reader should never need to know which class, file, route, or library implements the behavior in order to understand the contract.

## Sections to include

_Optional_ sections to consider; none of these are strictly required if it doesn't make sense (i.e., do not force content).

- **Triggering conditions** — user actions, system events, or context changes that activate the behavior.
- **Observable outcomes** — visible state, available actions, messages surfaced to the user.
- **Guaranteed invariants** — ordering, persistence, idempotency, and other properties the user can rely on.
- **Edge cases the user can encounter** — empty states, unreachable targets, conflicts between actions, error surfaces.

## What to leave out

- **Code references** — class names, function names, file paths, type names, or any identifier drawn from the source tree.
- **Transport and storage mechanics** — wire encodings, library names, internal event names.
- **Volatile UI placement/verbiage** — pixel positions, directional locators ("top-left of the chart header," "bottom-right corner"), or exact message verbiage. Describe the affordance ("a control that opens the diff for a file"), not where it currently sits or what shape it takes today (popover vs. modal, button vs. menu item).
- **Engineering tunables** — magic numbers, named constants, default values that exist as implementation knobs rather than user-visible behavior.

A useful test: if the team rewrote the implementation in a different framework or moved a control to a different corner of the screen, the spec should not need to change.

## Brevity

A spec is a contract, not a tour. Cut anything that does not narrow the contract or sharpen the reader's understanding of it. When two surfaces share behavior, factor the shared rules into a canonical spec and link to it rather than restating them.

## Updating specs

Specs ship alongside the code they describe. When behavior is added, removed, or corrected, update the relevant spec in the same change. A spec that disagrees with the running product is a defect — fix one or the other before merging.

## Template

```markdown
# {{Feature Area}}

{{One or two sentences framing the surface and pointing to related specs.}}

## {{Behavior name}}

{{Prose describing what the behavior does from the user's perspective and any invariants the user can depend on.}}

**{{Section}}**

{{Section content}}
```
