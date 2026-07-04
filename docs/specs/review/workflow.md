# Review Workflow

This contract defines the review experience: opening a Git repository, navigating its commit graph, selecting commits to review, browsing the rollup change set, and inspecting individual file diffs.

## Opening a repository

The user opens a local Git repository through a folder picker. The application reads the repository's history and presents the commit graph for that repository. Recently opened repositories persist between application launches. On launch the application automatically reopens the most-recently-opened repository so the user returns straight to where they left off; the list of recent repositories is surfaced for re-picking only when no repository can be reopened. Only one repository is open per window in v1.

**Triggering conditions**

- The application launches with a most-recently-opened repository that can still be opened.
- The user picks a folder via the open-repository affordance on launch or from within an open window.
- The user activates a recently opened repository entry on launch.

**Observable outcomes**

- The window displays the commit graph of the chosen repository with its checked-out tip
  selected (see "Selecting commits to review").
- The window bar identifies the open repository by name.
- The chosen repository is added to (or moved to the top of) the recently opened list, and that ordering is retained for future launches.
- On launch, when a most-recently-opened repository can be opened, the window comes up showing that repository's commit graph without any further action.

**Edge cases**

- A chosen folder that is not a Git repository surfaces a clear error and the previous window state is preserved, including the window bar's repository name. If no repository was open, the window bar shows no repository name.
- A repository with zero commits opens to an empty graph with a message explaining there is nothing to review.
- A recently opened repository whose folder has been moved or deleted is shown as unavailable; activating it surfaces a clear error and offers to remove it from the list.
- On launch, a most-recently-opened repository whose folder has been moved or deleted does not interrupt startup: the application drops silently to the recent-repositories list with no error, and the entry is shown as unavailable. When the most recent entry is already marked unavailable, the application does not attempt to reopen it and starts on the recent-repositories list.

## Switching repositories from the window bar

With a repository open, the repository name in the window bar is an affordance: activating it
opens a switcher listing the Git repositories that sit alongside the open one in its parent
folder. The open repository is shown in the list and marked as the current one; selecting any
other repository opens it in place, exactly as if it had been opened from the folder picker.
The switcher always offers an "open repository" control that falls back to the folder picker
so any repository remains reachable. The switcher is available in both graph and changeset
mode, since the repository name is always shown. When a linked worktree is open, the switcher
lists and marks repositories relative to the repository's primary worktree rather than the
linked worktree's own location, matching the repository name the window bar already shows.

**Triggering conditions**

- The user activates the repository name in the window bar.
- The user selects another repository in the switcher.
- The user activates the switcher's open-repository control.
- The user dismisses the switcher by activating outside it.

**Observable outcomes**

- Activating the repository name opens a switcher listing the sibling repositories in the
  parent folder, ordered by folder name, with the open repository marked as current.
- Selecting another repository opens it: the window shows that repository's commit graph with
  its checked-out tip selected (see "Selecting commits to review"), any open changeset is
  closed, and the repository is moved to the top of the recently opened list.
- The open-repository control launches the folder picker.
- Dismissing the switcher by activating outside it leaves the open repository unchanged.
- Opening the switcher dismisses the diff-context popover, and vice versa; the two are never
  shown at once.

**Edge cases**

- A parent folder that contains no other Git repositories shows an explanatory message in
  place of the list; the open-repository control still works.
- A repository whose folder is at the filesystem root, or whose parent cannot be read, shows
  the same empty message.
- Selecting a sibling whose folder has been moved or deleted since the switcher opened
  surfaces a clear error through the normal open-failure flow and leaves the previous
  repository open.

## Switching worktrees from the window bar

With a repository open, the window bar shows the active worktree's name between the
repository name and the changeset identifier — "main" when the primary worktree is open,
otherwise the worktree folder's name. The worktree name is an affordance: activating it opens
a switcher listing every worktree registered for the repository, read from Git's own worktree
registry each time the switcher opens. The primary worktree is listed first as "main
worktree"; linked worktrees follow in registry order under their folder names. Each entry
shows its checked-out branch, short commit identifier, and an abbreviated path, and the
active worktree is marked. Selecting another worktree switches the review context to it,
exactly as if that folder had been opened from the folder picker. The switcher is available
in both graph and changeset mode and never offers to create, remove, or search worktrees.

The window bar and window title always identify the repository by its primary worktree's
folder name, even while a linked worktree is open. Worktree selection is session-only: the
recently opened list records the primary worktree, so relaunching the application always
returns to the primary worktree.

**Triggering conditions**

- The user activates the worktree name in the window bar.
- The user selects another worktree in the switcher.
- The user activates the entry for the already-active worktree.
- The user dismisses the switcher by activating outside it.

**Observable outcomes**

- Activating the worktree name opens the switcher and dismisses the repository switcher and
  the diff-context popover; opening either of those dismisses the worktree switcher. At most
  one of the three is ever shown.
- Selecting another worktree opens it: the window shows that worktree's commit graph with its
  checked-out tip selected (see "Selecting commits to review"), any open changeset is closed,
  pending changes reflect that worktree's working tree, and the repository's primary worktree
  is moved to the top of the recently opened list.
- Selecting the already-active worktree, or dismissing the switcher by activating outside it,
  closes the switcher and leaves the context unchanged.

**Edge cases**

- A repository with no linked worktrees still shows the worktree name and opens a switcher
  containing only "main worktree", marked as current.
- A registered worktree whose folder has been moved or deleted is omitted from the list.
- If the worktree registry cannot be read, the switcher shows only the active worktree and a
  clear error is surfaced through the normal notification flow.
- A worktree on a detached or unborn HEAD lists without a branch name.

## Quitting the application

The user can quit Greviewer with the standard quit keyboard shortcut.

**Triggering conditions**

- The user presses Cmd-Q while the Greviewer window is focused.

**Observable outcomes**

- The application begins its normal quit flow.

## Viewing the commit graph

