# Review Persistence

A review is the durable form of a code review: the user gives a changeset a name, works through it, and can leave and come back to it later. Everything else in the review experience — selecting commits, browsing the change set, inspecting diffs — is ephemeral, gone the moment the changeset closes. This contract defines the one part that survives: what a review is, how the user starts, names, resumes, completes, reopens, and deletes one, where reviews are listed, and what the user can rely on across application launches. The review-navigation surfaces themselves — the commit graph, the window bar, the sidebar — are specified in [Review Workflow](workflow.md); this spec is the source of truth for review lifecycle and persistence, and those surfaces point here.

A review anchors to exactly one changeset: a single commit, a contiguous range of commits, or a directional comparison between two commits. The anchor is the identity of the reviewed commits, not a moment in time — the same range always names the same review, and a comparison's direction is part of its identity, so reviewing A into B is a different review from B into A. Pending changes can never be reviewed durably; a review always names committed work. At most one review exists per changeset in a repository, so starting a review on a changeset that already has one resumes the existing review rather than creating a second.

## Starting a review

From the controls that describe an open commit-addressed changeset, the user can start a review of it. The affordance is present only for changesets built from committed work — a single commit, a range, or a comparison — and never for pending changes, which cannot be reviewed durably. Starting a review names the changeset and begins tracking it; from that point the changeset is attached to a review until the user detaches it by deleting the review.

A new review takes a default name drawn from the changeset itself: the summary line of the newest reviewed commit for a single commit or a range, and a plain "*target* into *base*" phrasing for a comparison, naming the two commits it merges. The user can rename it at any time (see "Being in a review").

**Triggering conditions**

- The user activates the start-review control while a commit-addressed changeset is open.
- The user activates the start-review control on a changeset that already has a review.

**Observable outcomes**

- Starting a review attaches it to the open changeset and gives it a default name derived from the changeset: the newest commit's summary for a single commit or a range, or a "*target* into *base*" phrasing for a comparison.
- The changeset stays open, now showing the review's controls (see "Being in a review").

**Edge cases**

- The start-review control is absent while pending changes are the open changeset; pending changes cannot be reviewed durably.
- Starting a review on a changeset that already has one resumes the existing review — its name, dates, and status are unchanged — rather than creating a duplicate. A changeset never carries two reviews.

## Being in a review

While a changeset that is attached to a review is open, the window bar identifies the review by name alongside the changeset identifier, so the user always knows which review they are in. The controls that describe the open changeset carry the review's details and the actions that act on it: the review's name, shown as an editable field; the dates that frame the review's life; a control to complete or reopen the review; and a control to delete it.

The name is editable in place. Committing an edit — by confirming it or by moving focus away — renames the review. An empty name is not a valid review name: an attempt to clear the name is rejected and the previously shown name is restored. Two reviews may share a name; names are for the user's benefit and are not required to be unique.

The dates read as a short life history: when the review was started, when it was last active, and, once completed, when it was completed. Renaming, resuming, completing, and reopening a review all count as activity and advance its last-active time; casual browsing does not.

**Triggering conditions**

- A changeset attached to a review is open.
- The user edits the review's name and commits the edit by confirming it or moving focus away.
- The user clears the name field and commits the edit.

**Observable outcomes**

- The window bar shows the review's name beside the changeset identifier while the review is attached; with no review attached it shows only the changeset identifier.
- The changeset's descriptive controls carry the review's editable name, its started and last-active dates (and its completed date once completed), a complete/reopen control, and a delete control.
- Committing a non-empty name edit renames the review and counts as activity.

**Edge cases**

- Committing an empty name is rejected; the field returns to the name it showed before the edit, and nothing is renamed.
- Two reviews are allowed to carry the same name.

## Completing and reopening a review

A review is either active or completed. Completing a review marks it done without detaching it from the changeset — the changeset stays open and reviewable, and the review's controls now offer to reopen it instead of complete it. Reopening reverses the transition, returning the review to active. Both transitions count as activity. Completion records the moment it happened, which the review's dates then show.

**Triggering conditions**

- The user activates the complete control on an active review.
- The user activates the reopen control on a completed review.

**Observable outcomes**

- Completing an active review marks it completed, records the completion time, and keeps the changeset open and attached; the control now offers to reopen the review.
- Reopening a completed review returns it to active and clears the completed marker; the control offers to complete it again.
- Both transitions count as activity and advance the review's last-active time.

## Deleting a review

Deleting a review removes it permanently. Because the action cannot be undone, the delete control asks for a confirming second activation: the first activation arms it and asks for confirmation, and any other action disarms it without deleting anything. Confirming removes the review from every list it appeared in.

Deleting the review attached to an open changeset detaches it: the changeset stays open, but as an ordinary ephemeral session with no review — exactly the state it would have been in had the user never started a review. Closing the changeset afterward discards it like any unreviewed browsing.

**Triggering conditions**

- The user activates a review's delete control.
- The user activates the delete control a second time to confirm, or takes any other action while it is armed.

**Observable outcomes**

- The first activation of a delete control arms it and asks for confirmation; a second activation deletes the review permanently.
- Any other action while the control is armed disarms it and deletes nothing.
- Deleting the review attached to the open changeset detaches it and leaves the changeset open as an unreviewed session; the changeset is not closed.

## Listing and resuming reviews

