# Git Guide

This repository uses a linear history and conventional commit messages. Keep commits easy to read in `git log` and easy to reason about in code review.

## Conventional Commits

Format: `<type>(<scope>): <description>`

- `type` is required and should describe the intent of the change.
- `scope` is optional but preferred for subsystem-level changes (for example: `repo`, `graph`, `tree`, `diff`, `viewer`, `selection`, `changeset`, `ui`).
- Use `!` before `:` for breaking changes (example: `feat(diff)!: ...`).
- For breaking changes, include a `BREAKING CHANGE:` footer with migration notes.
- Keep the description short, imperative, and specific.

### Commit Types

- `feat`: New user-facing behavior or capability.
- `fix`: Bug fix or correctness repair.
- `refactor`: Internal code change that does not change behavior.
- `perf`: Performance improvement.
- `test`: Add or update tests only.
- `docs`: Documentation-only changes.
- `build`: Build system or dependency changes.
- `ci`: CI pipeline or automation changes.
- `chore`: Repo maintenance that does not fit the types above.
- `revert`: Revert a previous commit.

## History Hygiene

- Prefer rebase over merge to keep history linear (`git pull --rebase`, `git rebase <base-branch>`).
- Prefer one cohesive commit per logical change; combine related work instead of keeping many micro-commits.
- Before opening a merge request, clean up branch history so each commit is meaningful and self-contained.
- Treat agent-created Git refs as temporary state. When an agent finishes a workflow, it must delete any refs it created, including refs under `refs/backup/` and `refs/codex/`, once those refs are no longer needed for recovery.
- Use a scoped ref deletion command for agent namespaces instead of broad repository cleanup:
  ```bash
  git for-each-ref --format='delete %(refname)' refs/backup refs/codex | git update-ref --stdin
  git for-each-ref --format='%(refname)' refs/backup refs/codex
  ```
  The verification command should return no rows before the agent reports cleanup complete.