The user sees a graphical history of the repository's commits with branch lanes and merge connectors. The history covers the checked-out commit, every local branch, every remote-tracking branch, and every tag, so work that has not been merged or pulled — and releases whose branches are gone — is visible alongside the checked-out history. Commits from all branches interleave in a single graph ordered newest-first, with no visual distinction between commits that are and are not part of the checked-out history. Branch and merge lines use smooth rounded bends where they turn between lanes, including the final turn into a branch commit. Each commit is presented as a single-row entry whose reading order is ref labels, graph, short identifier, summary line, author, and authored date. The ref labels — any branch names or tags (local branch, remote-tracking branch, or tag) that point at the commit — occupy a column at the left edge of every row that is exactly as wide as the widest set of labels currently visible — at most about a third of the panel — and gives its space back to the summary when no labels are visible. Labels that do not fit the column are clipped at its edge; hovering the column reveals the full set, unabridged, in a tooltip that also names each label's role — checked out, checked out in a linked worktree, remote, or tag. The graph is scrollable; older commits load progressively as the user scrolls. The currently checked-out tip is visually marked, but selection is independent of checkout state — reviewing never moves HEAD.

**Triggering conditions**

- A repository is open.

**Observable outcomes**

- Commits appear in graphical order with branch lanes and merge connectors.
- The graph's top-most item is always the repository's pending changes (see "Reviewing pending changes"), above every commit, whether or not the working tree has any uncommitted difference from HEAD.
- Commits reachable only from unmerged local branches, remote-tracking branches, or tags appear
  interleaved with the checked-out history; each such branch or tag renders as its own lane that
  ends at its tip.
- Each visible commit shows its short identifier, summary line, author, and authored date.
- Branch names are shown on the commits they point to; a remote-tracking branch shows its remote-qualified name and renders visually distinct from local branch labels.
- Tag names are shown on the commits they point to, in a color distinct from both branch labels and the checked-out marker, and carry a tag icon; on a commit carrying both, branch labels precede tag labels.
- A local branch, a remote-tracking branch, and a tag with the same name are never confusable; when more than one points at the same commit, every label appears.
- The currently checked-out tip carries a visual marker: the checked-out branch's own label renders in the checked-out accent with a checked-out icon in place of a separate HEAD label. When no visible label names the checked-out branch, a standalone HEAD label carries the marker instead.
- A local branch checked out in a linked worktree carries a worktree icon on its label.
- Ref labels render in a shared leading column sized to the widest visible label set, capped at roughly a third of the panel width; labels that overflow the cap are clipped at the column edge, and hovering the column reveals every label in full. The column is the same width on every row, and it yields its space to the commit summary when no labels are visible.

**Edge cases**

- Very large histories load progressively rather than blocking the UI.
- Histories with several active branches remain legible: the visible first-parent history of the checked-out commit anchors the left-most lane, active branch lanes keep a stable horizontal position until they end, branch lanes render as continuous vertical lines through and between rows without row separators or rounded branch/merge joins interrupting them, horizontal branch and merge runs appear at row boundaries so commits remain centered within their branch lanes, multiple visible edges may target the same future parent commit without deleting one another, sibling side branches with the same trunk parent keep strictly earlier authored commits on inner side lanes, keep later siblings on their outward lanes until they rejoin the shared parent, share the same trunk branch-off with horizontal extensions to outer lanes, new branch lanes use the nearest available lane to the right and do not reuse occupied lanes, multi-lane connectors stay continuous and horizontally aligned across intermediate lanes even when those lanes are occupied by other active branches, and branch labels do not obscure the graph.
- An unmerged branch's lane runs from its tip down to the commit where it diverged from
  visible history and ends there; when the tip is newer than the checked-out commit, the
  left-most lane is simply empty above the checked-out commit's row.
- A branch that shares no history with the others ends without joining any other lane.
- A repository whose checked-out branch has no commits yet still shows the history of its
  other local branches, remote-tracking branches, and tags.
- Detached-HEAD repositories render normally with no checked-out branch marker.
- An annotated tag labels the commit it was created on, exactly like a plain tag; the tag's
  message does not appear in the graph.
- A tag that points at something other than a commit does not appear anywhere.

## Navigating branches from the sidebar

Graph mode includes a sidebar beside the graph listing the repository's branches and tags. The list is split into a Local section, a Remote section, and a Tags section, in that order. Each section is introduced by a header that bears a distinguishing icon, the section's name, and a count of the refs the section contains; the header also acts as a collapse control for the whole section (see "Collapsing a section" below). The Local section holds the repository's local branches; the Remote section holds remote-tracking branches, grouped under one collapsible folder per remote so multiple remotes stay separate; the Tags section holds the repository's tags. Within any section, names containing `/` nest under collapsible folders (see "Nesting branches in sidebar folders" below). Remote branch entries render visually distinct from local entries, and tag entries carry a tag icon in place of the branch icon. The checked-out branch carries a visual marker. Activating a branch or tag focuses it in the graph: its tip commit becomes the selected commit and the graph scrolls so that commit is visible. The divider between the sidebar and the graph is draggable to resize the sidebar. The sidebar exists only in graph mode; review mode shows the file tree in its place.

**Triggering conditions**

- A repository is open and the window is in graph mode.
- The user activates a branch entry in the sidebar.
- The user activates a section header to collapse or expand the section.
- The user drags the divider between the sidebar and the graph.

**Observable outcomes**

- The sidebar lists every local branch under the Local section, every remote-tracking branch under its remote's folder in the Remote section, and every tag under the Tags section, alphabetically within each folder level, with the checked-out branch visually marked.
- Each section header shows a distinguishing icon, the section's name, and the number of refs the section contains.
- Adjacent sections are set off from each other by a single divider: it sits below the upper section's rows when that section is expanded, and is shared between two headers when they stack directly (the upper section collapsed), never doubling. A section at the top of the list relies on the sidebar's own border rather than adding one.
- A section with no refs does not appear; a repository with no remotes shows no Remote section, and one with no tags shows no Tags section.
- Activating a branch or tag selects its tip commit and scrolls the graph so the commit is visible.
- A branch or tag entry whose tip commit is the current selection is visually distinct, using the same selected treatment as the commit row.
- Dragging the divider resizes the sidebar.

**Edge cases**

- A repository with no branches or tags at all shows an explanatory message in place of the list.
- The remote's default-branch pointer (the remote's "HEAD") is not a branch and never appears.
- Detached-HEAD repositories list branches normally with no checked-out marker.
- Activating a branch or tag whose tip commit has not yet been loaded into the visible history loads older history until the tip appears, then selects and reveals it.
- Activating a branch or tag whose tip is already the selected commit keeps that selection; it does not clear it.
- When the list exceeds the sidebar's height, it scrolls; a scrollbar appears while the pointer is over the sidebar and stays out of the way otherwise.

