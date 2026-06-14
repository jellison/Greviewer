# Review Workflow

This contract defines the review experience: opening a Git repository, navigating its commit graph, selecting commits to review, browsing the rollup change set, and inspecting individual file diffs.

## Opening a repository

The user opens a local Git repository through a folder picker. The application reads the repository's history and presents the commit graph for that repository. Recently opened repositories persist between application launches. On launch the application automatically reopens the most-recently-opened repository so the user returns straight to where they left off; the list of recent repositories is surfaced for re-picking only when no repository can be reopened. Only one repository is open per window in v1.

**Triggering conditions**

- The application launches with a most-recently-opened repository that can still be opened.
- The user picks a folder via the open-repository affordance on launch or from within an open window.
- The user activates a recently opened repository entry on launch.

**Observable outcomes**

- The window displays the commit graph of the chosen repository.
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
mode, since the repository name is always shown.

**Triggering conditions**

- The user activates the repository name in the window bar.
- The user selects another repository in the switcher.
- The user activates the switcher's open-repository control.
- The user dismisses the switcher by activating outside it.

**Observable outcomes**

- Activating the repository name opens a switcher listing the sibling repositories in the
  parent folder, ordered by folder name, with the open repository marked as current.
- Selecting another repository opens it: the window shows that repository's commit graph, the
  prior selection and changeset are cleared, and the repository is moved to the top of the
  recently opened list.
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

## Quitting the application

The user can quit Greviewer with the standard quit keyboard shortcut.

**Triggering conditions**

- The user presses Cmd-Q while the Greviewer window is focused.

**Observable outcomes**

- The application begins its normal quit flow.

## Viewing the commit graph

The user sees a graphical history of the repository's commits with branch lanes and merge connectors. The history covers the checked-out commit and every local branch, so work on branches that have not been merged is visible alongside the checked-out history; remote-tracking branches and tags do not contribute commits. Commits from all branches interleave in a single graph ordered newest-first, with no visual distinction between commits that are and are not part of the checked-out history. Branch and merge lines use smooth rounded bends where they turn between lanes, including the final turn into a branch commit. Each commit is presented as a single-row entry whose reading order is graph, short identifier, summary line, author, authored date, and any local branch names that point at it. The graph is scrollable; older commits load progressively as the user scrolls. The currently checked-out tip is visually marked, but selection is independent of checkout state — reviewing never moves HEAD.

**Triggering conditions**

- A repository is open.

**Observable outcomes**

- Commits appear in graphical order with branch lanes and merge connectors.
- Commits reachable only from unmerged local branches appear interleaved with the
  checked-out history; each such branch renders as its own lane that ends at its tip.
- Each visible commit shows its short identifier, summary line, author, and authored date.
- Local branch names are shown on the commits they point to.
- The currently checked-out tip carries a visual marker.

**Edge cases**

- Very large histories load progressively rather than blocking the UI.
- Histories with several active branches remain legible: the visible first-parent history of the checked-out commit anchors the left-most lane, active branch lanes keep a stable horizontal position until they end, branch lanes render as continuous vertical lines through and between rows without row separators or rounded branch/merge joins interrupting them, horizontal branch and merge runs appear at row boundaries so commits remain centered within their branch lanes, multiple visible edges may target the same future parent commit without deleting one another, sibling side branches with the same trunk parent keep strictly earlier authored commits on inner side lanes, keep later siblings on their outward lanes until they rejoin the shared parent, share the same trunk branch-off with horizontal extensions to outer lanes, new branch lanes use the nearest available lane to the right and do not reuse occupied lanes, multi-lane connectors stay continuous and horizontally aligned across intermediate lanes even when those lanes are occupied by other active branches, and branch labels do not obscure the graph.
- An unmerged branch's lane runs from its tip down to the commit where it diverged from
  visible history and ends there; when the tip is newer than the checked-out commit, the
  left-most lane is simply empty above the checked-out commit's row.
- A branch that shares no history with the others ends without joining any other lane.
- A repository whose checked-out branch has no commits yet still shows the history of its
  other local branches.
- Detached-HEAD repositories render normally with no checked-out branch marker.

