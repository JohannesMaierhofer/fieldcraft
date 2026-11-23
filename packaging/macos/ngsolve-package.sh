#!/bin/bash
# Package a separately built FieldCraft.app and a pinned, privately installed solver.
set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
here=$(cd "$(dirname "$0")" && pwd)
usage() { echo "Usage: $0 --app FieldCraft.app --arch x86_64|arm64 --output result.pkg [--offline-cache DIR]" >&2; exit 2; }
app=''; arch=''; out=''; cache=''
while [[ $# -gt 0 ]]; do
    case "$1" in
        --app) app=$2; shift 2 ;;
        --arch) arch=$2; shift 2 ;;
        --output) out=$2; shift 2 ;;
        --offline-cache) cache=$2; shift 2 ;;
        *) usage ;;
    esac
done
[[ "$arch" == x86_64 || "$arch" == arm64 ]] || usage
[[ -d "$app" && ${app##*/} == FieldCraft.app && "$out" == *.pkg ]] || usage
[[ ! -e "$out" ]] || { echo "Output already exists: $out" >&2; exit 1; }
if [[ -n "$cache" && ! -d "$cache" ]]; then echo "Missing offline cache" >&2; exit 1; fi
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][[:alnum:].-]+)?$ ]] || usage
executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
/usr/bin/lipo "$app/Contents/MacOS/$executable" -verify_arch "$arch"
/usr/bin/codesign --verify --strict --deep "$app"
work=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/fc-ngsolve.XXXXXXXX")
cleanup() { /bin/rm -rf "$work"; }
trap cleanup EXIT
mkdir -p "$work/scripts" "$work/apps"
cp "$here/ngsolve/$arch.lock" "$work/scripts/runtime.lock"
cp "$here/ngsolve/postinstall" "$here/ngsolve/fieldcraft-ngsolve" "$work/scripts/"
cp "$here/../ngsolve/SOLVER-NOTICES.txt" "$work/scripts/NOTICE.txt"
chmod 755 "$work/scripts/postinstall"
printf '%s\n' "$arch" > "$work/scripts/arch"
if [[ -n "$cache" ]]; then
    mkdir "$work/scripts/offline"
    while read -r digest url; do
        name=${url##*/}; name=${name//%2B/+}
        if [[ "$url" == https://codeload.github.com/* ]]; then name="bridge-$name.zip"; fi
        asset="$cache/$name"
        [[ $(/usr/bin/shasum -a 256 "$asset" | /usr/bin/awk '{print $1}') == "$digest" ]] || { echo "Checksum failed: $name" >&2; exit 1; }
        cp -c "$asset" "$work/scripts/offline/$name" 2>/dev/null || cp "$asset" "$work/scripts/offline/$name"
    done < "$work/scripts/runtime.lock"
fi
/usr/bin/pkgbuild --nopayload --scripts "$work/scripts" --identifier org.tangency-labs.fieldcraft.ngsolve-runtime --version "${version}" "$work/runtime.pkg"
# Do not relocate or overwrite an unrelated app elsewhere in the user's home.
# APFS clones keep the staging footprint small; ditto handles other volumes.
cp -cR "$app" "$work/apps/FieldCraft.app" 2>/dev/null || ditto "$app" "$work/apps/FieldCraft.app"
/usr/bin/pkgbuild --analyze --root "$work/apps" "$work/components.plist"
/usr/libexec/PlistBuddy -c 'Set :0:BundleIsRelocatable false' "$work/components.plist"
/usr/bin/pkgbuild --root "$work/apps" --component-plist "$work/components.plist" --install-location /Applications \
    --identifier org.tangency-labs.fieldcraft.app --version "$version" --ownership recommended "$work/app.pkg"
cat > "$work/distribution.xml" <<XML
<installer-gui-script minSpecVersion="2">
  <title>FieldCraft with NGSolve</title>
  <options customize="never" hostArchitectures="$arch"/>
  <domains enable_anywhere="false" enable_currentUserHome="false" enable_localSystem="true"/>
  <choices-outline><line choice="runtime"/><line choice="application"/></choices-outline>
  <choice id="runtime" visible="false"><pkg-ref id="org.tangency-labs.fieldcraft.ngsolve-runtime"/></choice>
  <choice id="application" visible="false"><pkg-ref id="org.tangency-labs.fieldcraft.app"/></choice>
  <pkg-ref id="org.tangency-labs.fieldcraft.ngsolve-runtime" version="$version">runtime.pkg</pkg-ref>
  <pkg-ref id="org.tangency-labs.fieldcraft.app" version="$version">app.pkg</pkg-ref>
</installer-gui-script>
XML
sign=(); if [[ -n "${MACOS_INSTALLER_IDENTITY:-}" ]]; then sign=(--sign "$MACOS_INSTALLER_IDENTITY"); fi
/usr/bin/productbuild --distribution "$work/distribution.xml" --package-path "$work" ${sign[@]+"${sign[@]}"} "$work/product.pkg"
mkdir -p "$(dirname "$out")"
mv "$work/product.pkg" "$out"
/usr/bin/shasum -a 256 "$out"
