# Pull Requests

Greviewer surfaces the open pull requests for the repository under review, so a
reviewer can see which branches have proposed merges and jump to the commit each
one proposes. Pull-request data is read-only in this version.

## Loading pull requests for a repository

**Triggering conditions.** The user opens a repository whose origin is hosted on
the organization's pull-request server, and an access token is available to the
application.

**Observable outcomes.** The application loads every open pull request for that
repository in the background, without blocking the view or the local history.
When loading finishes, pull requests appear in the sidebar, the commit graph, and
the window bar. The user can refresh at any time to reload.

**Guaranteed invariants.** Loading never blocks interaction. The access token is
never shown, logged, or stored by the application. When no token is available,
the feature stays inert and explains, in general terms, that a token is required.

**Edge cases.** When the repository is not hosted on the pull-request server, the
feature is invisible — no section, no graph column content, no window-bar badge.
When authentication is rejected, the repository is unknown to the server, or the
server cannot be reached, the sidebar explains the problem and offers a way to
retry.

## Listing pull requests in the sidebar

**Observable outcomes.** A collapsible "Active PRs" section sits between the local
and remote branch groups. Each open pull request appears as a row showing its
number, newest first. The section header shows how many are open and offers a
refresh control. While loading, when none are open, or when a token is missing,
the section explains the current state instead of listing rows.

## Anchoring pull requests on the graph and window bar

**Observable outcomes.** In the commit graph, a dedicated resizable column marks
each commit that is the tip of a pull request's source branch with that pull
request's number. When several pull requests share a tip, all their numbers
appear in increasing numeric order. When the user is viewing a change set whose
primary commit is such a tip, the window bar appends the pull request's number;
if several share the tip, the lowest number is shown.

**Edge cases.** A commit that is not a pull-request tip shows nothing in the
column, which still occupies its place so the layout stays stable.

## Opening a pull request

**Triggering conditions.** The user clicks a pull request — its sidebar row or its
graph marker.

**Observable outcomes.** The commit at the tip of the pull request's source branch
is selected and scrolled into view, loading older history if needed to reach it.

**Edge cases.** When that commit cannot be found in the available history, a brief,
non-blocking notice explains that the commit for that pull request is not in the
loaded history, and the current selection is left unchanged.
