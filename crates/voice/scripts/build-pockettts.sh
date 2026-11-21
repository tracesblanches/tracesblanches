#!/usr/bin/env bash
# Build of pockettts.cpp, for the voice clips of the packs (VO3, #85): with CUDA on the
# workstation (default), on the CPU in the pipeline image (BACKEND=cpu, deploy/Dockerfile,
# #129: the catch-up builds the voice of every site).
#
# Same source tree as the speak Android app (speak/app/scripts/build_native.sh): pockettts.cpp
# at REV, its ggml submodule at GGML_REV, and every patch of speak/app/native/patches applied in
# order (*-ggml-* patches inside ggml/). The patches are read from the speak checkout, never copied.
# Output: $POCKETTTS_DIR/build-$BACKEND/pocket-tts (default ~/.cache/skimap/pockettts), outside git.
#
#   BACKEND         cuda (default) or cpu
#   SPEAK_DIR       speak checkout (default ~/projects/speak), read only
#   POCKETTTS_DIR   clone + build directory
#   CUDA_ARCH       CMAKE_CUDA_ARCHITECTURES (default: native)
#   NVCC            CUDA compiler (default /usr/local/cuda/bin/nvcc)
set -euo pipefail
SPEAK=${SPEAK_DIR:-$HOME/projects/speak}
DEST=${POCKETTTS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/skimap/pockettts}
REV=fab79722efc72f79d062de7a3a4472fc430a385d       # 2026-10-16 "ggml: fork rebase", as in speak
GGML_REV=cc5df49a6b878db2b68b9a7b45976e279594627d  # submodule pin of REV
PATCHES=$SPEAK/app/native/patches
SRC=$DEST/src
BACKEND=${BACKEND:-cuda}
B=$DEST/build-$BACKEND
mkdir -p "$DEST"
[ -d "$PATCHES" ] || { echo "FATAL: $PATCHES not found (set SPEAK_DIR)"; exit 1; }

[ -d "$SRC/.git" ] || git clone -q https://github.com/ServeurpersoCom/pockettts.cpp "$SRC"
git -C "$SRC" checkout -q -f "$REV"
git -C "$SRC" submodule update -q --init --recursive
git -C "$SRC/ggml" checkout -q -f .
git -C "$SRC" clean -q -fd
git -C "$SRC/ggml" clean -q -fd
[ "$(git -C "$SRC/ggml" rev-parse HEAD)" = "$GGML_REV" ] || { echo "FATAL: ggml is not at $GGML_REV"; exit 1; }
for p in "$PATCHES"/*.patch; do
    case "$(basename "$p")" in
        *-ggml-*) git -C "$SRC/ggml" apply "$p" ;;
        *)        git -C "$SRC" apply "$p" ;;
    esac
    echo "[patch] $(basename "$p")"
done

case "$BACKEND" in
    cuda)
        NVCC=${NVCC:-/usr/local/cuda/bin/nvcc}
        set -- -DGGML_CUDA=ON -DCMAKE_CUDA_COMPILER="$NVCC" \
            -DCMAKE_CUDA_ARCHITECTURES="${CUDA_ARCH:-native}" ;;
    # The CPU of the build host (the image is built natively on the cluster's nodes).
    cpu) set -- -DGGML_CUDA=OFF ;;
    *) echo "FATAL: BACKEND=$BACKEND (cuda or cpu)"; exit 1 ;;
esac
cmake -S "$SRC" -B "$B" -G Ninja -DCMAKE_BUILD_TYPE=Release "$@" \
    -DGGML_NATIVE=ON -DGGML_BLAS=OFF -DBUILD_SHARED_LIBS=OFF > "$DEST/cmake-configure.log"
cmake --build "$B" --config Release -j "$(nproc)" --target pocket-tts
ls -l "$B/pocket-tts"
