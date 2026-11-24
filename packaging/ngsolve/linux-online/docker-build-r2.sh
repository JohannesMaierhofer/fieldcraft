#!/usr/bin/env bash
# Package R2 from committed public inputs. Run from any Git checkout containing this recipe.
set -euo pipefail
if [[ $# -ne 1 ]]; then echo "Usage: $0 OUTPUT_DIRECTORY" >&2; exit 2; fi
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
packager=$(git -C "$repo" rev-parse HEAD)
desktop=8b495f31389512c7f599ef17b5ee3a4d0cc39bc9
src_repo=$repo
image=fieldcraft-r2-linux-amd64:rust-1.95.0-bookworm
volume=${FC_DOCKER_VOLUME:-fieldcraft-r2-linux-amd64-8b495f3}
jobs=${FC_BUILD_JOBS:-1}
[[ "$jobs" =~ ^[1-9][0-9]*$ ]] || { echo "FC_BUILD_JOBS must be positive" >&2; exit 2; }
[[ $(docker info --format '{{.OSType}}') == linux ]] || { echo "Select a Linux Docker Engine" >&2; exit 1; }
git -C "$repo" diff --quiet HEAD -- packaging/ngsolve || { echo "Commit packaging edits before building a release" >&2; exit 1; }
mkdir -p "$1"
output=$(cd "$1" && pwd)
stage=$(mktemp -d)
context=$stage/build-context
mkdir "$context"
trap 'rm -rf -- "$stage"' EXIT

# Use one commit and its trees and blobs. Other refs, histories and local files stay outside Docker.
if ! git -C "$repo" cat-file -e "$desktop^{commit}" 2>/dev/null; then
    src_repo=$stage/source.git
    git init -q --bare "$src_repo"
    git -C "$src_repo" fetch --quiet --depth=1 --no-tags https://github.com/Maierhofer-Technology/fieldcraft.git "$desktop"
fi
{
    git -C "$src_repo" rev-parse "$desktop" "$desktop^{tree}"
    git -C "$src_repo" ls-tree -rt --format='%(objectname)' "$desktop"
} | sort -u | git -C "$src_repo" pack-objects --stdout > "$context/desktop.pack"
{
    git -C "$repo" rev-parse "$packager" "$packager^{tree}" "$packager:packaging"
    git -C "$repo" ls-tree -rt --format='%(objectname)' "$packager" -- packaging/ngsolve
} | sort -u | git -C "$repo" pack-objects --stdout > "$context/recipe.pack"
git -C "$repo" show "$packager:packaging/ngsolve/linux-online/docker/Dockerfile" > "$context/Dockerfile"

# Keep both snapshots inside the image so host cleanup cannot remove a running build's inputs.
docker build --platform linux/amd64 --pull --tag "$image" --iidfile "$stage/image-id" \
    --build-arg "FC_PACKAGING_REVISION=$packager" --label "org.opencontainers.image.revision=$packager" "$context"
image_id=$(cat "$stage/image-id")
docker version --format 'Docker Engine {{.Server.Version}}' > "$output/docker-builder.txt"
docker info --format 'Docker host {{.OSType}}/{{.Architecture}}' >> "$output/docker-builder.txt"
docker image inspect --format 'Build image {{.Id}}' "$image_id" >> "$output/docker-builder.txt"
docker volume create "$volume" >/dev/null
mounts=(--mount "type=bind,src=$output,dst=/out" --mount "type=volume,src=$volume,dst=/work")
if [[ -n ${FC_CARGO_CHECKPOINT:-} ]]; then
    [[ -f "$FC_CARGO_CHECKPOINT" ]] || { echo "Supplied Cargo checkpoint does not exist" >&2; exit 1; }
    cache_file=$(cd "$(dirname "$FC_CARGO_CHECKPOINT")" && pwd)/$(basename "$FC_CARGO_CHECKPOINT")
    mounts+=(--mount "type=bind,src=$cache_file,dst=/cargo-checkpoint.tar.zst,readonly")
fi
docker run --rm --platform linux/amd64 "${mounts[@]}" \
    -e FC_INPUT_DIR=/inputs -e FC_OUTPUT_DIR=/out -e FC_WORK_ROOT=/work -e FC_BUILD_JOBS="$jobs" \
    "$image_id" bash /inputs/recipe/packaging/ngsolve/linux-online/docker/build-in-container.sh
