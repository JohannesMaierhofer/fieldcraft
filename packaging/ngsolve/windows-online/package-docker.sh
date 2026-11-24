#!/usr/bin/env bash
set -euo pipefail

revision=8b495f31389512c7f599ef17b5ee3a4d0cc39bc9
bridge=74393901cf1b6fe70e5956003f948c9fe5eee889
lock_sha=f80a8787de6f5483a01084a324cd2812d70fd772c86dc6c8719dc5c738b88868
case "${1:-}" in
    x64) target=x86_64-pc-windows-msvc; machine=IMAGE_FILE_MACHINE_AMD64 ;;
    arm64) target=aarch64-pc-windows-msvc; machine=IMAGE_FILE_MACHINE_ARM64 ;;
    *) echo 'Select x64 or arm64.' >&2; exit 2 ;;
esac
arch=$1
export FIELDCRAFT_VERSION=0.8.1-ngsolve.r1 FIELDCRAFT_BUILD_SHA=$revision
export CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-feature=+crt-static'
recipe=/opt/fieldcraft
source=/build/fieldcraft
out=/out
archive="FieldCraft-NGSolve-0.8.1-r2-windows-$arch-network.exe"
mkdir -p "$out" "$CARGO_HOME" "$XWIN_CACHE_DIR"
if [[ -e "$out/$archive" ]]; then
    echo "Output already exists: $out/$archive" >&2
    exit 1
fi
if [[ ! -d "$source/.git" ]]; then
    git init -q "$source"
    git -C "$source" fetch --depth=1 https://github.com/Maierhofer-Technology/fieldcraft.git "$revision"
    git -C "$source" checkout -q --detach FETCH_HEAD
fi
cd "$source"
[[ $(git rev-parse HEAD) == "$revision" && -z $(git status --porcelain) ]] || {
    echo 'A clean, revision-matched FieldCraft source checkout is required.' >&2; exit 1;
}
echo "$lock_sha  packaging/ngsolve/runtime-lock.json" | sha256sum -c -
jq -e --arg bridge "$bridge" '.bridge.url | endswith("/"+$bridge)' packaging/ngsolve/runtime-lock.json >/dev/null
[[ $(uname -s) == Linux ]]
case $(uname -m) in
    x86_64) host=linux/amd64 ;;
    aarch64) host=linux/arm64 ;;
    *) echo 'The builder image supports Linux AMD64 or ARM64 hosts.' >&2; exit 1 ;;
esac
if [[ $arch == arm64 ]]; then
    export PATH="/opt/fieldcraft/arm64-tools:$PATH"
fi
cargo xwin build --release --locked --target "$target" -p fieldcraft -p fieldcraft-cli
cargo run --release --locked -p xtask -- licences

bin="$CARGO_TARGET_DIR/$target/release"
stage=$(mktemp -d "$out/.$arch-payload.XXXXXXXX")
tmp_output=$(mktemp -d "$out/.$arch-installer.XXXXXXXX")
trap 'rm -rf -- "$stage" "$tmp_output"' EXIT
mkdir "$stage/licenses"
for name in fieldcraft.exe fieldcraft-cli.exe; do
    llvm-readobj --file-headers "$bin/$name" > "$tmp_output/$name.headers"
    grep -Fq "Machine: $machine (" "$tmp_output/$name.headers"
    grep -aFq 'results.external.deformed' "$bin/$name"
    cp "$bin/$name" "$stage/$name"
done
cp LICENSE-MIT LICENSE-APACHE NOTICE THIRD-PARTY-LICENSES.txt "$stage/licenses/"
cp /usr/share/doc/nsis/copyright "$stage/licenses/NSIS.txt"
cp packaging/ngsolve/{runtime-lock.json,fieldcraft-ngsolve.cmd,SOLVER-NOTICES.txt} "$stage/"
cp "$recipe"/{install.ps1,INSTALL.txt} "$stage/"
digest() { sha256sum "$1" | cut -d' ' -f1; }
identity=$(jq -ncS --arg arch "$arch" --arg bridge "$bridge" --arg sha "$revision" \
    --arg cli "$(digest "$stage/fieldcraft-cli.exe")" --arg gui "$(digest "$stage/fieldcraft.exe")" \
    --arg lock "$lock_sha" '{arch:$arch,bridge:$bridge,desktop:$sha,files:{"fieldcraft-cli.exe":$cli,"fieldcraft.exe":$gui},installer_revision:"r2",runtime_lock_sha256:$lock}')
generation=$(printf '%s' "$identity" | openssl dgst -sha256 -binary | base32 | tr -d '=\n' | tr 'A-Z' 'a-z')
jq --arg generation "$generation" '. + {generation:$generation}' <<< "$identity" > "$stage/identity.json"
makensis -V2 "-DARCH=$arch" "-DPAYLOAD=$stage" "-DOUTPUT=$tmp_output/$archive" "$recipe/setup.nsi"
seven=$(command -v 7zz || command -v 7z)
prefix='$PLUGINSDIR/payload/'
"$seven" l -slt -ba "$tmp_output/$archive" | awk -v p="$prefix" '/^Path = /{n=substr($0,8); if(index(n,p)!=1) exit 1; print substr(n,length(p)+1)}' | sort > "$tmp_output/listed"
(cd "$stage" && find . -type f -printf '%P\n' | sort) > "$tmp_output/staged"
diff -u "$tmp_output/staged" "$tmp_output/listed"
[[ $(wc -l < "$tmp_output/staged") == 13 ]]
while IFS= read -r name; do
    read -r unpacked _ < <("$seven" x -so -spd "$tmp_output/$archive" "$prefix$name" | sha256sum)
    [[ $unpacked == "$(digest "$stage/$name")" ]] || { echo "Incorrect archive contents: $name" >&2; exit 1; }
    jq -nc --arg key "$name" --arg value "$unpacked" '{key:$key,value:$value}'
done < "$tmp_output/staged" > "$tmp_output/files"
sum=$(digest "$tmp_output/$archive")
jq -n --arg installer "$archive" --arg sha "$sum" --arg source "$revision" --arg target "$target" \
    --arg rust "$(rustc --version)" --arg xwin "$(cargo xwin --version)" --arg llvm "$(llvm-config --version)" \
    --arg nsis "$(makensis -VERSION)" --arg container "$(hostname)" --arg host "$host" --argjson bytes "$(stat -c%s "$tmp_output/$archive")" \
    --slurpfile receipt "$stage/identity.json" --slurpfile files "$tmp_output/files" \
    '{installer:$installer,sha256:$sha,source:$source,target:$target,bytes:$bytes,identity:$receipt[0],
      build_host:($host+" Docker"),container:$container,tools:{rust:$rust,cargo_xwin:$xwin,llvm:$llvm,nsis:$nsis},
      native_feature_catalog_checked:false,payload:($files|from_entries)}' > "$tmp_output/receipt.json"
printf '%s  %s\n' "$sum" "$archive" > "$tmp_output/$archive.sha256"
mv "$tmp_output/$archive" "$tmp_output/$archive.sha256" "$out/"
mv "$tmp_output/receipt.json" "$out/receipt-$arch.json"
printf '%s  %s\n' "$sum" "$archive"
