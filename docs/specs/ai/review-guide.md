# Review Guide

Greviewer can generate an AI-written orientation for a changeset: a plain-language
summary of what changed and why, plus a suggested order for reading the files. It
answers the two questions a reviewer normally has to answer alone before starting:
what does this change do, and where should I start reading? The guide is optional,
generated on request, and lives inside the changeset screen's AI sidebar; see
[Review Workflow](../review/workflow.md) for the control strip that reveals that
sidebar, and [Review Persistence](../review/persistence.md) for how the guide is
stored alongside the rest of a review.

## Availability

A review guide can be generated only for a changeset built from committed
history — a single commit, a range, or a comparison — and only while AI
assistance is enabled. Assistance is on by default; turning it off is an
explicit user choice, and a machine without a working Claude CLI surfaces
that as a visible failure when generating, not as missing controls. A guide
is never available for pending changes: pending changes have no
fixed commits for the AI to inspect, so there is nothing stable to summarize.
Availability does not depend on whether the changeset is already attached to a
review; generating a guide attaches one if none exists yet (see "Persistence"
below).

**Edge cases**

- With AI assistance turned off, the guide's controls and its tab in the
  review sidebar are absent; the sidebar itself can still be present, showing
  only the Threads tab (see [Review Threads](../review/threads.md)).
- Opening the pending changeset never offers a guide, regardless of whether AI
  assistance is on.

## Generating a guide

Generation is explicit: nothing runs automatically when a changeset opens or when
the AI sidebar is revealed. The user starts generation from an affordance in the
guide panel — the same affordance doubles as "Generate" the first time and
"Regenerate" on every later request for the same changeset, and both mean the
same thing: run a fresh pass and replace whatever guide is currently shown.

**Triggering conditions**

- The user activates the generate affordance while the guide panel shows no
  guide yet.
- The user activates the regenerate affordance while a guide is already showing,
  or after a previous attempt failed.

**Observable outcomes**

- Generation begins immediately and the panel switches to its in-progress state
  (see "Panel states" below).
- A previously generated guide for this changeset, if any, is not touched until
  the new attempt finishes; if it fails, the previous guide is preserved and
  remains available to view again.

## What a guide contains

A guide has two parts. The **summary** is a few short paragraphs written in
plain, product-owner language: what user-visible behavior or business rule
changed, and why. The summary never names a file, a line, or a code-level detail
— that information lives only in the second part.

The **suggested review order** lists the changeset's critical files — the
ones where the substantive behavior changes live — in the order the AI
recommends reading them so a reviewer builds understanding before
encountering what depends on it. It is a curated subset, not an inventory:
mechanical fallout (lockfiles, generated output, formatting churn, call
sites updated only to follow a change) is left out, and there is no fixed
count — a large changeset may warrant many entries or only a few. The file
list remains the exhaustive account of what changed. Each listed file
appears exactly once. Each entry shows the file alongside the same
change indicator used elsewhere in the file list, plus a one-sentence
rationale for its place in the order. Entries lead with the file name; a file
whose name is unique within the guide shows no path at all, and when several
entries share a name, each shows only as much of its containing path as is
needed to tell them apart — usually just the immediate folder. Activating an
entry opens that file's diff, exactly as activating it in the file list does.

**Edge cases**

- If the AI's response names a file that is not actually part of the changeset,
  that entry is left out of the reading order; the rest of the guide is
  unaffected and still renders normally.

## Panel states

The guide panel is always in exactly one of four states, and when more than one
condition could apply, the panel resolves them in a fixed order: an in-progress
generation always takes priority, then a failure from the most recent attempt,
then a previously generated guide, and only when none of those apply does the
panel show its starting state.

**No guide yet.** The panel explains what a guide is and offers the affordance to
generate one.

**Generating.** The panel shows a live indicator of what the AI is currently
doing, updating as generation proceeds, alongside a control to cancel. If a
guide was already showing before this generation started, it stays visible
beneath the indicator — visually de-emphasized — so the reviewer keeps something
to read while the new attempt runs, and only swaps in the new guide once it
lands successfully.

**Guide ready.** The panel shows the summary, then the suggested review order,
then the affordance to regenerate, and a fixed reminder that the content is
AI-generated and should be verified against the diff. This reminder is always
present whenever a guide is shown, with no way to dismiss it. Content taller
than the panel scrolls vertically; nothing in a guide is ever unreachable.

**Generation failed.** The panel shows what went wrong and an affordance to
retry. A failure replaces the in-progress indicator; it does not replace a
previously generated guide from an earlier successful attempt — see the next
section.

**Edge cases**

- Cancelling an in-progress generation returns the panel to whatever it was
  showing beforehand: the previously generated guide if one exists, otherwise
  the starting state. Cancelling is not treated as a failure.
- A generation that fails after an earlier one had already produced a guide for
  this changeset hides that earlier guide behind the failure state; the guide
  itself is not discarded (see "Persistence"), and a successful retry brings it
  back, updated.
- Too many AI tasks already running elsewhere in the app, the AI being
  unreachable, and the AI failing partway through a turn are all surfaced as a
  generation failure, each with an explanation appropriate to what happened.

## Persistence

A generated guide is stored with the changeset's review, not with the changeset
session itself, so it survives closing and reopening the changeset and survives
relaunching the application entirely. If the changeset has no review yet when
generation completes, one is created automatically to hold the guide — the
reviewer is not required to start a review first. Reopening a changeset that
already has a guide shows it immediately, with the regenerate affordance
available right away.

Regenerating replaces the stored guide outright: there is no history of past
guides, only the most recent one. Because a guide is generated from a fixed
commit range, a guide already stored for a changeset never goes stale on its
own — regenerating is always a deliberate choice, not a response to the
underlying commits changing.

**Guaranteed invariants**

- A guide generated for a changeset is available again the next time that exact
  changeset is opened, in the same application session or a later one.
- Regenerating a guide fully replaces the previous one; nothing about the old
  guide persists once the new one lands.

**Edge cases**

- Deleting the review a guide belongs to removes the guide along with it, the
  same as any other part of a deleted review.