## Collapsing a section

Activating a section header collapses or expands the whole section. Collapsing
is purely visual: the section's rows — folders, branches, and tags alike —
leave the sidebar, but graph visibility does not change, and refs hidden from
the graph stay hidden while the section is collapsed. The header reflects the
collapsed state, and the count it shows always reports every ref the section
contains, regardless of collapse state or whether individual refs are hidden
from the graph. Sections start expanded, and collapse state is not persisted:
opening a repository expands every section.

**Observable outcomes**

- Activating a section header removes that section's rows from the sidebar;
  activating it again restores them. Graph contents are unchanged either way.
- The count in a section header reports the total number of refs the
  section contains and does not change when the section is collapsed or when
  refs are hidden from the graph. The one exception is while a sidebar
  filter is active, when the count reports the number of matching refs
  (see "Filtering branches in the sidebar").
- Reopening a repository expands every section.

## Nesting branches in sidebar folders

Branch and tag names that contain `/` nest in the sidebar: every name segment
except the last becomes a collapsible folder, so `features/some-feature`
appears as a `some-feature` row inside a `features` folder, and
`team/alice/feature-x` nests two folders deep. A folder exists even when it
holds a single ref — grouping depends only on the ref's own name, so rows do
not reorganize as siblings appear or disappear. Within a level, folders list
before refs, each alphabetically. A nested row shows only its final name
segment, indented under its folders; everywhere else — graph labels, hiding,
focusing — the ref keeps its full name.

Activating a folder row collapses or expands it. Collapsing is purely
visual: descendant rows leave the sidebar, but graph visibility does not
change, and branches hidden from the graph stay hidden while collapsed.
Folders start expanded, and collapse state is not persisted: opening a
repository expands every folder.

Each folder row carries a visibility toggle with the same hover-reveal
behavior as branch rows. Activating it hides every branch under the folder
when at least one is visible, and shows them all otherwise. The checked-out
branch cannot be hidden and is skipped: hiding a folder that contains it
hides everything else inside, and the folder reads as fully hidden once all
its other branches are. A fully hidden folder renders muted with its toggle
always visible; a folder with a mix of hidden and visible branches keeps its
normal color but also keeps its toggle visible.

**Observable outcomes**

- A branch named with `/` renders inside one folder per leading segment,
  showing only its final segment, indented by depth.
- A folder appears even for a single nested branch; multi-segment names nest
  multi-level.
- Folders precede branches at each level; both sort alphabetically.
- Activating a folder row removes its descendant rows from the sidebar;
  activating it again restores them. Graph contents are unchanged either way.
- Activating a folder's visibility toggle hides all its branches from the
  graph, skipping the checked-out branch, or shows them all when none are
  visible. A selection that becomes invisible resets to the checked-out tip,
  as with single-branch hiding.
- A fully hidden folder renders muted with its toggle always shown; a
  partially hidden folder keeps its toggle shown without muting.
- Reopening a repository expands all folders and shows all branches.

## Filtering branches in the sidebar

The branch sidebar carries an always-visible search field pinned above the
section list, showing a search icon, the placeholder "Search branches…", and,
whenever it holds text, a control to clear it. Typing filters the branch tree
in place. The filter is purely a view concern: it changes only what the
sidebar shows. The commit graph, the current selection, and every other
surface are untouched by the filter itself — only the sidebar's ordinary
actions (activating a branch, hiding a branch, collapsing a section or folder)
affect the rest of the app, and they behave identically whether or not a
filter is active.

Matching is a case-insensitive subsequence ("fuzzy") test against each ref's
full display path — a local branch's or tag's own name, and a remote branch's
name led by its remote (for example `origin/feature/login`). While a filter is
active, only sections and folders that contain at least one matching ref
appear, and they render expanded regardless of any saved collapse state;
clearing the query restores that collapse state. Matched characters are
highlighted wherever they fall, across folder rows and the leaf row.
Section counts report the number of matching refs while filtering. The
checked-out branch is filtered like any other ref and is not kept visible
when it does not match.

**Triggering conditions**

- A repository is open and the window is in graph mode.
- The user types in, or clears, the sidebar search field.

**Observable outcomes**

- With an empty query the sidebar is unchanged: saved collapse state is
  honored and section counts report every branch each section contains.
- With a non-empty query, only matching branches and their ancestor folders
  and sections appear; matched characters are highlighted on both folder and
  leaf rows.
- While filtering, section and folder rows render expanded, and each section
  count reports the number of matching branches it contains.
- Clearing the query restores the pre-filter view, including any collapsed
  sections and folders.
- Applying or changing a filter never alters which commits the graph shows or
  which commit is selected.

**Edge cases**

- A non-empty query that matches no branch shows a "No matching branches"
  message in place of the tree.
- Pressing Escape while the field is focused clears the query.
- The query is not persisted: opening a repository clears the field.

## Hiding branches from the graph

Every branch and tag except the checked-out branch can be toggled off from
the sidebar. A hidden ref's name no longer appears as a label in the graph,
and commits reachable only from hidden refs are removed: the graph re-flows
as if those commits did not exist. Commits a hidden ref shares with any
visible ref (or with the checked-out branch) remain.

The toggle control on a visible branch is revealed when the pointer is over
its row; a hidden branch's control is always visible and its name renders
muted. Activating a hidden branch's row does not focus it; the branch must be
shown again first. The checked-out branch offers no toggle. If hiding a
branch removes the selected commit — or any commit in a selected range — the
selection resets to the checked-out tip, which can never be hidden.
Visibility choices are not persisted: opening a repository
shows every branch and tag. Remote-tracking branches and tags hide and show exactly like local branches, and both are visible by default. A local branch, a remote-tracking branch, and a tag that share a display name are independent: hiding, showing, or collapsing one never affects the others.

**Observable outcomes**

- A non-checked-out branch or tag row reveals a visibility toggle on hover;
  the checked-out branch row never shows one.
- Toggling a ref off removes its labels and its exclusive commits from
  the graph; toggling it back on restores them.
- A hidden ref's row is muted, keeps its toggle visible, and does not
  focus its tip when activated.
- Hiding a ref that makes the current selection invisible resets the
  selection to the checked-out tip.
- Reopening a repository resets all branches and tags to visible.

## Selecting commits to review

