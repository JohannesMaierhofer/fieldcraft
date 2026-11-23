# Docker Official Image rust:1.95.0-slim-bookworm, selected Linux amd64 manifest.
FROM docker.io/library/rust@sha256:6f9e63259f12e1e599296f5ecfed2bae46de4af0ee0525dd8b89c046e236d5c5
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    binutils build-essential ca-certificates cmake curl dpkg-dev git libasound2-dev \
    libfontconfig-dev libgtk-3-dev libssl-dev libudev-dev libvulkan-dev libxkbcommon-x11-dev \
    perl pkg-config unzip util-linux zstd && rm -rf /var/lib/apt/lists/*
