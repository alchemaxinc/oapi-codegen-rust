#!/usr/bin/env bash
# Idempotently finds an existing issue by exact title, or creates a new one.
#
# This is the backing script for the `find-or-create-issue` composite action
# (see ./action.yml) — use that via `uses: ./.github/actions/find-or-create-issue`
# for normal single-call sites (e.g. cargo-deny-scheduled.yml).
#
# Composite actions can only be invoked via a `uses:` step, which can't be
# looped inline within a single job step. Callers that need to file one issue
# per item in a bash loop (e.g. check-openapi-versions.yml, one issue per
# newly released OpenAPI version) instead invoke this script file directly by
# path, so both call sites share the exact same logic with no duplication.
#
# Requires GH_TOKEN (or GITHUB_TOKEN) in the environment, as used by `gh`.
# Prints the resulting issue URL to stdout; progress goes to stderr as
# workflow annotations.
#
# Usage:
#   find-or-create-issue.sh --title TITLE --body BODY \
#     [--label LABEL] [--type TYPE] [--state open|all]
set -euo pipefail

title=""
body=""
label=""
type=""
state="all"

while [ $# -gt 0 ]; do
  case "$1" in
    --title)
      title="$2"
      shift 2
      ;;
    --body)
      body="$2"
      shift 2
      ;;
    --label)
      label="$2"
      shift 2
      ;;
    --type)
      type="$2"
      shift 2
      ;;
    --state)
      state="$2"
      shift 2
      ;;
    *)
      echo "::error::find-or-create-issue.sh: unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [ -z "$title" ] || [ -z "$body" ]; then
  echo "::error::find-or-create-issue.sh: --title and --body are required" >&2
  exit 1
fi

url=$(gh issue list --state "$state" --search "$title in:title" \
  --json title,url --jq "[.[] | select(.title == \"$title\")][0].url // empty")

if [ -z "$url" ]; then
  args=(--title "$title" --body "$body")
  [ -n "$label" ] && args+=(--label "$label")
  [ -n "$type" ] && args+=(--type "$type")
  url=$(gh issue create "${args[@]}")
  echo "::notice::Opened issue: $url" >&2
else
  echo "::notice::Reusing existing issue: $url" >&2
fi

echo "$url"
