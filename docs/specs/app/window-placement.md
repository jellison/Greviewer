# Window Placement

This contract defines how the application's main window remembers where and how
it was shown, so that each launch returns the user to the window they left
rather than a generic default. It covers restoring the window on launch and
capturing its placement whenever the user leaves.

## Restoring the window on launch

When the application starts, it opens its window at the size, position, and
display state — normal, maximized, or full screen — it had when it was last
closed, on the same monitor. The user returns to a window exactly where they
left it, without having to move or resize it again.

**Triggering conditions**

- The application launches after having been closed at least once with a
  remembered window placement.

**Observable outcomes**

- The window appears at its previous size and position.
- If the window was maximized or full screen when last closed, it returns to
  that state; leaving that state reveals the previous normal-window size.
- The window appears on the same physical monitor it was last shown on.

**Guaranteed invariants**

- Window placement is remembered per physical monitor, so reconnecting a
  previously used display restores the window to it.
- The remembered placement persists across application restarts.

**Edge cases the user can encounter**

- On first launch, or when no placement has been remembered, the window opens
  at a default size centered on the primary monitor.
- If the monitor the window was last shown on is no longer connected, the
  window opens at the default size centered on the primary monitor.
- A remembered window never returns smaller than a usable minimum size.

## Remembering placement when leaving

Whenever the user closes the window or quits the application, the current
window placement is saved so the next launch can restore it. Both ways of
leaving are treated the same: neither closing the window nor quitting outright
loses where the user had put it.

**Triggering conditions**

- The user closes the window, or quits the application.

**Observable outcomes**

- The most recent size, position, display state, and monitor are remembered for
  the next launch.
