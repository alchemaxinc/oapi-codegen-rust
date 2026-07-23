#!/usr/bin/env bash
#
# Probes the upstream OpenAPI specification releases and reconciles them against
# the versions this repository tracks in .github/openapi-versions.json.
#
# For every released OpenAPI minor version that is higher than the highest
# version we currently support and that we are not already tracking, this
# script:
#
#   * opens a GitHub issue asking Copilot for a deep dive into supporting it,
#   * adds a "Planned" row to the README support matrix, and
#   * records the version in the manifest so it is not processed again.
#
# It is idempotent: re-running it does not create duplicate issues or rows.
#
# Required environment:
#   GH_TOKEN            token used by the gh CLI (issues + repo read)
# Optional environment:
#   GITHUB_REPOSITORY   owner/repo (defaults to alchemaxinc/oapi-codegen-rust)
#   ISSUE_LABEL         label applied to created issues (default: enhancement)
#   ISSUE_TYPE          issue type applied to created issues (default: Feature)
#   GITHUB_OUTPUT       when set, "changed=true" is written on any change
set -euo pipefail

REPO="${GITHUB_REPOSITORY:-alchemaxinc/oapi-codegen-rust}"
UPSTREAM_REPO="OAI/OpenAPI-Specification"
MANIFEST=".github/openapi-versions.json"
README="README.md"
ISSUE_LABEL="${ISSUE_LABEL:-enhancement}"
ISSUE_TYPE="${ISSUE_TYPE:-Feature}"
UPSTREAM_RELEASES="https://github.com/${UPSTREAM_REPO}/releases"
WORKFLOW_PATH=".github/workflows/check-openapi-versions.yml"

# Returns 0 when minor version $1 is strictly greater than minor version $2.
minor_gt() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    split(a, x, "."); split(b, y, ".");
    if (x[1] + 0 > y[1] + 0) { exit 0 }
    if (x[1] + 0 < y[1] + 0) { exit 1 }
    exit (x[2] + 0 > y[2] + 0) ? 0 : 1
  }'
}

# Inserts a "Planned" support-matrix row after the last tracked version row.
add_readme_row() {
  local minor="$1" url="$2"
  local row="| v${minor}.0 | Planned | Tracking [here](${url}). |"
  awk -v row="$row" '
    /^\| v3\./ { last = NR }
    { lines[NR] = $0 }
    END {
      for (i = 1; i <= NR; i++) {
        print lines[i]
        if (i == last) { print row }
      }
    }
  ' "$README" >"${README}.tmp"
  mv "${README}.tmp" "$README"
}

# Prints the URL of an existing issue whose exact title matches, or nothing.
existing_issue_url() {
  local title="$1"
  gh issue list --repo "$REPO" --state all --search "${title} in:title" \
    --json title,url --jq ".[] | select(.title == \"${title}\") | .url" |
    head -n1
}

issue_body() {
  local minor="$1" highest="$2"
  cat <<EOF
@copilot

OpenAPI **${minor}** has been released upstream (see the [OpenAPI Specification releases](${UPSTREAM_RELEASES})), but \`oapi-codegen-rust\` currently supports up to OpenAPI **${highest}**.

Please perform a thorough deep dive into what it would take to support OpenAPI ${minor} in this repository:

- Summarise the notable changes introduced in OpenAPI ${minor} relative to ${highest} at the specification level.
- Identify the concrete areas of the current implementation (loader, lowering, IR, emitters, config, tests) that would need to change.
- Call out the biggest challenges, ambiguities, and design decisions that need to be made.
- Propose a rough, staged plan of action to reach parity, in line with how support was added for previous versions.

_Filed automatically by the [OpenAPI version check workflow](${WORKFLOW_PATH})._
EOF
}

main() {
  local highest
  highest="$(jq -r '.highest_supported' "$MANIFEST")"

  local tracked_json
  tracked_json="$(jq -c '.tracked' "$MANIFEST")"

  local released
  released="$(
    gh api "repos/${UPSTREAM_REPO}/releases" --paginate \
      --jq '.[] | select(.prerelease == false) | .tag_name' |
      grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' |
      awk -F. '{ print $1 "." $2 }' |
      sort -u -t. -k1,1n -k2,2n
  )"

  local changed=false
  local minor
  while IFS= read -r minor; do
    [ -n "$minor" ] || continue

    if jq -e --arg m "$minor" 'index($m) != null' <<<"$tracked_json" >/dev/null; then
      continue
    fi

    if ! minor_gt "$minor" "$highest"; then
      continue
    fi

    local title="Support OpenAPI ${minor}"
    local url
    url="$(existing_issue_url "$title")"

    if [ -z "$url" ]; then
      echo "Creating issue: ${title}"
      url="$(
        gh issue create --repo "$REPO" \
          --title "$title" \
          --label "$ISSUE_LABEL" \
          --type "$ISSUE_TYPE" \
          --body "$(issue_body "$minor" "$highest")"
      )"
    else
      echo "Reusing existing issue for ${title}: ${url}"
    fi

    add_readme_row "$minor" "$url"
    tracked_json="$(jq -c --arg m "$minor" '. + [$m]' <<<"$tracked_json")"
    changed=true
  done <<<"$released"

  if [ "$changed" = true ]; then
    jq --argjson tracked "$tracked_json" \
      '.tracked = ($tracked | sort_by(split(".") | map(tonumber)))' \
      "$MANIFEST" >"${MANIFEST}.tmp"
    mv "${MANIFEST}.tmp" "$MANIFEST"
    echo "New OpenAPI versions tracked."
  else
    echo "No new OpenAPI versions detected."
  fi

  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    echo "changed=${changed}" >>"$GITHUB_OUTPUT"
  fi
}

main "$@"