The user stages a review by selecting a single commit, a contiguous sequential range, or a two-commit comparison from the graph. Selection is tentative: no review activity begins until the user explicitly opens the changeset (described below). Clicking a commit selects it. Shift-clicking a second commit extends the selection to a range, provided the two commits lie on a single ancestry path (one is an ancestor of the other). The selection is the inclusive set of commits between the two endpoints along that path. Double-clicking bypasses tentative selection entirely; it is specified under "Opening the changeset" below.

A comparison stages a merge preview between two commits that need not share a linear ancestry — for example, a feature branch tip against the trunk tip. The current selection's anchor commit is the merge source: clicking another commit while holding the platform's primary modifier previews merging that source into the clicked commit, provided the two share any common history. To preview merging a feature branch into the trunk, the user selects the feature tip, then modifier-clicks the trunk tip. The comparison is directional — it shows what the merge would introduce into the clicked destination — and the selection summary states that direction, with a swap affordance beside it that reverses it. Modifier-clicking a further commit re-aims the preview at a new destination while the source stays anchored; modifier-clicking either commit of the pending comparison leaves it unchanged. A plain click replaces the comparison with a single-commit selection, exactly as it replaces a range.

The graph always carries a selection: opening a repository selects its checked-out tip, and any change that would otherwise leave nothing selected returns to that default instead. Clicking the selected commit keeps it selected. There is no affordance for clearing the selection — the user moves it by selecting something else. The only state with no selection is a graph with no commits at all.

**Triggering conditions**

- A repository opens (its checked-out tip becomes the selection).
- The user clicks a commit in the graph.
- The user shift-clicks a second commit while a single-commit selection is active.
- The user modifier-clicks a commit while any selection is active.
- The user activates the swap affordance while a comparison is staged.

**Observable outcomes**

- The selected commit or range is visually distinct in the graph; a comparison renders both of its commits with the same selected treatment.
- While a selection is active — which, because a selection always exists, is whenever the graph shows any commits — the graph shows how many commits the selection covers, alongside the open-changeset affordance. A staged comparison shows the merge-preview direction in place of a commit count, plus the swap affordance.
- Clicking the already-selected commit leaves it selected.
- Activating the swap affordance reverses the comparison's base and target, and the stated direction updates to match.

**Edge cases**

- If the two endpoints of a shift-click do not share a linear ancestry — they lie on diverged branches — the second click is rejected with a message explaining why, and the original selection is preserved.
- If the two commits of a comparison share no common history at all, the modifier-click is rejected with a message explaining why, and the original selection is preserved.
- Modifier-clicking with a range selection active starts the comparison from the range's first-selected endpoint.
- Merge commits inside a selected range are included in the range and contribute to the rollup.
- When there is no checked-out tip to select — the checked-out branch has no commits yet, or no branch is checked out at all — the newest visible commit is selected in its place.
- Hiding a branch that removes either commit of a comparison from the graph resets the selection to the checked-out tip, as with any selection (see "Hiding branches from the graph").
- A repository with no commits at all has no selection, and the selection summary and its affordance are absent.
- Gestures that would extend a range or stage a comparison to or from the pending-changes row are handled separately; see "Reviewing pending changes" below.

## Reviewing pending changes

Above every commit, the graph always carries one more item: the repository's pending changes, the work that has not yet been committed. It sits at the top of the graph with an edge connecting it down to the checked-out commit, so it reads as the newest revision in the history rather than a separate list. It renders with a distinct hollow dot in place of a commit's filled one, the text "Pending changes", and either a count of changed files with their added and removed line totals, or a muted "No pending changes" when the working tree matches HEAD exactly.

Pending changes behave like any other item for plain selection: clicking the row selects it, and the selection summary reads "Pending changes selected". Enter, the open-changeset affordance, and double-clicking all open its changeset, the same as for a commit. What pending changes cannot do is join a range or a comparison — a shift-click or a modifier-click that would pair the pending row with a commit, in either direction, is rejected with a message explaining that pending changes can only be reviewed on their own, and the selection beforehand is left exactly as it was.

Opening the pending changeset shows every uncommitted difference against the checked-out commit: staged changes, unstaged changes, and untracked files, combined into one changeset where each file appears once for its net change. Untracked files appear as added, with their on-disk content as the new side. Files or folders matched by `.gitignore` are excluded, and submodules are skipped, exactly as they are for any other diff. The window bar reads `{repository} / pending`, and the popover opened from it is headed "Reviewing pending changes" with the changeset's file count, line totals, and kind breakdown — it carries no commit list and no identifier line, since there is no fixed set of commits to enumerate.

The file list shown when the changeset opens, and the summary shown in the graph row, are both a snapshot taken at that moment — open, activate the window, or close a changeset elsewhere in the app, and the list is recomputed. An individual file's diff content, though, is only read the first time that file is viewed: once read, it is held fixed for as long as the changeset stays open, even if the file changes on disk again in the meantime, so a reviewer's read never shifts underfoot mid-review.

**Triggering conditions**

- A repository is open — the pending row is always present, alongside every commit.
- The user clicks the pending row, presses enter while it is selected, activates the open-changeset affordance, or double-clicks the row.
- The user attempts a shift-click or modifier-click that would pair the pending row with a commit.
- The repository is opened, the window is activated, or an open changeset is closed.

**Observable outcomes**

- The pending row appears above every commit, connected to the checked-out commit by an edge, showing a hollow dot, "Pending changes", and either the file/line summary or "No pending changes".
- Clicking the row selects it and shows "Pending changes selected" in the selection summary.
- Enter, the open-changeset affordance, and double-clicking open the pending changeset.
- Opening it shows the combined staged, unstaged, and untracked difference against HEAD, one entry per file; the window bar reads `{repository} / pending`; the popover header reads "Reviewing pending changes" with counts and a kind breakdown, and no commit list or identifier line.
- The row's summary and the changeset's file list refresh on repository open, on the window regaining focus, and when any open changeset closes.

**Edge cases**

- A shift-click or modifier-click that would pair the pending row with a commit, in either direction, is rejected with a message explaining that pending changes can only be reviewed on their own; the prior selection is preserved.
- An empty repository still shows the pending row on its own; the "no commits to review" message appears alongside it only when the working tree is also clean.
- In a detached-HEAD repository, pending changes diff against the checked-out commit, whatever it is.
- Hiding branches never removes the pending row; it is not a branch and is unaffected by branch visibility.
- Binary files in the pending changeset show the same explanatory placeholder as binary files anywhere else.
- A file viewed while the changeset is open keeps showing what it read at that first view, even if the file changes on disk again before the changeset is closed.

