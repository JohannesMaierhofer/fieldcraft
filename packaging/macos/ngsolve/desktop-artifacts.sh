#!/bin/bash
# Select desktop DMGs from the matching native source tree before assigning macOS runners.
set -euo pipefail
base="repos/$GITHUB_REPOSITORY/actions"
run="${DESKTOP_RUN:-}"
if [[ -z "$run" ]]; then exit 0; fi
[[ "$run" =~ ^[0-9]+$ ]]
metadata=$(gh api "$base/runs/$run")
jq -e --arg sha "$GITHUB_SHA" --arg repo "$GITHUB_REPOSITORY" '
    .head_sha == $sha and .head_repository.full_name == $repo and
    .path == ".github/workflows/macos-ngsolve-installer.yml" and .event == "workflow_dispatch"
' <<< "$metadata" >/dev/null || { echo "The selected macOS run must build this exact release commit." >&2; exit 1; }
sha=$GITHUB_SHA
for ((attempt=0; attempt<120; attempt++)); do
    records=$(gh api "$base/runs/$run/artifacts?per_page=100")
    ok=true
    for arch in arm64 x86_64; do
        id=$(jq -r --arg n "fieldcraft-desktop-$arch-$sha" '[.artifacts[]|select(.name==$n and .expired==false)]|if length==1 then .[0].id else empty end' <<< "$records")
        [[ -n "$id" ]] || ok=false
    done
    if [[ "$ok" == true ]]; then
        printf 'run=%s\nsha=%s\n' "$run" "$sha" >> "$GITHUB_OUTPUT"
        exit 0
    fi
    if [[ $(gh api "$base/runs/$run" --jq '.status') == completed ]]; then
        echo "Desktop artifacts missing from completed run $run" >&2; exit 1
    fi
    sleep 60
done
echo "Timed out waiting for both native desktop artifacts in run $run" >&2; exit 1
