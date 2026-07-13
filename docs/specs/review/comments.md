# Review Comments

Greviewer lets a reviewer attach a short note to a specific piece of a file's diff without
leaving the changeset screen. A comment belongs to the changeset's review — see [Review
Persistence](persistence.md) for how it travels with that record — and is surfaced in the
review sidebar's Comments tab, alongside the AI-generated guide; see [Review
Workflow](workflow.md) for the sidebar's dock and reveal controls.

## What a comment is

A comment is a reviewer-authored note anchored to a range of text on one side of one file's
diff. It carries the exact text it was written against, quoted, so the comment stays
meaningful even if the surrounding diff shifts. A comment belongs to the review of the
changeset it was written in — pending changes cannot carry comments, since they cannot carry
a review at all (see "Starting a review" in [Review Persistence](persistence.md)).

## Adding a comment

Selecting a run of text in either side of a file's diff, in either wrap mode, offers the
reviewer an affordance to comment on the selection; a keyboard shortcut does the same. Either
one stages a pending anchor for the current selection: it renders visually distinct from a
saved anchor so the reviewer can tell an unsaved thought from a committed one, reveals the
sidebar's Comments tab if something else was showing, and opens a composer at the top of the
comments list, above every saved comment. Only one comment can be staged at a time; staging a
new one while a draft is already open replaces it, discarding the prior draft with no
confirmation.

Saving the draft adds the comment: its anchor turns from the pending color to the saved
color, the Comments tab's count badge increases by one, and the new comment becomes the
selected one (see "Selecting a comment" below). An empty or whitespace-only comment cannot be
saved; the composer stays open until the reviewer either writes something or cancels.
Cancelling — by an explicit control or by dismissing the composer with the keyboard —
discards the draft entirely and leaves no trace: no comment, no anchor, nothing added to any
count.

Staging is available only on a changeset built from committed history; it is not offered on
pending changes, matching the restriction the review guide has (see "Availability" in [Review
Guide](../ai/review-guide.md)). Saving the first comment on a changeset that has no review yet
starts one automatically, exactly as generating a guide does — the reviewer is never required
to start a review by hand first.

**Triggering conditions**

- The user selects a run of text in a file's diff and activates the comment affordance, or the
  equivalent keyboard shortcut.
- The user activates the composer's save control, or cancels the composer.

**Observable outcomes**

- Staging shows a pending anchor distinct from saved anchors, reveals the Comments tab, and
  opens a composer at the top of the comments list.
- Saving turns the anchor to the saved color, adds one to the Comments tab's count, and
  selects the new comment.
- Cancelling removes the pending anchor and the composer, and adds nothing to the review.

**Edge cases**

- The comment affordance and its shortcut do nothing, and no pending anchor appears, while the
  open changeset is pending changes.
- An empty comment cannot be saved; the composer remains open.
- Staging a new comment while a draft is already open discards the previous draft without
  confirmation.
- Saving the first comment on a changeset with no review yet starts one, taking its default
  name from the changeset as usual.

## Seeing comments

