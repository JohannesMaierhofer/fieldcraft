#!/bin/bash
# Select desktop DMGs from the matching native source tree before assigning macOS runners.
set -euo pipefail
base="repos/$GITHUB_REPOSITORY/actions"
run="${DESKTOP_RUN:-}"
if [[ "$GITHUB_REF_NAME" == codex/ngsolve-macos-recheck-* ]]; then
    run=${GITHUB_REF_NAME#codex/ngsolve-macos-recheck-}
fi
desktop_paths=(. ':(exclude).github/workflows/macos-ngsolve-installer.yml' ':(exclude).github/workflows/ngsolve-platforms.yml'
    ':(exclude)packaging/ngsolve' ':(exclude)packaging/macos/ngsolve' ':(exclude)packaging/macos/ngsolve-package.sh')
same_desktop() {
    git diff --quiet "$1" HEAD -- "${desktop_paths[@]}"
}
if [[ -z "$run" && "${BEFORE:-}" =~ ^[0-9a-f]{40}$ && "${BEFORE:-}" != 0000000000000000000000000000000000000000 ]] && same_desktop "$BEFORE"; then
    desktop=$(git log -1 --format=%H "$BEFORE" -- "${desktop_paths[@]}")
    run=$(gh api "$base/workflows/macos-ngsolve-installer.yml/runs?head_sha=$desktop&per_page=20" \
        --jq '[.workflow_runs[] | select(.head_branch=="codex/ngsolve-installers" and .event=="push")][0].id // empty')
fi
if [[ -z "$run" ]]; then exit 0; fi
[[ "$run" =~ ^[0-9]+$ ]]
metadata=$(gh api "$base/runs/$run")
jq -e '.head_branch=="codex/ngsolve-installers" and .path==".github/workflows/macos-ngsolve-installer.yml" and (.event=="push" or .event=="workflow_dispatch")' <<< "$metadata" >/dev/null
sha=$(jq -r '.head_sha' <<< "$metadata")
[[ "$sha" =~ ^[0-9a-f]{40}$ ]]
git cat-file -e "$sha^{commit}"
same_desktop "$sha" || { echo "The selected run built a different desktop source tree" >&2; exit 1; }
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