Graph mode lists the open repository's reviews in a dedicated section of the branch sidebar, above the branch and tag sections. The section lists every review the user has started for the repository, no matter which of its worktrees it was started from, and it is present only when the repository has at least one review. Because a review is neither a branch nor a tag, the section is absent while the user is filtering the sidebar by branch name — there is nothing for a branch-name query to match — and returns when the filter is cleared.

Active reviews list first, most recently active first. Completed reviews are gathered behind a single collapsed "Completed" group beneath them, which reports how many there are and expands in place to reveal them, also most recently active first. A completed review's row is muted. Each row shows the review's name and a compact identifier of the changeset it reviews — a single commit's short identifier, a range's oldest and newest identifiers, or a comparison's base and target identifiers.

Activating an available review resumes it: the graph's selection becomes the review's changeset, the graph scrolls so the newest reviewed commit is visible — loading older history first if that commit is not yet in the loaded window — and the changeset opens with the review attached, taking the user straight into review mode exactly as if they had selected and opened that changeset by hand. Resuming counts as activity. Each row carries a delete control on hover, behind the same arm-then-confirm second activation as everywhere else (see "Deleting a review").

Because the sidebar is a review-navigation surface, its full behavior — section header, collapse, ordering, counts — is specified alongside the other sidebar sections in [Review Workflow](workflow.md); this section defines only the review-lifecycle rules that resuming and listing depend on.

**Triggering conditions**

- A repository with at least one review is open, the window is in graph mode, and the sidebar's branch filter is empty.
- The user activates a review row, the "Completed" group, or a review's delete control.

**Observable outcomes**

- The Reviews section lists active reviews first, most recently active first, then a "Completed" group when any review is completed.
- Each review row shows its name and a compact identifier of the commit, range, or comparison it reviews; completed rows are muted.
- Activating an available review resumes it: the graph selects and reveals the review's changeset — loading older history if needed — and opens it into review mode with the review attached. Resuming counts as activity.
- Activating the "Completed" group reveals or hides the completed reviews beneath it.

**Edge cases**

- A repository with no reviews shows no Reviews section.
- Filtering the sidebar by branch name hides the Reviews section entirely; clearing the filter restores it.
- Deleting the last review removes the Reviews section along with it.

## Reviews whose commits are gone

A review can outlive the commits it named. A branch can be deleted, history can be rewritten so a range's endpoints no longer lie on a single ancestry path, or a comparison's two sides can lose their common ancestor. Such a review is unavailable: it still appears in every list, rendered muted, but the user cannot resume it. Activating an unavailable review opens nothing and instead surfaces a clear message that its commits are no longer in the repository. Renaming and deleting an unavailable review still work — the user can tidy up or clean out a review whose commits are gone.

Availability is decided fresh every time the review list is rebuilt, never cached. A review that becomes unreviewable reflects that the next time the list refreshes; and if the missing commits return — history is restored, a deleted branch is recreated — the review becomes available again on its own, with no action from the user.

**Triggering conditions**

- The commits a review named are removed from the repository, or restored.
- The user activates an unavailable review.
- The user renames or deletes an unavailable review.

**Observable outcomes**

- An unavailable review still appears in its list, rendered muted like a completed review.
- Activating an unavailable review opens nothing and surfaces a message explaining that its commits are no longer in the repository.
- Renaming and deleting an unavailable review behave exactly as they do for an available one.
- The next list refresh after the commits return shows the review as available again.

## Carrying a review guide

A review may hold one generated review guide: a summary, an ordered list of
file notes, and when it was generated. A review that has never had a guide
generated for it simply carries none — there is no guide to show until the
user generates one. Deleting a review removes any guide it carries along with
the rest of it. The guide's own contract — what generates it, what it
contains, and how it is presented — is specified in
[Review Guide](../ai/review-guide.md); this document is the source of truth
only for the fact that the guide travels with the review record, exactly like
its name, dates, and status.

## What the user can rely on

Reviews are durable. They survive quitting and relaunching the application: a review started in one session is present, with its name, dates, and status intact, in the next. They belong to the repository, not to a particular worktree — every worktree linked to the same repository sees the same set of reviews, and starting a review in one worktree makes it visible from the others.

Reviews live entirely in the application's own storage, never in the repository folder. Nothing about a review appears among the repository's files or in its version-control status; reviewing a repository never dirties its working tree or leaves a trace a teammate would see. Because reviews are keyed to a repository's location, moving the repository folder hides its reviews until the folder is back where the reviews expect it, at which point they reappear.

What is durable is deliberately narrow. Casual browsing of the graph, pending changes, and the arrangement of tabs and split panes the user built while reviewing are all ephemeral: resuming a review reopens its changeset fresh, at the change-set view, with no tabs or splits carried over from the last session. The review preserves *which* changeset the user was reviewing and its name, dates, and status — not the transient shape of the workspace around it.

**Guaranteed invariants**

- A review survives application relaunch with its name, dates, and status intact.
- A review belongs to the repository and is visible from every worktree linked to it, regardless of which worktree started it.
- A review leaves nothing in the repository folder or its version-control status.

**Edge cases**

- Moving the repository folder hides its reviews until the folder is restored to the location the reviews are keyed to; the reviews reappear when it is.
- Resuming a review reopens its changeset fresh — at the change-set view, with no tabs or split panes — even if the user had built up tabs and splits before leaving it.
- Casual browsing and pending changes are never captured by a review.
