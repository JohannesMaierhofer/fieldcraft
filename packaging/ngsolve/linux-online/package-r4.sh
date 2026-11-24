#!/bin/bash
# Package the binaries from the same checkout as the runtime lock and notices.
set -euo pipefail
[[ $# == 1 ]] || { echo "Usage: $0 release-binary-directory" >&2; exit 2; }
bin=$(realpath "$1")
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
revision=$(git -C "$repo" rev-parse HEAD)
[[ $(dpkg --print-architecture) == amd64 && $FIELDCRAFT_BUILD_SHA == "$revision" ]]
for name in fieldcraft fieldcraft-cli; do
    [[ $("$bin/$name" --version) == "$name $FIELDCRAFT_VERSION" ]]
done
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pkg="$work/pkg"
base="$pkg/opt/fieldcraft-ngsolve"
mkdir -p "$base/licenses" "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" "$pkg/usr/share/icons/hicolor/128x128/apps"
install -m 755 "$bin/fieldcraft" "$bin/fieldcraft-cli" "$base/"
install -m 644 "$repo"/{LICENSE-APACHE,LICENSE-MIT,NOTICE,THIRD-PARTY-LICENSES.txt} "$base/licenses/"
install -m 644 "$here/../"{runtime-lock.json,SOLVER-NOTICES.txt} "$base/"
install -m 644 "$repo/assets/app-icon/icon.iconset/icon_128x128.png" "$pkg/usr/share/icons/hicolor/128x128/apps/fieldcraft-ngsolve.png"
install -m 644 "$here/../fieldcraft-ngsolve.desktop" "$pkg/usr/share/applications/"
install -m 755 "$here/../fieldcraft-ngsolve.sh" "$pkg/usr/bin/fieldcraft-ngsolve"
ln -s /opt/fieldcraft-ngsolve/fieldcraft "$pkg/usr/bin/fieldcraft-ngsolve-gui"
ln -s /opt/fieldcraft-ngsolve/fieldcraft-cli "$pkg/usr/bin/fieldcraft-ngsolve-cli"
ln -s /var/lib/fieldcraft-ngsolve/current/python "$base/python"
install -m 755 "$here/install-r4" "$pkg/DEBIAN/postinst"
install -m 755 "$here/postrm" "$pkg/DEBIAN/postrm"
cd "$work"
mkdir debian
printf 'Source: fieldcraft-ngsolve\n\nPackage: fieldcraft-ngsolve-network\nArchitecture: amd64\n' > debian/control
deps=$(dpkg-shlibdeps -O -e"$base/fieldcraft" -e"$base/fieldcraft-cli")
[[ $deps == shlibs:Depends=* ]]
sed -e 's/^Package: .*/Package: fieldcraft-ngsolve-network\nConflicts: fieldcraft-ngsolve\nReplaces: fieldcraft-ngsolve\nProvides: fieldcraft-ngsolve/' \
    -e 's/^Version: .*/Version: 0.8.1+ngsolve2608.r4-1/' \
    -e "/^Depends:/s/\$/, curl, ca-certificates, jq, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0, ${deps#shlibs:Depends=}/" \
    "$here/../control" > "$pkg/DEBIAN/control"
printf 'Desktop %s\nVersion %s\n' "$revision" "$FIELDCRAFT_VERSION" > "$base/BUILD.txt"
cd "$repo"
mkdir -p dist/ngsolve
out=dist/ngsolve/FieldCraft-NGSolve-0.8.1-r4-linux-x64-network.deb
dpkg-deb --root-owner-group -Zgzip -z3 --build "$pkg" "$out"
cp "$base/SOLVER-NOTICES.txt" dist/ngsolve/
(cd dist/ngsolve && sha256sum "$(basename "$out")" > "$(basename "$out").sha256")