## Opening the changeset

With a valid selection in place, the user opens the changeset to begin reviewing. Opening transitions the window from graph mode to review mode, where the file tree and diff viewer are the primary content. The graph remains the basis for the open selection but is not the active surface. The user closes the open changeset to return to graph mode; the prior selection is preserved so it can be adjusted and re-opened without re-selecting from scratch. Modifying the selection while a changeset is open is not supported in v1 — the user closes the changeset, adjusts, and reopens. Double-clicking a commit in the graph is a one-gesture shortcut that combines selection and opening: it selects exactly that commit — replacing any prior selection, including a range or the commit's own selected state — and immediately opens its changeset.

**Triggering conditions**

- The user activates the open-changeset affordance while a valid selection is active.
- The user presses enter while a valid selection is active; this is equivalent to activating the affordance.
- The user double-clicks a commit in the graph.
- The user closes the open changeset.

**Observable outcomes**

- Activating the open-changeset affordance transitions the window into review mode and renders the change set for the current selection.
- The open-changeset affordance is unavailable when no selection is active or when the selection is not a valid contiguous range.
- Opening a comparison renders the merge preview's change set: every file that differs between the two commits' common ancestor and the target commit — exactly what merging the target into the base would introduce. Changes present only on the base side do not appear.
- Double-clicking a commit transitions the window into review mode showing that single commit's changeset, regardless of the prior selection.
- Closing the changeset returns the window to graph mode with the prior selection preserved.
- Review mode does not move HEAD or modify the repository.

**Edge cases**

- Opening a changeset whose net effect is no change still transitions to review mode; the change set view shows the empty-state message described in the next section.
- Opening a comparison whose target is already contained in the base yields an empty change set with the same empty-state message: there is nothing the merge would introduce.
- Double-clicking a commit inside a selected range opens that single commit's changeset, not the range's.
- Pressing enter while a changeset is already open has no effect: the open changeset and any tab or split layout the user has built are unchanged.
- Pressing enter while typing in the branch filter does not open a changeset.

## Seeing diff context in the window bar

While a changeset is open, the window bar shows the open diff's context next to the
repository name, in the form `{repository} / {commit identifier}`. For a single-commit
changeset and for a range alike, the commit identifier is the newest commit's short
identifier followed by the number of commits reviewed ("1 commit", "3 commits"); for a
comparison it is the base and target short identifiers joined in git's three-dot form. The
identifier is an affordance: activating it opens a popover describing the open changeset and
offering a control to close it. Graph mode shows only the repository name, with no context
identifier.

**Triggering conditions**

- The user activates the context identifier in the window bar while a changeset is open.
- The user activates the close control inside the popover, or dismisses the popover by
  activating outside it.

**Observable outcomes**

- In changeset mode the window bar shows the repository name and the changeset's commit
  identifier; in graph mode it shows only the repository name.
- Activating the identifier opens a popover whose header shows, for a single commit or a
  range, "Reviewing" plus the commit count, above an identifier line: the oldest and newest
  short identifiers for a range, or the commit's short identifier alone for a single commit.
  For a comparison, the header shows the merge-preview direction and the short identifier of
  the two commits' common ancestor.
- The popover shows the number of changed files and the total added and removed line counts
  for the changeset.
- The popover shows a breakdown of the changed files by how they changed (added, modified,
  deleted, renamed), listing only the kinds that are present.
- The popover lists the reviewed commits, newest first, each with its short identifier and
  summary; the list scrolls when it exceeds the popover's height. A single-commit changeset
  lists its one commit.
- For a comparison, the popover lists the commits the merge would introduce — those reachable
  from the target but not the base — newest first, in the same form, including when only one
  commit would be introduced.
- The popover offers a control that closes the changeset, returning the window to graph mode
  with the prior selection preserved.
- Dismissing the popover by activating outside it leaves the changeset open.

**Edge cases**

- A changeset whose net effect is no change shows zero changed files, `+0 / −0` lines, and no
  kind breakdown in the popover; the close control still returns to graph mode.
- A commit that is not yet in the loaded history window appears in the commit list with its
  short identifier and no summary.

## Reviewing the change set

With a changeset open, the user sees the rollup change set: a file tree containing every file whose content differs between the state immediately before the oldest selected commit and the state at the newest selected commit. For a comparison, the two states are instead the commits' common ancestor and the target commit, and everywhere the changeset otherwise refers to the newest selected commit, a comparison means its target. The tree shows each file's path and an indicator of how it changed (added, modified, deleted, renamed). Selecting a file in the tree opens that file's diff (described below).

**Triggering conditions**

- A changeset is open.

**Observable outcomes**

- The change set lists every net-changed file across the selection.
- The tree is headed by a row naming the repository — the same name the title bar shows — and every file and folder nests beneath it. The header stays in place while the tree scrolls and is not itself selectable or collapsible.
- Paths are grouped into a tree with open and closed folder icons and vertical nesting guides for files under folders.
- Each changed file entry shows a compact change marker: plus for added, dot for modified, horizontal bar for deleted, and rename marker for renamed.
- Deleted file entries render their filename struck through.
- Changed entries show added and removed line counts so the user can estimate review size before opening a file.
- When the tree's contents exceed the visible area, the user can scroll it both vertically and horizontally; long or deeply nested paths are shown in full and reached by scrolling rather than being abbreviated.
- Only the file's path scrolls horizontally; each entry's change details (its added and removed line counts) stay pinned in view at the trailing edge so the user can always read them, even while scrolling a long path. Vertical scrolling moves a row's path and its change details together.
- A purely vertical scroll gesture never pans the paths horizontally, and a purely horizontal one never scrolls the rows.
- The highlighted row's background spans the full width of the tree. Widening the tree past its longest entry stretches the highlight to the new edge rather than leaving a gap; narrowing it below the longest entry keeps the highlight filling the visible width as the path scrolls.
- A scrollbar appears while the user's pointer is over the tree and lets the user drag to scroll; it stays out of the way otherwise. An axis with nothing off-screen shows no scrollbar.
- Selecting a file opens it for inspection in a tab above the diff area (see "Holding files open in tabs").

