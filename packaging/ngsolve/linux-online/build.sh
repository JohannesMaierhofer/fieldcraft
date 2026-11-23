#!/bin/sh
# Repackage the verified 9def desktop from the older installer, without its solver payload.
set -eu
if [ "$#" -ne 3 ]; then
    echo "Usage: $0 original-9def.deb install-runtime.py output.deb" >&2
    exit 2
fi
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
source_deb=$(realpath "$1")
loader=$(realpath "$2")
out=$(realpath -m "$3")
loader_revision=0e3064d1bba003574f1cb3e46b0290eedc690080
loader_sha256=95e63a4dc47db5227ff56314f513d5e462dfbc3dd4c9edf77de10d861ed1e345
[ "$(sha256sum "$loader" | cut -d' ' -f1)" = "$loader_sha256" ] || { echo 'Unexpected external runtime bootstrapper SHA-256' >&2; exit 1; }
[ ! -e "$out" ] || { echo "Output exists: $out" >&2; exit 1; }
actual=$(sha256sum "$source_deb" | cut -d' ' -f1)
case "$actual" in
    59ee6878d4ba07ab5fdaae565792b2bfc6a362afb8ef160f958195c308a25657|b407e0eee1fe8371b84055da9da14ab4373ca41052b700061b026e88a6cdd80e) ;;
    *) echo 'Unexpected 9def desktop package SHA-256' >&2; exit 1 ;;
esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pkg="$work/package"
base="$pkg/opt/fieldcraft-ngsolve"
mkdir -p "$base" "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/lib/fieldcraft-ngsolve" "$pkg/usr/share/applications"
dpkg-deb --fsys-tarfile "$source_deb" | tar -xf - -C "$pkg" \
    ./opt/fieldcraft-ngsolve/fieldcraft ./opt/fieldcraft-ngsolve/fieldcraft-cli \
    ./opt/fieldcraft-ngsolve/licenses ./usr/share/icons/hicolor/128x128/apps/fieldcraft-ngsolve.png
(cd "$base" && sha256sum --check <<'BINARIES'
480745ed2eb1f2c18c9e3bcd075bc45c4d405cc16139936d3d70e0168659d779  fieldcraft
71e4e5be32d788da94e098c1e8825d8e1e8ce1a8f37261394746d0c394fcd07a  fieldcraft-cli
BINARIES
)
# Keep the 9def package's validated native runtime dependencies and add the bootstrap tools.
sed -e 's/^Package: fieldcraft-ngsolve$/Package: fieldcraft-ngsolve-network\nConflicts: fieldcraft-ngsolve\nReplaces: fieldcraft-ngsolve\nProvides: fieldcraft-ngsolve/' \
    -e 's/^Version: .*/Version: 0.8.1+ngsolve2608.9def8cf4-2/' \
    -e '/^Depends:/s/$/, python3, curl, ca-certificates, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0/' "$here/../control" > "$pkg/DEBIAN/control"
install -m 755 "$here/postinst" "$here/postrm" "$pkg/DEBIAN/"
install -m 644 "$loader" "$pkg/usr/lib/fieldcraft-ngsolve/install-runtime.py"
install -m 644 "$here/../runtime-lock.json" "$here/../SOLVER-NOTICES.txt" "$base/"
install -m 644 "$here/README.md" "$base/INSTALL.md"
install -m 755 "$here/../fieldcraft-ngsolve.sh" "$pkg/usr/bin/fieldcraft-ngsolve"
install -m 755 "$here/launch-gui" "$pkg/usr/bin/fieldcraft-ngsolve-gui"
ln -s /opt/fieldcraft-ngsolve/fieldcraft-cli "$pkg/usr/bin/fieldcraft-ngsolve-cli"
ln -s /var/lib/fieldcraft-ngsolve/current/python "$base/python"
ln -s /var/lib/fieldcraft-ngsolve/current/bridge "$base/bridge"
install -m 644 "$here/../fieldcraft-ngsolve.desktop" "$pkg/usr/share/applications/"
packager=$(git -C "$here" rev-parse --verify HEAD)
bridge_url=$(perl -MJSON::PP -0777 -ne 'print decode_json($_)->{bridge}{url}' "$base/runtime-lock.json")
bridge=${bridge_url##*/}
case "$bridge" in *[!0-9a-f]*|'') echo 'Invalid bridge revision' >&2; exit 1 ;; esac
[ "${#bridge}" -eq 40 ] || { echo 'Invalid bridge revision length' >&2; exit 1; }
printf '%s\n' 'FieldCraft desktop 9def8cf427505a7faf5fc79c014357ce7e170375' "Source .deb SHA-256 $actual" "External bridge $bridge" "Bootstrapper $loader_revision (SHA-256 $loader_sha256)" "Packager $packager" > "$base/BUILD.txt"
mkdir -p "$(dirname "$out")"
DPKG_DEB_THREADS_MAX=1 dpkg-deb --root-owner-group -Zgzip -z3 --build "$pkg" "$work/release.deb"
mv "$work/release.deb" "$out"
(cd "$(dirname "$out")" && sha256sum "$(basename "$out")" > "$(basename "$out").sha256")
