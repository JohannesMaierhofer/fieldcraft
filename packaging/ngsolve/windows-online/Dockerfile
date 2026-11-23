FROM rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07

# clang-cl, the LLVM resource compiler and lld produce Windows PE files on Linux.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates clang cmake curl git jq lld llvm nasm ninja-build nsis \
      openssl 7zip python3-venv && rm -rf /var/lib/apt/lists/* \
    && ln -s "$(llvm-config --bindir)" /opt/llvm-tools \
    && rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc

# The wheel contains the cargo-xwin binary. The compiler runs inside this image.
COPY cargo-xwin.lock /tmp/xwin.lock
RUN python3 -m venv /opt/xwin \
    && /opt/xwin/bin/pip install --no-deps --require-hashes -r /tmp/xwin.lock \
    && rm /tmp/xwin.lock
ENV PATH="/opt/xwin/bin:/opt/llvm-tools:${PATH}" RUSTUP_TOOLCHAIN=1.99.1 \
    CARGO_HOME=/cache/cargo XWIN_CACHE_DIR=/cache/xwin \
    CARGO_TARGET_DIR=/cache/target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 \
    CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_OPT_LEVEL=1 \
    CARGO_PROFILE_RELEASE_LTO=false FIELDCRAFT_TELEMETRY=0
RUN cargo xwin --version && command -v llvm-rc && command -v lld-link && makensis -VERSION
WORKDIR /build
COPY setup.nsi install.ps1 INSTALL.txt package-docker.sh /opt/fieldcraft/
ENTRYPOINT ["/bin/bash", "/opt/fieldcraft/package-docker.sh"]
