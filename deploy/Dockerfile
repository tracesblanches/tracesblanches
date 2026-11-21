# syntax=docker/dockerfile:1.7
#
# Images of skimap on the cluster (arm64), built natively by the cluster's buildkitd
# (deployment script of the infrastructure repository), from the repository root:
#
#   docker buildx build --platform linux/arm64 -f deploy/Dockerfile --target server   .
#   docker buildx build --platform linux/arm64 -f deploy/Dockerfile --target pipeline \
#     --build-context pockettts-patches=$HOME/projects/speak/app/native/patches \
#     --build-context tts-models=$HOME/projects/speak/app/models/tts .
#
# The same file cross-compiles when the build platform differs from the target (amd64
# laptop → arm64): only the build stage runs on the build platform; the pockettts stage
# runs on the target platform (emulated when cross-compiling, native on the server).
#
# server:   skimap-server alone, distroless (glibc), uid 65532, no shell.
# pipeline: skimap + skimap-server (publish) + skimap-job, Debian slim (sh, flock), and
#           the voice of the sites (#129): pockettts.cpp on the CPU, its French and
#           English models, ffmpeg (Opus).
#
# The pipeline image takes two named build contexts, read only, never copied into git:
#
#   --build-context pockettts-patches=$HOME/projects/speak/app/native/patches  # of speak
#   --build-context tts-models=$HOME/projects/speak/app/models/tts  # Pocket TTS GGUF models

ARG RUST_VERSION=1.99.1

FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-slim-trixie AS build
ARG TARGETARCH
ARG BUILDARCH
ARG RUST_VERSION
# rust-toolchain.toml asks for the Android target and clippy: not needed here, and the
# image already has this exact toolchain.
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION} CARGO_TERM_COLOR=never
RUN set -eux; \
    case "$TARGETARCH" in \
      arm64) triple=aarch64-unknown-linux-gnu; cross="gcc-aarch64-linux-gnu libc6-dev-arm64-cross"; cc=aarch64-linux-gnu-gcc ;; \
      amd64) triple=x86_64-unknown-linux-gnu; cross="gcc-x86-64-linux-gnu libc6-dev-amd64-cross"; cc=x86_64-linux-gnu-gcc ;; \
      *) echo "unsupported $TARGETARCH"; exit 1 ;; \
    esac; \
    apt-get update; \
    if [ "$TARGETARCH" = "$BUILDARCH" ]; then pkgs="gcc libc6-dev"; cc=gcc; else pkgs="gcc libc6-dev $cross"; fi; \
    apt-get install -y --no-install-recommends $pkgs; \
    rm -rf /var/lib/apt/lists/*; \
    rustup target add "$triple"; \
    printf 'TRIPLE=%s\nCC_BIN=%s\n' "$triple" "$cc" > /target.env
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    set -eux; . /target.env; \
    t=$(echo "$TRIPLE" | tr 'a-z-' 'A-Z_'); \
    export "CARGO_TARGET_${t}_LINKER=$CC_BIN" "CC_$(echo "$TRIPLE" | tr '-' '_')=$CC_BIN"; \
    cargo build --release --locked --target "$TRIPLE" -p skimap-cli -p skimap-server; \
    mkdir -p /out; \
    cp "target/$TRIPLE/release/skimap" "target/$TRIPLE/release/skimap-server" /out/

FROM gcr.io/distroless/cc-debian13:nonroot AS server
COPY --from=build /out/skimap-server /usr/local/bin/skimap-server
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/skimap-server"]
CMD ["serve"]

# pockettts.cpp for the CPU of the target (the image is built natively on the nodes of
# the cluster: GGML_NATIVE), with the patches of the speak app, by the script of the workstation
# build (crates/voice/scripts/build-pockettts.sh, BACKEND=cpu).
FROM debian:trixie-slim AS pockettts
RUN set -eux; apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates git cmake ninja-build g++ libgomp1; \
    rm -rf /var/lib/apt/lists/*
COPY crates/voice/scripts/build-pockettts.sh /usr/local/bin/build-pockettts.sh
RUN --mount=type=bind,from=pockettts-patches,target=/speak/app/native/patches \
    BACKEND=cpu SPEAK_DIR=/speak POCKETTTS_DIR=/pockettts bash /usr/local/bin/build-pockettts.sh

FROM debian:trixie-slim AS pipeline
RUN set -eux; apt-get update; \
    apt-get install -y --no-install-recommends ffmpeg libgomp1; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd -g 65532 nonroot && useradd -u 65532 -g 65532 -d /nonexistent -s /usr/sbin/nologin nonroot
COPY --from=pockettts /pockettts/build-cpu/pocket-tts /usr/local/bin/pocket-tts
COPY --from=tts-models pocket-tts-french-Q8_0.gguf pocket-tts-french-voices.gguf \
    pocket-tts-english-Q8_0.gguf pocket-tts-english-voices.gguf /opt/pocket-tts/
# skimap_voice::VoiceConfig::from_env_lang; POCKET_N_THREADS: one thread per CPU of the
# limit of the pipeline CronJobs (3), not half the cores of the node.
ENV SKIMAP_POCKETTTS_BIN=/usr/local/bin/pocket-tts \
    SKIMAP_POCKETTTS_MODEL=/opt/pocket-tts/pocket-tts-french-Q8_0.gguf \
    SKIMAP_POCKETTTS_VOICES=/opt/pocket-tts/pocket-tts-french-voices.gguf \
    SKIMAP_POCKETTTS_MODEL_EN=/opt/pocket-tts/pocket-tts-english-Q8_0.gguf \
    SKIMAP_POCKETTTS_VOICES_EN=/opt/pocket-tts/pocket-tts-english-voices.gguf \
    SKIMAP_TTS_DEVICE=CPU \
    POCKET_N_THREADS=3
COPY --from=build /out/skimap /out/skimap-server /usr/local/bin/
COPY --chmod=0755 deploy/skimap-job /usr/local/bin/skimap-job
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/skimap-job"]
