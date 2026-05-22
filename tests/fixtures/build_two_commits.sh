#!/usr/bin/env bash
# Reproduces tests/fixtures/two-commits/dot-git/ from scratch.
#
# Run from the repo root:
#   bash tests/fixtures/build_two_commits.sh
#
# This script is not invoked by `cargo test`; it exists so the fixture is
# reproducible without spelunking the binary contents.

set -euo pipefail

FIXTURE_ROOT="tests/fixtures/two-commits"

rm -rf "$FIXTURE_ROOT"
mkdir -p "$FIXTURE_ROOT"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

cd "$WORK_DIR"

git init --quiet --initial-branch=main

git config user.name "Greviewer Tests"
git config user.email "tests@greviewer.invalid"

GIT_AUTHOR_DATE="2026-01-01T12:00:00Z"
GIT_COMMITTER_DATE="2026-01-01T12:00:00Z"
export GIT_AUTHOR_DATE GIT_COMMITTER_DATE

echo "hello" > hello.txt
git add hello.txt
git commit --quiet -m "Add hello.txt"

GIT_AUTHOR_DATE="2026-01-01T12:30:00Z"
GIT_COMMITTER_DATE="2026-01-01T12:30:00Z"
export GIT_AUTHOR_DATE GIT_COMMITTER_DATE

echo "hello world" > hello.txt
git add hello.txt
git commit --quiet -m "Update hello.txt"

cd - >/dev/null

cp -R "$WORK_DIR/.git" "$FIXTURE_ROOT/dot-git"
echo "Generated $FIXTURE_ROOT/dot-git/"
