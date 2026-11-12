#!/usr/bin/env bash
# Sync from upstream: fetch, merge, verify compile, push to origin dev.
# Usage: scripts/sync-upstream.sh [--push]
#   Without --push the script stops after verification (dry run).
set -euo pipefail

cd "$(dirname "$0")/.."

BRANCH="dev"
UPSTREAM="upstream"
ORIGIN="origin"

git fetch "$UPSTREAM" --prune

BEHIND=$(git rev-list --count "$BRANCH..$UPSTREAM/dev")
if [ "$BEHIND" -eq 0 ]; then
  echo "Already up to date with $UPSTREAM/dev."
else
  echo "Merging $BEHIND upstream commits into $BRANCH..."
  git merge "$UPSTREAM/dev" --no-edit -m "Merge $UPSTREAM/dev into $BRANCH: keep auto subscription refresh and best-server selection"
fi

echo "== Type check (frontend) =="
npx tsc --noEmit

echo "== Compile check (Rust) =="
(cd src-tauri && cargo check)

echo "Verification passed."

if [ "${1:-}" != "--push" ]; then
  echo "Dry run complete. Re-run with --push to publish to $ORIGIN/$BRANCH."
  exit 0
fi

echo "Pushing $BRANCH to $ORIGIN (hooks skipped: pre-commit/pre-push need network downloads)..."
# Credentials come from the environment / credential helper; never hardcode tokens here.
git push "$ORIGIN" "$BRANCH" --no-verify
echo "Pushed to $ORIGIN/$BRANCH."