**Edge cases**

- A selection whose net effect is no change (e.g., a commit and its revert within the same range) shows an empty change set with a message explaining the net result.
- Binary files appear in the tree with an indicator that no textual diff is available; selecting one opens an explanatory placeholder.
- Renamed files appear once with their new path and an indicator of the rename; the diff displays the rename's content delta.

## Seeing all files for context

The file tree's repository header row carries icon-only controls that stay pinned as the tree scrolls. A single show-all-files toggle switches between the change set and the all-files view; it reads as active while the all-files view is showing. Two further controls collapse every folder or expand every folder in one action. Each control names itself through a hover tooltip.

The show-all-files toggle reveals every file in the repository at the newest selected commit, not just the files in the change set. Files that are part of the change set remain marked so they are distinguishable from unchanged files. Selecting an unchanged file opens it for read-only viewing rather than as a diff.

**Triggering conditions**

- The user activates the show-all-files toggle while a changeset is open.
- The user activates collapse-all or expand-all in either the change-set or the all-files view.

**Observable outcomes**

- The all-files view shows every file at the newest selected commit.
- Folders that do not lead to a changed file are collapsed by default, so the change set stays visible without scrolling past unrelated files. Folders on the path to a changed file remain expanded.
- Files in the change set retain their change indicator; unchanged files render without one.
- Selecting an unchanged file opens it as a read-only view of its contents at the newest selected commit.
- Collapse-all closes every folder in the tree; expand-all opens every folder, including folders that were collapsed by default.

**Edge cases**

- The user can manually expand a collapsed folder or collapse an expanded one; manual toggles override the default and persist while the changeset stays open, including across switches between the change-set and all-files views.
- Collapse-all and expand-all establish a new baseline; subsequent manual folder toggles adjust from there.

## Holding files open in tabs

Opened files live in a row of tabs above each pane's diff area. A single click on a file opens it in the active pane's preview tab — a holding slot that subsequent single clicks reuse, so casual browsing never piles up tabs. At most one preview tab exists per pane, and its title renders in italics to signal that the next single click will replace it. Opening a file deliberately — double-clicking it in the tree, or double-clicking the preview tab itself — pins the tab; a pinned tab keeps its file until the user closes it.

**Triggering conditions**

- The user single-clicks or double-clicks a file row while a changeset is open, or clicks, double-clicks, or middle-clicks a tab.

**Observable outcomes**

- Single-clicking a file shows its diff in the active pane's preview tab, creating that tab when none exists and otherwise replacing its content in place; the tab keeps its position in the row.
- Double-clicking a file pins its tab. Double-clicking the preview tab pins it without changing its content.
- Opening a file that is already open in the pane activates the existing tab; a file is never open twice in one pane, and opening a pinned file never demotes it to preview.
- A tab's title is the file's name, tinted with the file's change-kind color; files opened from the all-files view that are not part of the change set use the default text color. When two open tabs share a file name, each also shows its parent folder name.
- Exactly one tab is active per pane; it is visually distinct from the other tabs (raised background and an accent line, with inactive tabs dimmed), and the pane always shows its active tab's file. Clicking a tab activates it.
- Every tab offers a close control, revealed on hover and always present on the active tab; middle-clicking a tab also closes it. Closing the active tab activates its right neighbor, or its left neighbor at the end of the row.
- When more tabs are open than fit the width, the tab row scrolls horizontally and activating a tab brings it into view.
- Closing a pane's last tab closes the pane itself when other panes remain (see "Splitting the diff area into panes"). In the only remaining pane, closing the last tab returns it to the select-a-file placeholder, and the tab row disappears along with its last tab.
- Leaving the changeset — closing it or opening a different one — closes all tabs in every pane.
- The file tree's highlighted row follows the user's clicks in the tree; activating a different tab does not move the tree highlight.

**Edge cases**

- Closing the preview tab removes it; the next single click opens a fresh preview tab at the end of the row.
- Re-opening the changeset after leaving it starts with no tabs open, even if files were open when it was left.
- Switching between the change-set and all-files views does not close tabs; a tab for a file outside the change set keeps showing its read-only content in either view. Only leaving the changeset closes tabs.

## Splitting the diff area into panes

The diff area can hold several panes at once, arranged by vertical and horizontal splits, so two files — or two parts of one review — sit side by side. Each pane has its own tab row and its own diff. Exactly one pane is active at a time: it is where file-tree clicks open tabs, and its tab row renders at full strength while the other panes' rows are dimmed.

**Triggering conditions**

- The user clicks a split control in a pane's tab row, presses Cmd+K followed by an arrow key, clicks inside a pane, drags a divider, or closes a pane with Cmd+K W.

**Observable outcomes**

- A pane shows its tab row only while it holds at least one tab; the row carries split-right and split-down controls at its right edge. A pane with no tabs shows only the select-a-file placeholder, so splitting an empty pane is a keyboard-only action.
- Splitting inserts a new pane next to the source pane — after it for right and down, before it for left and up. The new pane becomes active, takes half the source pane's space, and opens holding the same file the source pane was showing, with the same preview or pinned state; splitting a pane that shows nothing yields an empty pane with the placeholder. Splitting along an existing row or column of panes adds a sibling rather than nesting.
- Clicking anywhere within a pane — its tab row or its content — makes it the active pane.
- Single- and double-clicks in the file tree open files in the active pane only. A file may be open in several panes at once, but never twice in the same pane.
- Cmd+W closes the active pane's active tab. Ctrl+Tab and Ctrl+Shift+Tab cycle through the active pane's tabs, wrapping at the ends. Cmd+K then an arrow key splits the active pane in that direction. Cmd+K W closes the active pane.
- Dividers between panes can be dragged to trade space between the neighbors they separate; a pane never shrinks below a tenth of its row or column.
- Closing a pane's last tab — by close control, middle-click, or Cmd+W — closes the pane itself, exactly as if it had been closed explicitly. The last remaining pane is the exception: it stays and shows the placeholder.
- Closing a pane removes it and returns its space to its siblings; the pane taking the closed slot's place becomes active, and closing an inactive pane leaves the active pane untouched.
- The last remaining pane cannot be closed.
- The split arrangement lives and dies with the changeset: opening a changeset always starts with a single pane, and splits made while reviewing are discarded on leaving it.

**Edge cases**

