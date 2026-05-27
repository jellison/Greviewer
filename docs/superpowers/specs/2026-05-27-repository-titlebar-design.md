# Repository Titlebar Design

This design defines a small UI polish slice for Greviewer's window titlebar. The current app opens with a static native title of `Greviewer`, even after a repository is open. That leaves the desktop window chrome less useful than the in-app header and unlike the quiet editor-style title treatment the user expects from Zed-inspired tools.

The slice should make the native titlebar identify the opened repository without expanding scope into branch, worktree, or custom-titlebar rendering.

## Scope

When no repository is open, the native titlebar text is empty. After a repository opens successfully, the titlebar shows only the repository directory name. Opening a different repository updates the titlebar to that repository name.

Cancelled repository picks, picker failures, and failed repository opens preserve the current titlebar text. If no repository was open before the failure, the titlebar remains empty.

The in-window headers can remain unchanged in this slice. They still show the repository path and current review context.

## Recommended Approach

Use gpui's native window-title API instead of drawing an application-owned titlebar. The production window should be created with an empty title, and successful repository opens should call the window title setter with the basename of the canonical repository path.

This is the smallest approach that matches the requested visual outcome. It relies on platform window chrome for styling, traffic-light placement, and drag behavior, so it avoids the accessibility and window-management risks of a custom drawn titlebar.

Two alternatives are rejected. A fake in-app titlebar would create layout, hit-testing, and drag-region work for a single text change. Adding branch or worktree metadata would mimic more of the screenshot, but the user explicitly excluded it from this tweak.

## Behavior Details

The repository name is derived from the opened repository path's final component. If a valid repository path has no final component, the app falls back to an empty title rather than showing the full path.

Title updates happen only after repository opening succeeds. The title update should live on the same success path that resets review state and records the recent repository, so every successful open behaves consistently.

## Testing

The implementation should add focused gpui coverage for the visible title behavior:

- the app window starts with an empty title;
- opening a valid repository sets the title to that repository directory name;
- opening another valid repository replaces the title;
- opening an invalid path after a valid repository preserves the prior title;
- opening an invalid path with no prior repository leaves the title empty.

The existing open-repository state tests remain useful, but the title behavior should be asserted through gpui's window-title reader rather than by inspecting private app state.

## Spec Update

The review workflow spec should be updated in the implementation change because this is user-visible behavior. The product contract should say that an open repository is identified in the native titlebar by repository name, and that no-repository state leaves the titlebar blank.

## Definition of Done

The slice is complete when the native titlebar starts empty, successful repository opens update it to the repository directory name, failed opens preserve it, the review workflow spec reflects the behavior, targeted gpui tests cover the title updates, and `bin/check` passes.
