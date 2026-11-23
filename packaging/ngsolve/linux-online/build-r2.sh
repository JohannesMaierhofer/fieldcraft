#!/bin/sh
# Build the r2 replacement directly from its pinned source on a native amd64 Linux userspace.
set -eu
export LC_ALL=C
[ "$#" -eq 4 ] || { echo "Usage: $0 source-checkout install-runtime.py THIRD-PARTY-LICENSES.txt output.deb" >&2; exit 2; }
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
src=$(realpath "$1")
loader=$(realpath "$2")
licences=$(realpath "$3")
out=$(realpath -m "$4")
revision=8b495f31389512c7f599ef17b5ee3a4d0cc39bc9
deb_version=0.8.1+ngsolve2608.r2.8b495f3-1
loader_revision=0e3064d1bba003574f1cb3e46b0290eedc690080
loader_sha=95e63a4dc47db5227ff56314f513d5e462dfbc3dd4c9edf77de10d861ed1e345
digest() { sha256sum "$1" | cut -d' ' -f1; }
fail() { echo "$*" >&2; exit 1; }
[ "$(git -C "$src" rev-parse HEAD)" = "$revision" ] || fail 'Unexpected desktop revision'
git -C "$src" diff-index --quiet HEAD -- || fail 'Modified desktop source'
[ -z "$(git -C "$src" ls-files --others --exclude-standard)" ] || fail 'Untracked desktop source'
[ "$(digest "$loader")" = "$loader_sha" ] || fail 'Unexpected runtime bootstrapper'
[ "$(digest "$licences")" = 1775ff2eb03e65ba2ebc7cbac1b45c525d55fed73c4e54b4dcd687660206ae3b ] || fail 'Unexpected Rust licence inventory'
[ "$(dpkg --print-architecture)" = amd64 ] || fail 'Expected Debian amd64'
[ ! -e "$out" ] && [ ! -e "$out.sha256" ] || fail 'Output already exists'
dpkg --compare-versions "$deb_version" gt 0.8.1+ngsolve2608.9def8cf4-2
cd "$src"
[ "$(rustc -vV | sed -n 's/^host: //p')" = x86_64-unknown-linux-gnu ] || fail 'Expected native Linux x86_64 Rust'
export FIELDCRAFT_BUILD_SHA="$revision" FIELDCRAFT_VERSION=0.8.1-ngsolve.r1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo build --release --locked --message-format=json-render-diagnostics -p fieldcraft -p fieldcraft-cli > "$work/cargo.jsonl"
built_executable() {
    perl -MJSON::PP -e '
        use strict;
        use warnings;
        my $name = shift;
        my %paths;
        while (<>) {
            my $message = decode_json($_);
            next unless $message->{reason} eq "compiler-artifact" && defined $message->{executable};
            my $target = $message->{target};
            next unless $target->{name} eq $name && grep { $_ eq "bin" } @{$target->{kind}};
            $paths{$message->{executable}} = 1;
        }
        die "Expected one executable path from Cargo for $name\n" unless keys(%paths) == 1;
        print keys %paths;
    ' "$1" "$work/cargo.jsonl"
}
gui=$(built_executable fieldcraft)
cli=$(built_executable fieldcraft-cli)
"$here/verify-r2.sh" "$gui" "$cli"
pkg="$work/package"
base="$pkg/opt/fieldcraft-ngsolve"
mkdir -p "$base/licenses" "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/lib/fieldcraft-ngsolve" "$pkg/usr/share/applications" "$pkg/usr/share/icons/hicolor/128x128/apps" "$work/debian"
install -m 755 "$gui" "$base/fieldcraft"
install -m 755 "$cli" "$base/fieldcraft-cli"
install -m 644 "$src/LICENSE-APACHE" "$src/LICENSE-MIT" "$src/NOTICE" "$src/README.md" "$base/licenses/"
install -m 644 "$licences" "$base/licenses/THIRD-PARTY-LICENSES.txt"
install -m 644 "$src/assets/app-icon/icon.iconset/icon_128x128.png" "$pkg/usr/share/icons/hicolor/128x128/apps/fieldcraft-ngsolve.png"