- The workspace keyboard shortcuts do nothing while no changeset is open.
- Closing a pane that leaves a single child of a split collapses that level of the layout entirely.

## Rearranging tabs by dragging

Tabs answer to the mouse: a reviewer can drag one along its own row to reorder, drop it on another pane's tab row to move it, or drop it on the edge of a pane's content to carve out a new split — the same gestures Zed and VS Code train.

**Triggering conditions**

- The user drags a tab and drops it on a tab row, on another tab, or on the edge zone of a pane's content area.

**Observable outcomes**

- While a tab is dragged, a floating preview of the tab follows the cursor.
- Dropping a tab on another tab inserts it at that position; dropping it on empty tab-row space appends it at the end. An insertion indicator marks the target while hovering. Reordering within a pane keeps the tab's preview or pinned status.
- Dropping a tab on a different pane's tab row moves it there. The moved tab arrives pinned — even if it was the preview tab — and becomes the active tab of the now-active target pane.
- If the target pane already holds a tab for the same file, the drop merges: the existing tab activates and the dragged tab closes rather than duplicating.
- Dragging a tab over the left, right, top, or bottom band of a pane's content area highlights the corresponding half of the pane; dropping there splits that pane in that direction, and the dragged tab becomes the new pane's only, pinned tab.
- A pane with no tabs has no tab row and no edge zones; dropping a tab anywhere in it moves the tab there.
- Dragging the last tab out of a pane closes that pane; the layout collapses and returns its space to siblings.

**Edge cases**

- Dropping a tab back where it started changes nothing.
- Dragging a pane's only tab to that same pane's edge zone moves the tab into the new half; no empty pane is left behind.
- Releasing a drag outside any drop target leaves every tab where it was.

## Inspecting a file's diff

Opening a file in the change set presents its diff. The diff fills the pane edge to edge below the tab bar; the tab itself names the file and colors it by change kind, so the pane adds no header of its own. For files that exist on both sides of the selection (modified or renamed files), the diff is shown side-by-side: the "before" state on the left, the "after" state on the right, with corresponding lines aligned and the two sides separated by a thin divider. The user can scroll both sides; the alignment is preserved as they scroll. Line numbers are shown for each side. Lines longer than the pane extend past its edge; the user scrolls the diff horizontally to read them, and both sides of a side-by-side diff pan together. The line numbers and change accents stay in place at the left edge while the code pans beneath them. While the pointer is over the pane, a horizontal scrollbar appears along the bottom of each side whose content overflows; dragging it pans the diff.

Code is syntax-highlighted when the file's type is recognized; unrecognized types render as plain text. Changed lines read as color blocks: removed lines carry a red row tint and a red accent bar at the row's left edge, added lines the same in green. Within a modified line pair, the specific tokens that changed carry a stronger tint than the rest of the line, except when the pair differs almost entirely — a near-total rewrite reads as a whole-line change. Alignment gaps, where one side has no counterpart lines, render as a hatched region rather than blank space.

For files that exist on only one side of the selection — added or deleted files — the diff is shown full-width as a single pane. An added file shows its post-add content full-width; a deleted file shows its pre-delete content full-width. The full-width treatment exists because an empty pane wastes screen space without conveying additional information.

**Triggering conditions**

- The user selects a changed file in the change set.

**Observable outcomes**

- The diff content starts directly below the tab bar with no inner header, padding, or border.
- Modified and renamed files render as a side-by-side diff with aligned, scroll-synchronized panes separated by a thin divider.
- Added files render full-width showing the post-add content.
- Deleted files render full-width showing the pre-delete content.
- Removed lines show a red background tint and a red left-edge accent bar; added lines show the same in green.
- A changed line's tint and an alignment gap's hatch span the full width of their side of the diff, from the accent bar to the pane's edge, rather than stopping at the code text. Widening the pane past its longest line stretches the fill to the new edge instead of leaving a gap.
- Within a modified line pair, changed tokens carry a stronger background tint, unless the lines differ almost entirely.
- Rows with no counterpart line on the other side render as a hatched region. A gap spanning several lines reads as one continuous hatched block: the diagonal stripes flow across it unbroken rather than restarting at each line.
- Files with a recognized type render syntax-highlighted; unrecognized types render as plain text.
- Line numbers appear for each side of a side-by-side diff and for the single side of a full-width view.
- Code wider than the pane is reachable by horizontal scrolling: trackpad panning, shift+wheel, and dragging the horizontal scrollbar all pan the code region.
- Both sides of a side-by-side diff pan horizontally in lockstep.
- The accent bar and line-number gutter stay fixed at the pane's left edge while the code pans; panned code slides under the gutter's edge.
- A horizontal scrollbar overlays the bottom of a side only while the pointer is over the pane and that diff's content overflows horizontally.
- A purely vertical scroll gesture never pans the diff horizontally, and a purely horizontal one never scrolls the rows.
- Opening a file shows its diff unpanned; stepping between change blocks preserves the current pan.

**Edge cases**

- Renamed files diff old-path content against new-path content; the file's pre-rename path is surfaced so the user can see what was renamed.
- Binary files render an explanatory placeholder rather than attempting a textual diff.

## Selecting text in a diff

A file's diff behaves like an editor, not a static page: the user can place a caret, select text, and copy it out. Clicking places a caret at the character position under the pointer; a caret is a thin blinking bar, and its line carries a subtle full-width tint so its position reads at a glance even on a long line. Dragging from that point selects a run of text, and the fill of that selection takes over as the position signal — the caret-line tint is suppressed while a selection is active. Clicking anywhere in the diff, code or gutter, gives that pane keyboard focus, so selection and typing-driven motion pick up from wherever the user last clicked.

In a side-by-side diff, a selection lives on one side at a time. Placing the caret or starting a drag on the other side moves the selection there; a selection is never split across both sides, and a drag never crosses the divider between them. Alignment gaps — rows with no counterpart line on the other side — take no part in selection: they cannot hold the caret, a drag contributes nothing while passing through them, and copying skips them.

