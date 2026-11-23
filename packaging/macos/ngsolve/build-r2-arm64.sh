#!/bin/bash
# Build the r2 desktop on Apple silicon, sign the bundle ad hoc, and invoke the pinned Mac packager.
set -euo pipefail
if [[ $# -lt 3 || $# -gt 4 ]]; then
    echo "Usage: $0 clean-desktop-checkout THIRD-PARTY-LICENSES.txt output.pkg [offline-cache]" >&2
    exit 2
fi
src=$(cd "$1" && pwd)
licences=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
out=$(cd "$(dirname "$3")" && pwd)/$(basename "$3")
cache=''
if [[ -n "${4:-}" ]]; then cache=$(cd "$4" && pwd); fi
here=$(cd "$(dirname "$0")" && pwd)
packager=$(git -C "$here" rev-parse --show-toplevel)
revision=8b495f31389512c7f599ef17b5ee3a4d0cc39bc9
recipe=6642362fde3404b5c83931d2546c438499bceddf
version=0.8.1-ngsolve.r1
fail() { echo "$*" >&2; exit 1; }
[[ "$(uname -s)/$(uname -m)" == Darwin/arm64 ]] || fail 'Run the desktop and solver checks on native Apple silicon'
[[ "$out" == *.pkg && ! -e "$out" && ! -e "$out.sha256" ]] || fail 'Expected a new package path'
[[ "$(git -C "$src" rev-parse HEAD)" == "$revision" ]] || fail 'Unexpected desktop source revision'
git -C "$src" diff-index --quiet HEAD -- || fail 'Modified desktop source'
[[ -z "$(git -C "$src" ls-files --others --exclude-standard)" ]] || fail 'Untracked desktop source'
inputs=(packaging/macos/ngsolve-package.sh packaging/macos/ngsolve/arm64.lock packaging/macos/ngsolve/postinstall
        packaging/macos/ngsolve/fieldcraft-ngsolve 'packaging/macos/ngsolve/Run NGSolve Case.command' packaging/ngsolve/SOLVER-NOTICES.txt)
git -C "$packager" diff --quiet "$recipe" -- "${inputs[@]}" || fail 'Mac installation recipe differs from the r2 revision'
expected=1775ff2eb03e65ba2ebc7cbac1b45c525d55fed73c4e54b4dcd687660206ae3b
[[ "$(shasum -a 256 "$licences" | awk '{print $1}')" == "$expected" ]] || fail 'Unexpected Rust licence inventory'
work=$(mktemp -d "${TMPDIR:-/tmp}/fieldcraft-arm64.XXXXXXXX")
trap 'rm -rf "$work"' EXIT
app="$work/FieldCraft.app"
contents="$app/Contents"
export FIELDCRAFT_BUILD_SHA="$revision" FIELDCRAFT_VERSION="$version" MACOSX_DEPLOYMENT_TARGET=11.0
cd "$src"
[[ "$(rustc -vV | sed -n 's/^host: //p')" == aarch64-apple-darwin ]] || fail 'Expected a native Apple-silicon Rust compiler'
cargo build --locked --release --target aarch64-apple-darwin -p fieldcraft -p fieldcraft-cli
target=$(cargo metadata --no-deps --locked --format-version=1 | perl -MJSON::PP -0777 -ne 'print decode_json($_)->{target_directory}')
mkdir -p "$contents/MacOS" "$contents/Resources/Licenses"
for name in fieldcraft fieldcraft-cli; do
    binary="$target/aarch64-apple-darwin/release/$name"
    [[ "$(lipo -archs "$binary")" == arm64 && "$("$binary" --version)" == "$name $version" ]] || fail "Unexpected native $name"
    bundle_name=$name
    if [[ "$name" == fieldcraft ]]; then bundle_name=FieldCraft; fi
    cp "$binary" "$contents/MacOS/$bundle_name"
done
"$contents/MacOS/fieldcraft-cli" commands --json > "$work/commands.json"
perl -MJSON::PP -0777 -ne '
    my %c=map {$_->{id} => $_} @{decode_json($_)};
    exit !($c{"results.external.deformed"} && $c{"results.external.playback"}
           && $c{"results.external.series"}{schema}{properties}{deformation_scale}
           && $c{"results.external.contour"}{schema}{properties}{show}{default})
' "$work/commands.json" || fail 'Required external series and deformation commands missing'
cp assets/app-icon/icon.icns "$contents/Resources/FieldCraft.icns"
cp assets/app-icon/Assets.car "$contents/Resources/Assets.car"
cp LICENSE-APACHE LICENSE-MIT NOTICE README.md "$contents/Resources/Licenses/"
cp "$licences" "$contents/Resources/Licenses/THIRD-PARTY-LICENSES.txt"
id=io.github.tangencylabs.fieldcraft
sed -e "s/@APP_ID@/$id/g" -e "s/@VERSION@/$version/g" -e 's/@SHORT_VERSION@/0.8.1/g' \
    -e "s/@BUILD_SHA@/$revision/g" -e "s/@MIN_MACOS@/$MACOSX_DEPLOYMENT_TARGET/g" packaging/macos/Info.plist.in > "$contents/Info.plist"
plutil -lint "$contents/Info.plist"
printf 'APPL????' > "$contents/PkgInfo"
codesign --force --sign - --timestamp=none --options runtime --identifier "$id.cli" "$contents/MacOS/fieldcraft-cli"
codesign --force --sign - --timestamp=none --options runtime --entitlements packaging/macos/entitlements.plist "$contents/MacOS/FieldCraft"
codesign --force --sign - --timestamp=none --options runtime --entitlements packaging/macos/entitlements.plist "$app"
codesign --verify --strict --deep "$app"
args=(--app "$app" --arch arm64 --output "$out" --package-version 0.8.1.2 --desktop-revision "$revision")
if [[ -n "$cache" ]]; then args+=(--offline-cache "$cache"); fi
"$packager/packaging/macos/ngsolve-package.sh" "${args[@]}"
(cd "$(dirname "$out")" && shasum -a 256 "$(basename "$out")" > "$(basename "$out").sha256")