# Obtain required versions for every linked system library using the build host's Debian symbols.
cd "$work"
printf 'Source: fieldcraft-ngsolve\n\nPackage: fieldcraft-ngsolve-network\nArchitecture: amd64\n' > debian/control
dpkg-shlibdeps -O -e"$base/fieldcraft" -e"$base/fieldcraft-cli" > shlibdeps
readelf --version-info -W "$base/fieldcraft" "$base/fieldcraft-cli" > version-info
glibc=$(sed -nE 's/.*Name: GLIBC_([0-9]+(\.[0-9]+)+)([[:space:]].*)?$/\1/p' version-info | sort -Vu | tail -1)
[ -n "$glibc" ] || fail 'Cannot determine ELF GLIBC version requirements'
# Keep the solver runtime baseline and raise it for either linked executable if required.
base_glibc=2.35
if dpkg --compare-versions "$glibc" gt "$base_glibc"; then base_glibc="$glibc"; fi
sed -e 's/^Package: fieldcraft-ngsolve$/Package: fieldcraft-ngsolve-network\nConflicts: fieldcraft-ngsolve\nReplaces: fieldcraft-ngsolve\nProvides: fieldcraft-ngsolve/' \
    -e "s/^Version: .*/Version: $deb_version/" \
    -e "s/libc6 (>= 2.35)/libc6 (>= $base_glibc)/" \
    -e '/^Depends:/s/$/, python3, curl, ca-certificates, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0/' \
    "$here/../control" > template
SHLIBDEPS=$(sed -n 's/^shlibs:Depends=//p' shlibdeps)
[ -n "$SHLIBDEPS" ] || fail 'No linked Debian packages resolved'
export SHLIBDEPS
# An additional versioned relationship can only narrow the accepted versions of a package.
perl -pe 'if (/^Depends:/) { chomp; my %seen; my @deps = (split(/, /, substr($_, 9)), split(/, /, $ENV{SHLIBDEPS})); $_ = "Depends: ".join(", ", grep { !$seen{$_}++ } @deps)."\n"; }' template > "$pkg/DEBIAN/control"
install -m 755 "$here/postinst" "$here/postrm" "$pkg/DEBIAN/"
install -m 644 "$loader" "$pkg/usr/lib/fieldcraft-ngsolve/install-runtime.py"
install -m 644 "$here/../runtime-lock.json" "$here/../SOLVER-NOTICES.txt" "$base/"
install -m 644 "$here/R2.md" "$base/INSTALL.md"
install -m 755 "$here/../fieldcraft-ngsolve.sh" "$pkg/usr/bin/fieldcraft-ngsolve"
install -m 755 "$here/launch-gui" "$pkg/usr/bin/fieldcraft-ngsolve-gui"
ln -s /opt/fieldcraft-ngsolve/fieldcraft-cli "$pkg/usr/bin/fieldcraft-ngsolve-cli"
ln -s /var/lib/fieldcraft-ngsolve/current/python "$base/python"
ln -s /var/lib/fieldcraft-ngsolve/current/bridge "$base/bridge"
install -m 644 "$here/../fieldcraft-ngsolve.desktop" "$pkg/usr/share/applications/"
packager=$(git -C "$here" rev-parse --verify HEAD)
bridge=$(perl -MJSON::PP -0777 -ne 'my $u=decode_json($_)->{bridge}{url}; $u =~ m{/([0-9a-f]{40})$} or die "Unexpected bridge URL"; print $1' "$base/runtime-lock.json")
{
    printf '%s\n' "Desktop $revision" "Desktop version $FIELDCRAFT_VERSION" "Package version $deb_version" "Packager $packager" "Bridge $bridge" "Bootstrapper $loader_revision ($loader_sha)" "Maximum GLIBC symbol version $glibc" "Native dependencies $SHLIBDEPS"
    rustc -vV
    (cd "$base" && sha256sum fieldcraft fieldcraft-cli runtime-lock.json SOLVER-NOTICES.txt)
} > "$base/BUILD.txt"
mkdir -p "$(dirname "$out")"
DPKG_DEB_THREADS_MAX=1 dpkg-deb --root-owner-group -Zgzip -z3 --build "$pkg" "$work/release.deb"
mv "$work/release.deb" "$out"
(cd "$(dirname "$out")" && sha256sum "$(basename "$out")" > "$(basename "$out").sha256")
