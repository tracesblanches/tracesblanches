# syntax=docker/dockerfile:1.7
#
# Images of skimap on the cluster (arm64), built natively by the cluster's buildkitd
# (deployment script of the infrastructure repository), from the repository root:
#
#   docker buildx build --platform linux/arm64 -f deploy/Dockerfile --target server   .
#   docker buildx build --platform linux/arm64 -f deploy/Dockerfile --target pipeline .
#
# The same file cross-compiles when the build platform differs from the target (amd64
# laptop → arm64): only the build stage runs on the build platform, nothing is emulated.
#
# server:   skimap-server alone, distroless (glibc), uid 65532, no shell.
# pipeline: skimap + skimap-server (publish) + skimap-job, Debian slim (sh, flock).

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

FROM debian:trixie-slim AS pipeline
RUN groupadd -g 65532 nonroot && useradd -u 65532 -g 65532 -d /nonexistent -s /usr/sbin/nologin nonroot
COPY --from=build /out/skimap /out/skimap-server /usr/local/bin/
COPY --chmod=0755 deploy/skimap-job /usr/local/bin/skimap-job
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/skimap-job"]