The Comments tab sits beside the Review tab in the review sidebar (see [Review
Workflow](workflow.md) for the sidebar's dock control). Its label carries a count badge once
the changeset has at least one saved comment; the count totals every file in the changeset and
never includes an unsaved draft.

Every saved comment's anchored range gets a dotted underline in the diff; whichever comment is
selected gets a stronger treatment — a solid underline and a faint fill across the anchored
range, plus a small directional marker beside the line where the anchor begins — so its place
in the code is unmistakable. The marker belongs to selection alone: no unselected comment and
no in-progress draft ever shows one, and it disappears as soon as nothing is selected.

Every comment in the changeset lives in one flat, scrollable list, ordered by when it was
created with the most recent at the top. There is no grouping by file and no per-file
headers: the order is the same no matter which file is open, and nothing the reviewer does
short of adding a comment reshuffles it — not selecting, not scrolling, not switching files.
Each row stands on its own: it names the file the comment is anchored to and the line it
points at — a single line number or a line range — alongside an excerpt of the comment's
body and when it was written. The selected comment is marked within its own row, so which
comment is current reads directly from the list, wherever the diff happens to be. While a
draft is staged, its composer sits at the top of the list, above every saved comment.

**Observable outcomes**

- The Comments tab's badge shows the total saved-comment count across the whole changeset; it
  is absent at zero and never includes a staged draft.
- A saved anchor shows a dotted underline; the selected comment's anchor shows a solid
  underline, a faint fill, and a directional marker beside the anchor's first line. The
  marker appears for no other comment and not for a staged draft, and it goes away when the
  selection clears.
- The list shows every saved comment in the changeset, most recently created first,
  regardless of which file — if any — is open in the diff.
- Each comment row shows the anchored file's name, the anchored line number or line range, an
  excerpt of the comment's body, and when the comment was created.
- The selected comment's row is visually marked in the list itself.
- Activating a comment row selects it, exactly as described in "Selecting a comment" below.

**Edge cases**

- A comment whose anchor no longer resolves keeps its ordinary place in the list, ordered by
  creation time like any other comment, still showing its file name and line reference — it
  just carries no marks in the diff.
- With no saved comments in the changeset and no draft in progress, the tab shows a plain
  empty message instead of a list. Staging the first draft replaces the message with the
  composer.

## Selecting a comment

Selection moves only by activation, and it works in both directions. Activating a comment's
row in the list selects it and takes the diff to it: the file opens first if it was not
already open, and the diff scrolls so the anchored range is in view. Activating a saved
anchor's range in the diff selects its comment and takes the list to it: the list scrolls so
that comment's row is in view near the top.

Scrolling is never a trigger in either direction. Scrolling the diff leaves the list exactly
where it is, and scrolling the list leaves the diff exactly where it is — the reviewer can
read through the diff or browse the list freely without either surface pulling the other
along. The two surfaces move each other only through activation.

Selection is transient — a property of the current session only, never saved with the
comment, and gone once the changeset closes.

**Triggering conditions**

- The user activates a comment's row in the list.
- The user activates a comment's anchored range in the diff.
- The user scrolls the diff or the comments list.

**Observable outcomes**

- Activating a comment's row selects it: its row is marked in the list, its diff marks
  intensify (see "Seeing comments"), the file opens if it was not already open, and the diff
  scrolls to show the anchor.
- Activating an anchored range in the diff selects its comment and scrolls the list so that
  comment's row is in view near the top.
- Scrolling the diff never moves the comments list; scrolling the comments list never moves
  the diff. Neither changes the selection.

**Edge cases**

- Activating the row of a comment whose anchor no longer resolves still selects it — its row
  is marked in the list — but the diff does not move, since the comment has no place to point
  to.
- Selection is never persisted; reopening a changeset or relaunching the application starts
  with nothing selected.

## Persistence

Comments travel with the changeset's review record: they survive closing and reopening the
changeset, survive relaunching the application, and are deleted along with the rest of a
deleted review (see "Carrying comments" in [Review Persistence](persistence.md)).

Because a comment's anchor is quoted text at a line position, and the underlying diff can
shift between sessions, an anchor is re-resolved against the diff each time it is loaded:
line-position resolution is tried first, and falls back to searching for the quoted text when
the line position no longer holds it. A comment whose anchor resolves neither way is not
discarded — it still lists among the changeset's comments, just without a place to point to
in the diff, as described in "Seeing comments" above.

**Guaranteed invariants**

- A comment persists with its review across application relaunches.
- Deleting a review deletes every comment it holds.

**Edge cases**

- An anchor that no longer resolves by line position falls back to its quoted text; if
  neither resolves, the comment stays in the list without diff marks.

## Comments without AI assistance

Comments are not an AI feature. With AI assistance turned off, the review sidebar and its
Comments tab remain fully available — adding, seeing, and selecting comments all work exactly
as described above. Only the Review tab and the AI-specific controls are unavailable with
assistance off; see [Review Guide](../ai/review-guide.md) and [Review Workflow](workflow.md)
for what that turns off.

**Edge cases**

- With AI assistance off, the Review tab does not appear in the sidebar; the sidebar shows the
  Comments tab in its place.
