# Commit Graph

This contract defines the drawing of the commit graph: how commits are assigned to lanes, how the lines connecting them are routed, and how color and layering keep many simultaneous branches legible. Which commits appear in the graph, what each row displays, branch hiding, and selection are covered by the review workflow spec (`../review/workflow.md`); this spec governs the picture itself. The graph's job is to let a reviewer trace any branch from tip to merge point with the eye alone, so every rule below exists to keep each branch a single continuous, consistently colored stroke.

## Assigning commits to lanes

Every commit occupies exactly one lane — a fixed horizontal position its dot is drawn in. The left-most lane is the trunk: it belongs to the checked-out commit's first-parent history, the chain a reviewer most often follows. The trunk extends upward through fast-forwardable descendants, so a branch tip whose history is a pure first-parent continuation of the checked-out commit draws as more trunk rather than opening a side lane above it. When several tips compete to continue the trunk, the most recently authored chain wins and the rest remain side branches.

Everything off the trunk takes a lane to the trunk's right. A new side branch opens in the nearest free lane and never displaces a lane that is already in use; an occupied lane keeps its horizontal position until the branch it carries ends. When several side branches sprout from the same commit, their lanes are ordered by authored time: the branch with the earliest commits sits innermost (closest to the trunk), and each later sibling takes the next lane outward. Siblings keep their outward lanes until they rejoin the shared parent.

**Guaranteed invariants**

- The checked-out commit's first-parent history, extended through fast-forwardable descendants, occupies the left-most lane for its entire visible length.
- A commit that is not part of that history never appears in the left-most lane.
- A lane keeps its horizontal position from the moment a branch occupies it until that branch ends; new branches never evict it.
- Sibling branches sharing a parent are ordered inner-to-outer by the authored time of their commits, earliest innermost.

**Edge cases**

- When the newest loaded commit is a side-branch tip, the trunk lane is simply empty above the checked-out history; the side branch does not borrow it.
- Hiding a branch re-flows the layout as if its exclusive commits did not exist (see the review workflow spec): remaining branches may move to different lanes, but all of the rules above hold for the re-flowed result.

## Routing edges between a commit and its parents

A commit's connection to its first parent always leaves the commit's dot heading straight down, in the commit's own lane. The edge never sidesteps into another lane partway between the commit and its first parent — any change of lane happens only where the edge terminates, on the parent's row. This keeps every branch readable as one vertical line: a reviewer scanning down a lane is always following the same branch.

A merge commit's additional parents branch out from the merge dot itself: each extra edge leaves the dot, curves outward toward the lower border of the merge's row, and descends in the parent's lane from there. The fan-out at the dot is the visual signature of a merge.

When a branch edge reaches its parent's row, how it lands depends on whether the parent's lane is already carrying a line from above. If it is — the parent sits mid-trunk, or mid-branch, with its own history continuing past it — the arriving edge curves to horizontal along the upper border of the parent's row and joins the parent lane's vertical just above the dot, blending into the existing line rather than forming a hard tee against the dot. If the parent's lane is not fed from above — the parent is the top of its own line — the arriving edge runs horizontally at dot height and ends at the dot itself.

When sibling branches share a parent, they share a single edge into it: an inner sibling's line extends horizontally across to the outer sibling's lane and rides that one edge down to the shared parent, rather than drawing parallel duplicate lines into the same commit. A horizontal that must cross intermediate lanes to reach its destination stays at one consistent height for its whole run, even across lanes occupied by other active branches.

**Guaranteed invariants**

- A first-parent edge descends vertically in the commit's own lane; it never changes lanes between the commit and its first parent.
- Every commit dot sits on a vertical line segment. No horizontal line attaches directly at a dot except edges that begin or end at that commit's own row — a merge's fan-out, or an arriving edge landing at dot height on a parent whose lane is not fed from above.
- An edge arriving at a parent whose lane is fed from above joins the lane's vertical above the dot via a rounded curve, never at a right angle into the dot.
- Two edges converging on the same future parent coexist; neither deletes the other.
- A multi-lane horizontal stays straight and level across every intermediate lane it crosses.

**Edge cases**

- A branch whose parent is not in the visible history (an orphaned root, or a parent beyond the loaded window) simply ends; its lane stops without joining anything.
- A side branch may terminate on the checked-out commit's own row when that commit is its parent; the edge merges there like any other.
- A merge whose second parent is also reachable through an existing side edge keeps both: the merge fans out its own edge and the pre-existing side edge continues to the shared parent independently.

## Color and layering

Each branch edge carries a single color from a repeating palette, assigned when the edge first appears and held end to end — from the commit where the edge starts, down every vertical, around every bend, to the row where it terminates. Color is the second tracing aid after lane position: when lines cross or converge, color tells the reviewer which is which.

Layering resolves the crossings. Lanes paint right to left, so the lanes nearer the trunk — the longer-lived, more permanent lines — draw above branches that are bending in to join them. Where an outer sibling's horizontal run crosses an inner lane that is itself curving to merge, the outer edge keeps its own color visible beneath the inner lane's bend, so neither line appears broken. And where a vertical meets a curve it feeds — the lane above a merge bend, or the continuation below a branch-out — the vertical stops at the curve's tangent point rather than overlapping it, so the joint reads as one continuous stroke rather than two overdrawn lines.

**Guaranteed invariants**

- An edge is one color for its entire length; it never changes color at a bend, a crossing, or a lane boundary.
- At any crossing, both edges remain traceable: the crossing edge keeps its own color where it passes beneath another lane's bend.
- Lanes closer to the trunk render above branches joining them.
- Verticals and the curves they meet join seamlessly, with no visible overlap or gap at the tangent.

**Edge cases**

- With more simultaneous branches than palette entries, colors repeat; lane position remains the primary distinguisher and the continuity invariants still hold per edge.
