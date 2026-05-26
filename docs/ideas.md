# Ideas

A seed list of feature ideas that are not yet developed enough to become specs. Entries here are not commitments and they are not a roadmap. They exist so an idea is not lost between the moment it surfaces and the moment it is ready to design.

When an entry is fleshed out into a real design, it graduates to a spec under `docs/specs/` and is removed from this file.

## Comments on files and lines

Adding, persisting, viewing, and exporting per-file or per-line comments during a review. Touches the review workflow and depends on a persistence model the codebase does not yet have.

## Marking files or hunks as reviewed

Persisting per-file or per-hunk review state so a reviewer can track progress through a change set across sessions. Shares a persistence model with comments.

## Filtering the change set to un-reviewed items

Hiding entries from the file tree that the reviewer has already marked as reviewed, so the remaining work is easier to see. Builds on the reviewed-state idea above.

## AI-powered changeset context

When a changeset is opened, surface AI-generated context to help the reviewer get oriented: a brief summary of the changes from a business-rules perspective, and a recommended order in which to review the files for the best understanding.

## AI questions about selected diff code

Let a reviewer select code in the diff viewer and ask an AI a focused question about that selection, such as "explain this" or "why did we do this?" Depends on text selection in the diff viewer, an AI provider integration, and a prompt context model that can include the selected lines, file path, and surrounding changes.