Beyond a plain click-and-drag, the diff recognizes the gestures reviewers expect from a text editor. Shift-clicking extends the selection from the existing anchor to the clicked point. Double-clicking selects the word under the pointer, and dragging afterward extends word by word; triple-clicking selects the whole line, and dragging afterward extends line by line. In both cases the selection never splits the word or line where the gesture began — a word-wise or line-wise drag only grows in whole words or whole lines. Clicking a line number selects that entire line, and dragging in the gutter extends the selection line by line regardless of where within each line the pointer sits. Dragging the pointer to the edge of the pane auto-scrolls the diff in the direction of the drag, on whichever axis or axes the pointer is pressed against, so a selection can extend beyond what is currently visible.

Once a pane is focused, the full range of keyboard motion is available: moving by character or by word, jumping to the start or end of a line, jumping to the start or end of the document, and extending any of these motions into a selection. A select-all action selects the entire side the caret is on. Escape collapses an active selection back down to a caret at its current position, without moving that position. Before the user has clicked anywhere in a diff there is no caret, and every selection-related keyboard action is a no-op until a click establishes one. Any motion that moves the caret scrolls it into view, so keyboard navigation never leaves the caret off-screen.

Copying a selection places exactly the selected characters on the clipboard — the selected lines joined by newlines, gap rows skipped, with no diff markers or line numbers mixed in. Copying with only a caret placed, and no range selected, copies nothing.

Selection belongs to the open tab it was made in. It survives switching away from that tab and back, and is discarded when the tab closes, when its preview content is replaced by opening a different file into the same preview slot, or when the changeset closes. The same file open in two different panes holds two independent selections. Only the focused pane shows a blinking caret; every other pane that holds a selection keeps it visible but dimmed, with no caret shown. Moving between change blocks and scrolling the diff by hand move the view only — the caret and selection never move because of them, and a caret that scrolls off-screen this way is normal, not an error.

Read-only views of unchanged files, and full-width diffs for added or deleted files, support the same selection behavior on their single side. Binary placeholders have no textual content and support no selection.

**Triggering conditions**

- The user clicks in a diff's code or gutter.
- The user drags the pointer after pressing down in a diff, including dragging to the pane's edge.
- The user shift-clicks, double-clicks, or triple-clicks in a diff, or clicks a line number.
- The user presses a caret-motion, selection-extension, select-all, or Escape key while a diff pane is focused.
- The user copies while a diff pane holds a selection or caret.
- The user switches tabs, closes a tab, replaces a preview tab's content, or closes the changeset.
- The user moves focus between panes.

**Observable outcomes**

- Clicking places a blinking caret at the clicked character position and focuses that pane; the caret's line shows a subtle full-width tint.
- Dragging selects a run of text; the selection fill replaces the caret-line tint while the selection exists.
- Shift-click extends the selection from the current anchor to the clicked point.
- Double-click selects a word and extends word-wise on further dragging; triple-click selects a line and extends line-wise on further dragging; a line-number click selects the whole line and gutter dragging extends line-wise. None of these ever split the word or line the gesture started on.
- Dragging to the pane's edge auto-scrolls the diff on the axis or axes the drag is pressed against.
- Starting a selection or caret on one side of a side-by-side diff and then clicking or dragging on the other side moves the selection to that side; a selection never spans both sides at once.
- With a diff pane focused, character, word, line-start/end, and document-start/end motions move the caret; adding shift extends the selection instead of just moving the caret; select-all selects the whole current side; Escape collapses the selection to a caret without moving it.
- Any caret motion scrolls the caret into view.
- Copying with an active selection places exactly the selected text on the clipboard, lines joined by newlines, with gap rows skipped and no diff markers or line numbers included. Copying with only a caret placed does nothing.
- Selection is preserved across switching away from and back to a tab.
- Only the focused pane's caret blinks; other panes holding a selection show it dimmed with no caret.
- Unchanged-file views and single-side added/deleted diffs support the same behavior on their one side.

**Edge cases**

- Before any click has been made in a diff, there is no caret, and every keyboard selection action is a no-op.
- Alignment-gap rows never receive the caret, contribute nothing to a drag that passes through them, and are skipped when copying.
- Binary placeholders have no caret and no selection.
- Closing a tab, replacing a preview tab's content with a different file, or closing the changeset discards that tab's selection. The same file open in two panes keeps two independent selections.
- Stepping between change blocks and scrolling the diff by hand move the view only; the caret and any selection stay exactly where they were, even if that leaves the caret off-screen.

## Navigating change blocks

A file diff groups its changes into blocks: runs of changed lines, with runs
separated by only a few unchanged lines joined into one block so a single edit
does not fragment into many stops. When a diff is first opened it scrolls
straight to its first change block, so the reviewer lands on the change rather
than the top of the file. While viewing a diff that has at least one change
block, the user sees a navigation control that reports the current position as
`Change N of M` and offers a step-to-previous and a step-to-next affordance.
Keyboard shortcuts step to the next and previous block as well. The control and
the shortcuts share one behavior.

Stepping is relative to the top of the view: the next step goes to the first
block that begins below the current top, and the previous step to the last
block that begins above it. A block scrolled off the top of the view is
therefore stepped to rather than skipped. Stepping moves the diff so the target
block sits near the top, with a little of the preceding context kept visible.
Navigation wraps: stepping past the last block returns to the first, and
stepping back from the first goes to the last. The reported position always
reflects what is on screen — scrolling the diff by hand updates the counter to
the block at the top of the view, so the next step is always relative to what
the user is currently looking at.

**Triggering conditions**

- A diff is opened, which scrolls it to its first change block.
- The user activates the previous- or next-block affordance in the navigation
  control while viewing a diff with at least one change block.
- The user presses the next- or previous-block keyboard shortcut while a
  changeset is open.
- The user scrolls the diff by hand.

**Observable outcomes**

- Opening a diff scrolls it to its first change block.
- A diff with at least one change block shows a navigation control reporting
  `Change N of M` with previous- and next-block affordances.
- Stepping to a block scrolls the diff so that block sits near the top, keeping
  a little preceding context in view, and updates the reported position.
- A block that begins below the current top is the next step's destination even
  when the counter already reports it, so no unseen change is skipped.
- Navigation wraps at both ends: next from the last block goes to the first, and
  previous from the first goes to the last.
- Scrolling the diff by hand updates the reported position to the block at the
  top of the view.
- Added and deleted files, shown full-width, count their changes as blocks and
  navigate the same way.

**Edge cases**

- A diff with no change blocks — a binary file, or a change whose net effect
  leaves no differing lines — shows no navigation control.
- The block-navigation keyboard shortcuts do nothing while no changeset is open.
