# Review Threads

Greviewer lets a reviewer attach a short note to a specific piece of a file's diff without
leaving the changeset screen. A thread belongs to the changeset's review — see [Review
Persistence](persistence.md) for how it travels with that record — and is surfaced in the
review sidebar's Threads tab, alongside the AI-generated guide; see [Review
Workflow](workflow.md) for the sidebar's dock and reveal controls.

## What a thread is

A thread is an anchored conversation: one or more reviewer-authored messages attached to a
range of text on one side of one file's diff. It carries the exact text it was written
against, quoted, so the thread stays meaningful even if the surrounding diff shifts. A thread
belongs to the review of the changeset it was written in — pending changes cannot carry
threads, since they cannot carry a review at all (see "Starting a review" in [Review
Persistence](persistence.md)).

A new thread starts as a single message. Replying (see "Replying to a thread" below) adds
another message to the same conversation; nothing about the thread's anchor changes when it
gains a reply.

## Adding a thread

Selecting a run of text in either side of a file's diff, in either wrap mode, offers the
reviewer an affordance to comment on the selection; a keyboard shortcut does the same. Either
one stages a pending anchor for the current selection: it renders visually distinct from a
saved anchor so the reviewer can tell an unsaved thought from a committed one, reveals the
sidebar's Threads tab if something else was showing, and opens a composer at the top of the
threads list, above every saved thread. Only one thread can be staged at a time; staging a
new one while a draft is already open replaces it, discarding the prior draft with no
confirmation.

Saving the draft adds the thread: its anchor turns from the pending color to the saved
color, the Threads tab's count badge increases by one, and the new thread becomes the
selected one (see "Selecting a thread" below). An empty or whitespace-only comment cannot be
saved; the composer stays open until the reviewer either writes something or cancels.
Cancelling — by an explicit control or by dismissing the composer with the keyboard —
discards the draft entirely and leaves no trace: no thread, no anchor, nothing added to any
count.

Staging is available only on a changeset built from committed history; it is not offered on
pending changes, matching the restriction the review guide has (see "Availability" in [Review
Guide](../ai/review-guide.md)). Saving the first thread on a changeset that has no review yet
starts one automatically, exactly as generating a guide does — the reviewer is never required
to start a review by hand first.

**Triggering conditions**

- The user selects a run of text in a file's diff and activates the comment affordance, or the
  equivalent keyboard shortcut.
- The user activates the composer's save control, or cancels the composer.

**Observable outcomes**

- Staging shows a pending anchor distinct from saved anchors, reveals the Threads tab, and
  opens a composer at the top of the threads list.
- Saving turns the anchor to the saved color, adds one to the Threads tab's count, and
  selects the new thread.
- Cancelling removes the pending anchor and the composer, and adds nothing to the review.

**Edge cases**

- The comment affordance and its shortcut do nothing, and no pending anchor appears, while the
  open changeset is pending changes.
- An empty comment cannot be saved; the composer remains open.
- Staging a new thread while a draft is already open discards the previous draft without
  confirmation.
- Saving the first thread on a changeset with no review yet starts one, taking its default
  name from the changeset as usual.

## Replying to a thread

Any saved thread offers a Reply control. Activating it opens a composer on that thread, in
place, for a new message; only one composer — a new-thread draft or a reply — is ever open at
a time, so opening a reply closes an in-progress new-thread draft and vice versa, and opening
a reply on one thread closes a reply already open on another.

Saving the reply appends it to the thread's conversation as the newest message and moves the
thread to the top of the list (see "Seeing threads" below), the same way saving a brand-new
thread does. An empty or whitespace-only reply cannot be saved; the composer stays open.
Cancelling — by an explicit control or by dismissing the composer with the keyboard —
discards the reply with no trace: no message, nothing added to any count, the thread
unchanged.

**Triggering conditions**

- The user activates a saved thread's Reply control.
- The user activates the reply composer's save control, or cancels the composer.

**Observable outcomes**

- Activating Reply opens a composer on that thread and closes any other open composer.
- Saving appends the message to the thread and moves the thread to the top of the list.
- Cancelling removes the composer and adds nothing to the thread.

**Edge cases**

- An empty reply cannot be saved; the composer remains open.
- Opening a reply while a new-thread draft is staged discards the draft; staging a new-thread
  draft while a reply is open discards the reply. Neither asks for confirmation.