## Navigating branches from the sidebar

Graph mode includes a sidebar beside the graph listing the repository's local branches by name, in alphabetical order. Branch names containing `/` nest under collapsible folders (see "Nesting branches in sidebar folders" below). The sidebar mirrors the graph's scope: local branches only, with remote-tracking branches and tags excluded. The checked-out branch carries a visual marker. Activating a branch focuses it in the graph: the branch's tip commit becomes the selected commit and the graph scrolls so that commit is visible. The divider between the sidebar and the graph is draggable to resize the sidebar. The sidebar exists only in graph mode; review mode shows the file tree in its place.

**Triggering conditions**

- A repository is open and the window is in graph mode.
- The user activates a branch entry in the sidebar.
- The user drags the divider between the sidebar and the graph.

**Observable outcomes**

- The sidebar lists every local branch, alphabetically within its folder level, with the checked-out branch visually marked.
- Activating a branch selects its tip commit and scrolls the graph so the commit is visible.
- A branch entry whose tip commit is the current selection is visually distinct, using the same selected treatment as the commit row.
- Dragging the divider resizes the sidebar.

**Edge cases**

- A repository with no local branches shows an explanatory message in place of the list.
- Detached-HEAD repositories list branches normally with no checked-out marker.
- Activating a branch whose tip commit has not yet been loaded into the visible history loads older history until the tip appears, then selects and reveals it.
- Activating a branch whose tip is already the selected commit keeps that selection; it does not clear it.
- When the branch list exceeds the sidebar's height, it scrolls; a scrollbar appears while the pointer is over the sidebar and stays out of the way otherwise.

## Nesting branches in sidebar folders

Branch names that contain `/` nest in the sidebar: every name segment except
the last becomes a collapsible folder, so `features/some-feature` appears as
a `some-feature` row inside a `features` folder, and `team/alice/feature-x`
nests two folders deep. A folder exists even when it holds a single branch —
grouping depends only on the branch's own name, so rows do not reorganize as
siblings appear or disappear. Within a level, folders list before branches,
each alphabetically. A nested branch row shows only its final name segment,
indented under its folders; everywhere else — graph labels, hiding,
focusing — the branch keeps its full name.

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
  visible. A selection that becomes invisible clears, as with single-branch
  hiding.
- A fully hidden folder renders muted with its toggle always shown; a
  partially hidden folder keeps its toggle shown without muting.
- Reopening a repository expands all folders and shows all branches.

## Hiding branches from the graph

Every branch except the checked-out branch can be toggled off from the
sidebar. A hidden branch's name no longer appears as a ref label in the
graph, and commits reachable only from hidden branches are removed: the graph
re-flows as if those commits did not exist. Commits a hidden branch shares
with any visible branch (or with the checked-out branch) remain.

The toggle control on a visible branch is revealed when the pointer is over
its row; a hidden branch's control is always visible and its name renders
muted. Activating a hidden branch's row does not focus it; the branch must be
shown again first. The checked-out branch offers no toggle. If hiding a
branch removes the selected commit — or any commit in a selected range — the
selection clears. Visibility choices are not persisted: opening a repository
shows every branch.

**Observable outcomes**

- A non-checked-out branch row reveals a visibility toggle on hover; the
  checked-out branch row never shows one.
- Toggling a branch off removes its ref labels and its exclusive commits from
  the graph; toggling it back on restores them.
- A hidden branch's row is muted, keeps its toggle visible, and does not
  focus its tip when activated.
- Hiding a branch that makes the current selection invisible clears the
  selection.
- Reopening a repository resets all branches to visible.

## Selecting commits to review

The user stages a review by selecting either a single commit or a contiguous sequential range from the graph. Selection is tentative: no review activity begins until the user explicitly opens the changeset (described below). Clicking a commit selects it. Shift-clicking a second commit extends the selection to a range, provided the two commits lie on a single ancestry path (one is an ancestor of the other). The selection is the inclusive set of commits between the two endpoints along that path. Clicking the selected commit again clears the selection.

**Triggering conditions**

