#!/usr/bin/env bash
# Build the pinned Linux desktop, generate its license inventory, and run the native package recipe.
set -euo pipefail
export LC_ALL=C
input=$(realpath "${FC_INPUT_DIR:-/inputs}")
cache=$(realpath -m "${FC_WORK_ROOT:-/work}")
out=$(realpath -m "${FC_OUTPUT_DIR:-/out}")
recipe=$input/recipe
desktop=8b495f31389512c7f599ef17b5ee3a4d0cc39bc9
loader_revision=0e3064d1bba003574f1cb3e46b0290eedc690080
checkpoint_sha=7023134c4c9db04cb326e94cfb679adde3f2b76705729f4f16bd8e30e91a770f
expected_compiler=59807616e1fa2540724bfbac14d7976d7e4a3860
# Keep the release compiler active inside the desktop checkout as well as at image startup.
export RUSTUP_TOOLCHAIN=1.95.0-x86_64-unknown-linux-gnu RUSTUP_AUTO_INSTALL=0
. /etc/os-release
[[ "$ID:$VERSION_CODENAME" == debian:bookworm && $(dpkg --print-architecture) == amd64 && $(uname -m) == x86_64 ]]
[[ $(rustc -vV | sed -n 's/^host: //p') == x86_64-unknown-linux-gnu ]]
[[ $(rustc -vV | sed -n 's/^commit-hash: //p') == "$expected_compiler" ]]
mkdir -p "$cache" "$out"
exec 9>"$cache/.fieldcraft-docker-build.lock"
flock -n 9 || { echo "Another build owns this cache" >&2; exit 1; }
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory GIT_CONFIG_VALUE_0="$recipe"
export CARGO_HOME="$cache/cargo" CARGO_TARGET_DIR="$cache/target" CARGO_NET_GIT_FETCH_WITH_CLI=true
export CARGO_BUILD_JOBS="${FC_BUILD_JOBS:-1}" CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_OPT_LEVEL=1 CARGO_PROFILE_RELEASE_CODEGEN_UNITS=64
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=symbols
if [[ -f /cargo-checkpoint.tar.zst && ! -e "$CARGO_HOME" && ! -e "$CARGO_TARGET_DIR" ]]; then
    echo "$checkpoint_sha  /cargo-checkpoint.tar.zst" | sha256sum -c -
    # Import only published Cargo inputs and unfinished dependency artifacts.
    tar --zstd -xf /cargo-checkpoint.tar.zst -C "$cache" cargo target
fi
src="$cache/src/fieldcraft"
if [[ ! -d "$src/.git" ]]; then
    git init -q "$src"
    git -C "$src" index-pack --stdin < "$input/desktop.pack" >/dev/null
    echo "$desktop" > "$src/.git/shallow"
    git -C "$src" checkout --quiet --detach "$desktop"
fi
[[ $(git -C "$src" rev-parse HEAD) == "$desktop" ]]
git -C "$src" diff --exit-code HEAD --
cd "$src"
[[ $(rustc -vV | sed -n 's/^commit-hash: //p') == "$expected_compiler" ]]
cargo fetch --locked
export CARGO_NET_OFFLINE=true
cargo xtask licences
tools_dir="$cache/runtime-inputs/$loader_revision"
mkdir -p "$tools_dir"
loader="$tools_dir/install-runtime.py"
if [[ ! -f "$loader" ]]; then
    curl -fsSL --retry 3 "https://raw.githubusercontent.com/JohannesMaierhofer/fieldcraft-ngsolve/$loader_revision/packaging/debian/install-runtime.py" -o "$loader.download"
    echo "95e63a4dc47db5227ff56314f513d5e462dfbc3dd4c9edf77de10d861ed1e345  $loader.download" | sha256sum -c -
    mv "$loader.download" "$loader"
fi
# Compile and link the two final applications in this container on every release run.
cargo clean --release -p fieldcraft -p fieldcraft-cli
package="$out/FieldCraft-NGSolve-0.8.1-r2-linux-x64-network.deb"
"$recipe/packaging/ngsolve/linux-online/build-r2.sh" "$src" "$loader" "$src/THIRD-PARTY-LICENSES.txt" "$package"
(cd "$out" && sha256sum -c "${package##*/}.sha256")
{
    echo "Docker Official Image rust:1.95.0-slim-bookworm (Linux amd64 manifest)"
    echo "sha256:6f9e63259f12e1e599296f5ecfed2bae46de4af0ee0525dd8b89c046e236d5c5"
    echo "Desktop $desktop"
    echo "Packager $(git -C "$recipe" rev-parse HEAD)"
    cargo -V; rustc -vV
    echo "Linker driver $(command -v cc) ($(cc -dumpmachine))"
    cc --version | sed -n '1p'
    env | grep -E '^CARGO_(BUILD_JOBS|PROFILE_RELEASE_|INCREMENTAL|NET_OFFLINE)' | sort
} > "$out/docker-build-environment.txt"
dpkg-query -W -f='${binary:Package}\t${Version}\n' > "$out/docker-debian-packages.txt"