- Opening a reply on a thread while a reply is already open on a different thread closes the
  first without confirmation.

## Seeing threads

The Threads tab sits beside the Review tab in the review sidebar (see [Review
Workflow](workflow.md) for the sidebar's dock control). Its label carries a count badge once
the changeset has at least one saved thread; the count totals every file in the changeset and
never includes an unsaved draft.

Every saved thread's anchored range gets a dotted underline in the diff; whichever thread is
selected gets a stronger treatment — a solid underline and a faint fill across the anchored
range, plus a small directional marker beside the line where the anchor begins — so its place
in the code is unmistakable. The marker belongs to selection alone: no unselected thread and
no in-progress draft ever shows one, and it disappears as soon as nothing is selected.

Every thread in the changeset lives in one flat, scrollable list, ordered by its most recent
activity — its own creation, or its newest reply, whichever is latest — with the most
recently active at the top. There is no grouping by file and no per-file headers: the order
is the same no matter which file is open, and nothing the reviewer does short of adding a
thread or replying to one reshuffles it — not selecting, not scrolling, not switching files.
Each row stands on its own: it names the file the thread is anchored to and the line it
points at — a single line number or a line range — alongside every message in the
conversation, oldest first, and when the thread last saw activity. The selected thread is
marked within its own row, so which thread is current reads directly from the list, wherever
the diff happens to be. While a draft is staged, its composer sits at the top of the list,
above every saved thread.

**Observable outcomes**

- The Threads tab's badge shows the total saved-thread count across the whole changeset; it
  is absent at zero and never includes a staged draft.
- A saved anchor shows a dotted underline; the selected thread's anchor shows a solid
  underline, a faint fill, and a directional marker beside the anchor's first line. The
  marker appears for no other thread and not for a staged draft, and it goes away when the
  selection clears.
- The list shows every saved thread in the changeset, most recently active first, regardless
  of which file — if any — is open in the diff. Replying to a thread moves it to the top.
- Each thread row shows the anchored file's name, the anchored line number or line range,
  every message in the conversation in the order they were written, and when the thread last
  saw activity.
- The selected thread's row is visually marked in the list itself.
- Activating a thread row selects it, exactly as described in "Selecting a thread" below.

**Edge cases**

- A thread whose anchor no longer resolves keeps its ordinary place in the list, ordered by
  activity like any other thread, still showing its file name and line reference — it just
  carries no marks in the diff.
- With no saved threads in the changeset and no draft in progress, the tab shows a plain
  empty message instead of a list. Staging the first draft replaces the message with the
  composer.

## Selecting a thread

Selection moves only by activation, and it works in both directions. Activating a thread's
row in the list selects it and takes the diff to it: the file opens first if it was not
already open, and the diff scrolls so the anchored range is in view. Activating a saved
anchor's range in the diff selects its thread and takes the list to it: the list scrolls so
that thread's row is in view near the top.

Scrolling is never a trigger in either direction. Scrolling the diff leaves the list exactly
where it is, and scrolling the list leaves the diff exactly where it is — the reviewer can
read through the diff or browse the list freely without either surface pulling the other
along. The two surfaces move each other only through activation.

Selection is transient — a property of the current session only, never saved with the
thread, and gone once the changeset closes.

**Triggering conditions**

- The user activates a thread's row in the list.
- The user activates a thread's anchored range in the diff.
- The user scrolls the diff or the threads list.

**Observable outcomes**

- Activating a thread's row selects it: its row is marked in the list, its diff marks
  intensify (see "Seeing threads"), the file opens if it was not already open, and the diff
  scrolls to show the anchor.
- Activating an anchored range in the diff selects its thread and scrolls the list so that
  thread's row is in view near the top.
- Scrolling the diff never moves the threads list; scrolling the threads list never moves
  the diff. Neither changes the selection.

**Edge cases**

- Activating the row of a thread whose anchor no longer resolves still selects it — its row
  is marked in the list — but the diff does not move, since the thread has no place to point
  to.
- Selection is never persisted; reopening a changeset or relaunching the application starts
  with nothing selected.

## Collapsing threads

A long-running review can accumulate more threads than fit comfortably on screen. Any saved
thread's row carries a chevron that toggles it between its full form — every message, oldest
first, plus the Reply control — and a collapsed header-only form: the chevron, the anchored
file and line reference, a count of the thread's messages ("1 message" or "N messages"), and
when the thread last saw activity. Activating the chevron affects only that thread and never
selects it or moves the diff; activating anywhere else on the row still selects the thread as
usual, whether the row is collapsed or expanded.

The Threads tab's pinned header, above the scrollable list, carries a collapse-all and an
expand-all control that act on every thread in the open changeset's review at once. Collapse-
all collapses every thread currently in the list; expand-all clears every thread's collapsed
state, whether it was collapsed individually or by a previous collapse-all. The header itself
is shown only once the changeset has at least one saved thread — with none saved (draft
composer aside), there is nothing to collapse or expand.

A newly saved thread always starts expanded, never inheriting a prior collapse-all. Activating
a saved anchor's range in the diff (see "Selecting a thread" above) expands its thread if it
was collapsed, so navigating to a thread never lands on a hidden body. A thread with an open
reply composer stays expanded regardless of its collapsed state, so the composer the reviewer
just opened is never hidden out from under them.

Collapse state is transient, exactly like selection: it lives only for the current session,
is never saved with the thread or the review, and resets — every thread back to expanded —
whenever a repository is opened.

**Triggering conditions**

- The user activates a thread row's chevron.
- The user activates the Threads tab header's collapse-all or expand-all control.
- The user activates a thread's anchored range in the diff.

**Observable outcomes**

- Activating a thread's chevron toggles that thread between its full form and its
  header-only collapsed form, without selecting it or moving the diff.
- Activating collapse-all collapses every thread in the open changeset's review; activating
  expand-all returns every thread to its full form.
- A collapsed row shows the chevron, the file and line reference, a message count, and the
  last-activity timestamp; it shows no messages and no Reply control.
- Selecting a thread from the diff always leaves it expanded, even if it was collapsed
  beforehand.

**Edge cases**

- A newly saved thread starts expanded even if every other thread was just collapsed by
  collapse-all.
- A thread whose reply composer is open stays expanded even while it is otherwise recorded as
  collapsed; closing the composer (saving or cancelling) does not by itself re-collapse it.
- With no saved threads in the changeset, the collapse-all/expand-all header does not render,
  matching the empty-state tab described in "Seeing threads" above.
- Collapse state is never persisted and never restored: reopening a changeset, relaunching the
  application, or switching repositories starts every thread expanded.

## Agent threads

A thread can be addressed to the AI instead of left as a plain note. Alongside the affordance
to comment on a selection (see "Adding a thread"), a run of selected diff text also offers an
"ask the AI" affordance, available only while AI assistance is enabled and only on a changeset
built from committed history — the same availability the review guide has (see "Availability"
in [Review Guide](../ai/review-guide.md)). Activating it stages a draft exactly as commenting
does — a pending anchor, the Threads tab revealed, a composer at the top of the list — but the
composer is framed as a question to the AI, and saving it starts an agent thread rather than a
plain note. An empty question cannot be saved, matching the note composer.

Saving the question adds the thread with the reviewer's question as its first message, selects
it, and sets the AI working on an answer. When the AI finishes, its reply is appended to the
same thread as a new message and the thread reads as a two-message conversation: the reviewer's
question, then the AI's answer. The reviewer can keep the conversation going: replying to an
agent thread sends the reply to the AI, which answers again in the same thread, so an agent
thread grows as an alternating exchange. A reply cannot be sent while the AI is still working on
the previous turn.

While the AI is working, the thread shows a live indicator of what the AI is currently doing,
updating as the turn proceeds, alongside a control to cancel — the same running-and-cancel
convention the review guide uses (see "Panel states" in [Review Guide](../ai/review-guide.md)).
Cancelling stops the turn and leaves the thread with only the messages already saved; no partial
answer is added. If a turn fails — the AI is unreachable, too many AI tasks are already running
elsewhere, or the turn errors partway — the thread shows what went wrong and offers to retry,
and the reviewer's question (and any earlier answers) stay put so retrying resumes from where
the exchange left off. Every AI-authored message is marked as coming from the AI, and an agent
thread that has been answered carries the same standing reminder the guide does — that the
content is AI-generated and should be verified against the diff — shown once for the thread.

An AI-authored reply renders as formatted markdown — headings, lists, and inline or fenced code
with syntax highlighting — rather than as a flat block of text, matching the guide's own
rendering of AI-generated content. The reviewer's own messages, in an agent thread or any other,
always render as plain text.

Only the messages of an agent thread are saved; the live connection to the AI that produced
them is not. Within a single session the AI remembers the exchange, so a follow-up continues the
same conversation. Across an application relaunch that live connection is gone, so the first
follow-up after reopening re-establishes the conversation from the saved messages before
answering — invisibly to the reviewer, who simply sees the exchange continue.

With AI assistance turned off, the ask-the-AI affordance is not offered, and existing agent
threads still appear in the list as read-only history: their messages remain, the AI markings
and the reminder remain, but there is no way to ask a further question — the reply affordance is
withheld until assistance is turned back on.

**Triggering conditions**

- The user selects a run of diff text and activates the ask-the-AI affordance (offered only with
  AI assistance on, and only on a committed changeset).
- The user saves the question composer, or replies to a saved agent thread.
- The user activates the cancel control on a working agent thread, or the retry control on a
  failed one.

**Observable outcomes**

- Saving a question adds an agent thread whose first message is the question, selects it, and
  starts the AI working; the AI's answer is appended as a second message when the turn finishes.
- A working agent thread shows a live activity indicator and a cancel control; a failed one
  shows the error and a retry control.
- Replying to an agent thread sends the reply to the AI and appends its next answer to the same
  thread. AI-authored messages are marked as such, render as formatted markdown, and an answered
  agent thread shows the AI-generated reminder once.

**Edge cases**

- Cancelling a working turn leaves the thread with only the messages already saved — no partial
  answer is written.
- A reply cannot be sent while the AI is still working on the previous turn.
- Too many AI tasks running elsewhere, an unreachable AI, and a turn that errors partway are all
  surfaced as a failure with a retry affordance, matching the guide's failure surface.
- After an application relaunch, the first follow-up in an agent thread re-establishes the
  conversation from the saved transcript before answering.
- With AI assistance off, agent threads are read-only: no ask-the-AI affordance and no reply,
  while the saved messages, AI markings, and reminder remain visible.

## Persistence

Threads travel with the changeset's review record: they survive closing and reopening the
changeset, survive relaunching the application, and are deleted along with the rest of a
deleted review (see "Carrying threads" in [Review Persistence](persistence.md)).

Because a thread's anchor is quoted text at a line position, and the underlying diff can
shift between sessions, an anchor is re-resolved against the diff each time it is loaded:
line-position resolution is tried first, and falls back to searching for the quoted text when
the line position no longer holds it. A thread whose anchor resolves neither way is not
discarded — it still lists among the changeset's threads, just without a place to point to
in the diff, as described in "Seeing threads" above.

A review saved before threads could carry replies still opens without loss: each of its
threads becomes a one-message conversation, indistinguishable in the list from a thread
created directly with one message.

A review saved back when this feature was called Comments still opens exactly as it did
then: each saved comment reappears as a thread, its original text intact as the thread's
opening message.

**Guaranteed invariants**

- A thread persists with its review across application relaunches, and so does every message
  in it.
- Deleting a review deletes every thread it holds, replies included.
- A thread saved before replies existed opens as a normal one-message thread; nothing about
  it is lost or requires the reviewer to act.
- A review saved under this feature's former name, Comments, still opens without loss; each
  saved comment reappears as a thread's opening message.

**Edge cases**

- An anchor that no longer resolves by line position falls back to its quoted text; if
  neither resolves, the thread stays in the list without diff marks.

## Threads without AI assistance

Plain reviewer threads are not an AI feature. With AI assistance turned off, the review sidebar
and its Threads tab remain fully available — adding, replying to, seeing, and selecting plain
threads all work exactly as described above. Only the AI-specific surfaces are unavailable with
assistance off: the guide's Review tab, and the ask-the-AI affordance and further replies on
agent threads (see "Agent threads" above). Any agent threads already saved still appear as
read-only history. See [Review Guide](../ai/review-guide.md) and [Review Workflow](workflow.md)
for the rest of what assistance turns off.

**Edge cases**

- With AI assistance off, the Review tab does not appear in the sidebar; the sidebar shows the
  Threads tab in its place.
- With AI assistance off, existing agent threads remain visible as read-only history — their
  messages, AI markings, and reminder intact — but cannot be asked a further question.
