#!/usr/bin/env bash
# =============================================================================
# build-wasi.sh
#
# Build the Tectonic WASI reactor library.
#
# Prerequisites:
#   1. Run wasi-deps/setup-toolchain.sh to install wasi-sdk + Rust target
#   2. Run wasi-deps/build-wasi-deps.sh to build external C/C++ libraries
#
# Output: target/wasm32-wasip1/release/tectonic_wasi.wasm
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASI_SDK_PATH="${WASI_SDK_PATH:-/opt/wasi-sdk}"
SYSROOT="${SCRIPT_DIR}/wasi-deps/sysroot"

# ---------------------------------------------------------------------------
# Validate prerequisites
# ---------------------------------------------------------------------------
if [ ! -x "${WASI_SDK_PATH}/bin/clang" ]; then
    echo "ERROR: wasi-sdk not found at ${WASI_SDK_PATH}" >&2
    echo "       Run: wasi-deps/setup-toolchain.sh" >&2
    exit 1
fi

if [ ! -d "${SYSROOT}/lib" ]; then
    echo "ERROR: WASI sysroot not found at ${SYSROOT}/lib" >&2
    echo "       Run: wasi-deps/build-wasi-deps.sh" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# C/C++ toolchain for the cc crate
# ---------------------------------------------------------------------------
export CC_wasm32_wasip1="${WASI_SDK_PATH}/bin/clang"
export CXX_wasm32_wasip1="${WASI_SDK_PATH}/bin/clang++"
export AR_wasm32_wasip1="${WASI_SDK_PATH}/bin/llvm-ar"
SJLJ_STUB="${SCRIPT_DIR}/wasi-deps/sjlj-stub"
export CFLAGS_wasm32_wasip1="--sysroot=${WASI_SDK_PATH}/share/wasi-sysroot -isystem ${SJLJ_STUB} -I${SYSROOT}/include -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL"
export CXXFLAGS_wasm32_wasip1="${CFLAGS_wasm32_wasip1} -fno-exceptions"
export CARGO_TARGET_WASM32_WASIP1_LINKER="${WASI_SDK_PATH}/bin/clang"

# ---------------------------------------------------------------------------
# Tectonic dependency discovery (manual backend)
# ---------------------------------------------------------------------------
export TECTONIC_DEP_BACKEND=manual

# FreeType2
export FREETYPE2_INCLUDE_PATH="${SYSROOT}/include/freetype2;${SYSROOT}/include"
export FREETYPE2_LIB_DIR="${SYSROOT}/lib"

# Graphite2
export GRAPHITE2_INCLUDE_PATH="${SYSROOT}/include"
export GRAPHITE2_LIB_DIR="${SYSROOT}/lib"

# ICU
export ICUUC_INCLUDE_PATH="${SYSROOT}/include"
export ICUUC_LIB_DIR="${SYSROOT}/lib"

# libpng
export PNG_INCLUDE_PATH="${SYSROOT}/include"
export PNG_LIB_DIR="${SYSROOT}/lib"

# zlib (needed transitively)
export ZLIB_INCLUDE_PATH="${SYSROOT}/include"
export ZLIB_LIB_DIR="${SYSROOT}/lib"

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
echo "==> Building tectonic_wasi for wasm32-wasip1 ..."
echo "    WASI SDK    : ${WASI_SDK_PATH}"
echo "    Sysroot     : ${SYSROOT}"
echo ""

cargo build \
    --target wasm32-wasip1 \
    -p tectonic_wasi \
    --release

WASM_FILE="${SCRIPT_DIR}/target/wasm32-wasip1/release/tectonic_wasi.wasm"

if [ -f "${WASM_FILE}" ]; then
    echo ""
    echo "==> Build successful!"
    echo "    Output: ${WASM_FILE}"
    echo "    Size:   $(du -h "${WASM_FILE}" | cut -f1)"
else
    echo ""
    echo "ERROR: expected output not found: ${WASM_FILE}" >&2
    exit 1
fi