- The user clicks a commit in the graph.
- The user shift-clicks a second commit while a single-commit selection is active.
- The user clicks the selected commit again.

**Observable outcomes**

- The selected commit or range is visually distinct in the graph.

**Edge cases**

- If the two endpoints do not share a linear ancestry — they lie on diverged branches — the second click is rejected with a message explaining why, and the original selection is preserved.
- Merge commits inside a selected range are included in the range and contribute to the rollup.

## Opening the changeset

With a valid selection in place, the user opens the changeset to begin reviewing. Opening transitions the window from graph mode to review mode, where the file tree and diff viewer are the primary content. The graph remains the basis for the open selection but is not the active surface. The user closes the open changeset to return to graph mode; the prior selection is preserved so it can be adjusted and re-opened without re-selecting from scratch. Modifying the selection while a changeset is open is not supported in v1 — the user closes the changeset, adjusts, and reopens.

**Triggering conditions**

- The user activates the open-changeset affordance while a valid selection is active.
- The user closes the open changeset.

**Observable outcomes**

- Activating the open-changeset affordance transitions the window into review mode and renders the change set for the current selection.
- The open-changeset affordance is unavailable when no selection is active or when the selection is not a valid contiguous range.
- Closing the changeset returns the window to graph mode with the prior selection preserved.
- Review mode does not move HEAD or modify the repository.

**Edge cases**

- Opening a changeset whose net effect is no change still transitions to review mode; the change set view shows the empty-state message described in the next section.

## Seeing diff context in the window bar

While a changeset is open, the window bar shows the open diff's context next to the
repository name, in the form `{repository} / {commit identifier}`. For a single-commit
changeset the commit identifier is the commit's short identifier; for a range it is the
newest commit's short identifier followed by the number of commits in the range. The
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
- Activating the identifier opens a popover that shows, for a range, the oldest and newest
  short identifiers and the commit count, and for a single commit, the commit summary.
- The popover shows the number of changed files and the total added and removed line counts
  for the changeset.
- The popover shows a breakdown of the changed files by how they changed (added, modified,
  deleted, renamed), listing only the kinds that are present.
- For a range, the popover lists the commits in the changeset, newest first, each with its
  short identifier and summary; the list scrolls when it exceeds the popover's height. A
  single-commit changeset omits the list because its summary already appears in the header.
- The popover offers a control that closes the changeset, returning the window to graph mode
  with the prior selection preserved.
- Dismissing the popover by activating outside it leaves the changeset open.

**Edge cases**

- A changeset whose net effect is no change shows zero changed files, `+0 / −0` lines, and no
  kind breakdown in the popover; the close control still returns to graph mode.
- A single-commit changeset whose commit is not in the loaded history window falls back to
  showing the commit's short identifier in place of a summary.
- A range commit that is not yet in the loaded history window appears in the commit list with
  its short identifier and no summary.

## Reviewing the change set

With a changeset open, the user sees the rollup change set: a file tree containing every file whose content differs between the state immediately before the oldest selected commit and the state at the newest selected commit. The tree shows each file's path and an indicator of how it changed (added, modified, deleted, renamed). Selecting a file in the tree opens that file's diff (described below).

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

Opening a file in the change set presents its diff. The diff fills the pane edge to edge below the tab bar; the tab itself names the file and colors it by change kind, so the pane adds no header of its own. For files that exist on both sides of the selection (modified or renamed files), the diff is shown side-by-side: the "before" state on the left, the "after" state on the right, with corresponding lines aligned and the two sides separated by a thin divider. The user can scroll both sides; the alignment is preserved as they scroll. Line numbers are shown for each side.

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
- Within a modified line pair, changed tokens carry a stronger background tint, unless the lines differ almost entirely.
- Rows with no counterpart line on the other side render as a hatched region.
- Files with a recognized type render syntax-highlighted; unrecognized types render as plain text.
- Line numbers appear for each side of a side-by-side diff and for the single side of a full-width view.

**Edge cases**

- Renamed files diff old-path content against new-path content; the file's pre-rename path is surfaced so the user can see what was renamed.
- Binary files render an explanatory placeholder rather than attempting a textual diff.
