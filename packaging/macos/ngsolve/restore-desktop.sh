#!/bin/bash
# Restore and verify one exact native desktop without compiling another app.
set -euo pipefail
[[ "$ARCH" == "$(uname -m)" ]]
name="fieldcraft-desktop-$ARCH-$DESKTOP_SHA"
base="repos/$GITHUB_REPOSITORY/actions"
record=$(gh api "$base/runs/$DESKTOP_RUN/artifacts?per_page=100" | jq -ce --arg n "$name" '[.artifacts[] | select(.name==$n and .expired==false)] | select(length==1) | .[0]')
id=$(jq -r '.id' <<< "$record")
expected=$(jq -r '.digest' <<< "$record")
[[ "$id" =~ ^[0-9]+$ && "$expected" =~ ^sha256:[0-9a-f]{64}$ ]]
stage="$RUNNER_TEMP/desktop-artifact"
mkdir -p "$stage/mount" "$RUNNER_TEMP/target/macos-package" verification
gh api "$base/artifacts/$id/zip" > "$stage/desktop.zip"
actual=$(/usr/bin/shasum -a 256 "$stage/desktop.zip" | awk '{print $1}')
[[ "sha256:$actual" == "$expected" ]]
unzip -q "$stage/desktop.zip" -d "$stage"
images=("$stage"/fieldcraft-*-macos-*.dmg)
[[ ${#images[@]} == 1 && -f "${images[0]}" ]]
hdiutil attach -nobrowse -readonly -mountpoint "$stage/mount" "${images[0]}" >/dev/null
trap 'hdiutil detach "$stage/mount" >/dev/null' EXIT
app="$RUNNER_TEMP/target/macos-package/FieldCraft.app"
ditto "$stage/mount/FieldCraft.app" "$app"
codesign --verify --strict --deep "$app"
embedded=$(/usr/libexec/PlistBuddy -c 'Print :FieldCraftBuildCommit' "$app/Contents/Info.plist")
[[ "$embedded" == "$DESKTOP_SHA" ]] || { echo "Unexpected embedded desktop revision: $embedded" >&2; exit 1; }
lipo "$app/Contents/MacOS/FieldCraft" -verify_arch "$ARCH"
lipo "$app/Contents/MacOS/fieldcraft-cli" -verify_arch "$ARCH"
jq -n --arg arch "$ARCH" --arg revision "$DESKTOP_SHA" --argjson run "$DESKTOP_RUN" --argjson id "$id" --arg digest "$expected" \
    '{architecture:$arch,desktop_revision:$revision,source_run:$run,artifact_id:$id,artifact_digest:$digest}' > verification/desktop-source.json
