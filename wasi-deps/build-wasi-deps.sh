#!/usr/bin/env bash
# =============================================================================
# build-wasi-deps.sh
#
# Cross-compile the external C/C++ libraries required by Tectonic to
# wasm32-wasip1 using the WASI SDK.
#
# Libraries built (in order):
#   1. zlib        1.3.1
#   2. libpng      1.6.43
#   3. FreeType2   2.13.3
#   4. Graphite2   1.3.14
#   5. ICU         74.2  (stubdata + common/icuuc only)
#
# All artefacts are installed into wasi-deps/sysroot/.
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TECTONIC_ROOT="${TECTONIC_ROOT:-$(dirname "$SCRIPT_DIR")}"

WASI_SDK_PATH="${WASI_SDK_PATH:-/opt/wasi-sdk}"
TOOLCHAIN_FILE="${SCRIPT_DIR}/toolchain-wasi.cmake"

SYSROOT="${TECTONIC_ROOT}/wasi-deps/sysroot"
SRC_DIR="${TECTONIC_ROOT}/wasi-deps/src"
BUILD_DIR="${TECTONIC_ROOT}/wasi-deps/build"

NPROC="$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)"

# ---------------------------------------------------------------------------
# Toolchain variables (used for autotools-based builds like ICU)
# ---------------------------------------------------------------------------
export CC="${WASI_SDK_PATH}/bin/clang"
export CXX="${WASI_SDK_PATH}/bin/clang++"
export AR="${WASI_SDK_PATH}/bin/llvm-ar"
export RANLIB="${WASI_SDK_PATH}/bin/llvm-ranlib"
export NM="${WASI_SDK_PATH}/bin/llvm-nm"
export STRIP="${WASI_SDK_PATH}/bin/llvm-strip"

SJLJ_STUB="${SCRIPT_DIR}/sjlj-stub"
COMMON_CFLAGS="--target=wasm32-wasi --sysroot=${WASI_SDK_PATH}/share/wasi-sysroot -O2 -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL -isystem ${SJLJ_STUB}"
COMMON_CXXFLAGS="${COMMON_CFLAGS} -fno-exceptions"

export CFLAGS="${COMMON_CFLAGS}"
export CXXFLAGS="${COMMON_CXXFLAGS}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
msg() {
    echo ""
    echo "========================================"
    echo "  $*"
    echo "========================================"
    echo ""
}

download() {
    local url="$1"
    local dest="$2"
    if [ -f "$dest" ]; then
        echo "  [skip] $(basename "$dest") already downloaded."
        return 0
    fi
    echo "  Downloading $(basename "$dest") ..."
    curl -fSL --retry 3 -o "$dest" "$url"
}

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------
if [ ! -x "${CC}" ]; then
    echo "ERROR: WASI SDK not found at ${WASI_SDK_PATH}"
    echo "       Please install it or set WASI_SDK_PATH."
    exit 1
fi

mkdir -p "${SRC_DIR}" "${BUILD_DIR}" "${SYSROOT}"

# ============================= 1. zlib =====================================
ZLIB_VER="1.3.1"
ZLIB_TARBALL="${SRC_DIR}/zlib-${ZLIB_VER}.tar.gz"
ZLIB_URL="https://github.com/madler/zlib/releases/download/v${ZLIB_VER}/zlib-${ZLIB_VER}.tar.gz"
ZLIB_SRC="${SRC_DIR}/zlib-${ZLIB_VER}"
ZLIB_BUILD="${BUILD_DIR}/zlib"

build_zlib() {
    msg "Building zlib ${ZLIB_VER}"

    download "${ZLIB_URL}" "${ZLIB_TARBALL}"
    [ -d "${ZLIB_SRC}" ] || tar xf "${ZLIB_TARBALL}" -C "${SRC_DIR}"

    rm -rf "${ZLIB_BUILD}"
    mkdir -p "${ZLIB_BUILD}"

    cmake -S "${ZLIB_SRC}" -B "${ZLIB_BUILD}" \
        -DCMAKE_TOOLCHAIN_FILE="${TOOLCHAIN_FILE}" \
        -DCMAKE_INSTALL_PREFIX="${SYSROOT}" \
        -DCMAKE_PREFIX_PATH="${SYSROOT}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DZLIB_BUILD_EXAMPLES=OFF

    cmake --build "${ZLIB_BUILD}" -j "${NPROC}"
    cmake --install "${ZLIB_BUILD}"

    # zlib's CMake installs as libzlibstatic.a; create libz.a symlink for consumers
    if [ -f "${SYSROOT}/lib/libzlibstatic.a" ] && [ ! -f "${SYSROOT}/lib/libz.a" ]; then
        ln -s libzlibstatic.a "${SYSROOT}/lib/libz.a"
    fi

    echo "  zlib installed to ${SYSROOT}"
}

# ============================= 2. libpng ===================================
LIBPNG_VER="1.6.43"
LIBPNG_TARBALL="${SRC_DIR}/libpng-${LIBPNG_VER}.tar.gz"
LIBPNG_URL="https://download.sourceforge.net/libpng/libpng-${LIBPNG_VER}.tar.gz"
LIBPNG_SRC="${SRC_DIR}/libpng-${LIBPNG_VER}"
LIBPNG_BUILD="${BUILD_DIR}/libpng"

build_libpng() {
    msg "Building libpng ${LIBPNG_VER}"

    download "${LIBPNG_URL}" "${LIBPNG_TARBALL}"
    [ -d "${LIBPNG_SRC}" ] || tar xf "${LIBPNG_TARBALL}" -C "${SRC_DIR}"

    rm -rf "${LIBPNG_BUILD}"
    mkdir -p "${LIBPNG_BUILD}"

    cmake -S "${LIBPNG_SRC}" -B "${LIBPNG_BUILD}" \
        -DCMAKE_TOOLCHAIN_FILE="${TOOLCHAIN_FILE}" \
        -DCMAKE_INSTALL_PREFIX="${SYSROOT}" \
        -DCMAKE_PREFIX_PATH="${SYSROOT}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DPNG_SHARED=OFF \
        -DPNG_STATIC=ON \
        -DPNG_TESTS=OFF \
        -DPNG_EXECUTABLES=OFF \
        -DCMAKE_C_FLAGS="${COMMON_CFLAGS}" \
        -DZLIB_ROOT="${SYSROOT}" \
        -DZLIB_INCLUDE_DIR="${SYSROOT}/include" \
        -DZLIB_LIBRARY="${SYSROOT}/lib/libz.a"

    cmake --build "${LIBPNG_BUILD}" -j "${NPROC}"
    cmake --install "${LIBPNG_BUILD}"

    # Create libpng16.a symlink — CMake installs as liblibpng16_static.a
    if [ -f "${SYSROOT}/lib/liblibpng16_static.a" ] && [ ! -f "${SYSROOT}/lib/libpng16.a" ]; then
        ln -s liblibpng16_static.a "${SYSROOT}/lib/libpng16.a"
    fi

    echo "  libpng installed to ${SYSROOT}"
}

# ============================= 3. FreeType2 ================================
FREETYPE_VER="2.13.3"
FREETYPE_TARBALL="${SRC_DIR}/freetype-${FREETYPE_VER}.tar.gz"
FREETYPE_URL="https://download.savannah.gnu.org/releases/freetype/freetype-${FREETYPE_VER}.tar.gz"
FREETYPE_SRC="${SRC_DIR}/freetype-${FREETYPE_VER}"
FREETYPE_BUILD="${BUILD_DIR}/freetype"

build_freetype() {
    msg "Building FreeType2 ${FREETYPE_VER}"

    download "${FREETYPE_URL}" "${FREETYPE_TARBALL}"
    [ -d "${FREETYPE_SRC}" ] || tar xf "${FREETYPE_TARBALL}" -C "${SRC_DIR}"

    rm -rf "${FREETYPE_BUILD}"
    mkdir -p "${FREETYPE_BUILD}"

    cmake -S "${FREETYPE_SRC}" -B "${FREETYPE_BUILD}" \
        -DCMAKE_TOOLCHAIN_FILE="${TOOLCHAIN_FILE}" \
        -DCMAKE_INSTALL_PREFIX="${SYSROOT}" \
        -DCMAKE_PREFIX_PATH="${SYSROOT}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DFT_DISABLE_ZLIB=OFF \
        -DFT_DISABLE_PNG=OFF \
        -DFT_DISABLE_BZIP2=ON \
        -DFT_DISABLE_BROTLI=ON \
        -DFT_DISABLE_HARFBUZZ=ON \
        -DCMAKE_C_FLAGS="${COMMON_CFLAGS} -DFT_CONFIG_OPTION_DISABLE_MMAP" \
        -DZLIB_ROOT="${SYSROOT}" \
        -DZLIB_INCLUDE_DIR="${SYSROOT}/include" \
        -DZLIB_LIBRARY="${SYSROOT}/lib/libz.a" \
        -DPNG_PNG_INCLUDE_DIR="${SYSROOT}/include" \
        -DPNG_LIBRARY="${SYSROOT}/lib/libpng.a"

    cmake --build "${FREETYPE_BUILD}" -j "${NPROC}"
    cmake --install "${FREETYPE_BUILD}"

    echo "  FreeType2 installed to ${SYSROOT}"
}

# ============================= 4. Graphite2 ================================
GRAPHITE2_VER="1.3.14"
GRAPHITE2_TARBALL="${SRC_DIR}/graphite2-${GRAPHITE2_VER}.tar.gz"
GRAPHITE2_URL="https://github.com/silnrsi/graphite/releases/download/${GRAPHITE2_VER}/graphite2-${GRAPHITE2_VER}.tgz"
GRAPHITE2_SRC="${SRC_DIR}/graphite2-${GRAPHITE2_VER}"
GRAPHITE2_BUILD="${BUILD_DIR}/graphite2"

build_graphite2() {
    msg "Building Graphite2 ${GRAPHITE2_VER}"

    download "${GRAPHITE2_URL}" "${GRAPHITE2_TARBALL}"
    [ -d "${GRAPHITE2_SRC}" ] || tar xf "${GRAPHITE2_TARBALL}" -C "${SRC_DIR}"

    rm -rf "${GRAPHITE2_BUILD}"
    mkdir -p "${GRAPHITE2_BUILD}"

    cmake -S "${GRAPHITE2_SRC}" -B "${GRAPHITE2_BUILD}" \
        -DCMAKE_TOOLCHAIN_FILE="${TOOLCHAIN_FILE}" \
        -DCMAKE_INSTALL_PREFIX="${SYSROOT}" \
        -DCMAKE_PREFIX_PATH="${SYSROOT}" \
        -DBUILD_SHARED_LIBS=OFF \
        -DGRAPHITE2_NTRACING=ON \
        -DGRAPHITE2_TESTS=OFF \
        -DGRAPHITE2_COMPARE_RENDERER=OFF

    cmake --build "${GRAPHITE2_BUILD}" --target graphite2 -j "${NPROC}"

    # Manual install — cmake --install tries to install test targets that weren't built
    mkdir -p "${SYSROOT}/lib" "${SYSROOT}/include/graphite2"
    cp "${GRAPHITE2_BUILD}/src/libgraphite2.a" "${SYSROOT}/lib/"
    cp "${GRAPHITE2_SRC}/include/graphite2/"*.h "${SYSROOT}/include/graphite2/"

    echo "  Graphite2 installed to ${SYSROOT}"
}

# ============================= 5. ICU ======================================
ICU_VER="74.2"
ICU_VER_UNDERSCORE="74_2"
ICU_TARBALL="${SRC_DIR}/icu4c-${ICU_VER_UNDERSCORE}-src.tgz"
ICU_URL="https://github.com/unicode-org/icu/releases/download/release-${ICU_VER//./-}/icu4c-${ICU_VER_UNDERSCORE}-src.tgz"
ICU_SRC="${SRC_DIR}/icu/source"
ICU_HOST_BUILD="${BUILD_DIR}/icu-host"
ICU_WASM_BUILD="${BUILD_DIR}/icu-wasm"

build_icu() {
    msg "Building ICU ${ICU_VER} (host tools + wasm32 cross-compile)"

    download "${ICU_URL}" "${ICU_TARBALL}"
    [ -d "${SRC_DIR}/icu" ] || tar xf "${ICU_TARBALL}" -C "${SRC_DIR}"

    # Replace ICU's old config.sub/config.guess with system versions
    # that know about wasm32-wasi
    cp /usr/share/misc/config.sub "${ICU_SRC}/config.sub"
    cp /usr/share/misc/config.guess "${ICU_SRC}/config.guess" 2>/dev/null || true

    # ICU's mh-unknown stub errors out; always overwrite with mh-linux
    cp "${ICU_SRC}/config/mh-linux" "${ICU_SRC}/config/mh-unknown"

    # -----------------------------------------------------------------
    # Step 1: Build a host (native) ICU -- needed for cross-compile
    #         tooling (icupkg, pkgdata, genccode, etc.)
    # -----------------------------------------------------------------
    msg "  ICU: building host (native) tools"

    rm -rf "${ICU_HOST_BUILD}"
    mkdir -p "${ICU_HOST_BUILD}"

    (
        # Unset cross-compile environment for the host build
        unset CC CXX AR RANLIB NM STRIP CFLAGS CXXFLAGS

        cd "${ICU_HOST_BUILD}"
        "${ICU_SRC}/configure" \
            --disable-shared \
            --enable-static \
            --disable-tests \
            --disable-samples \
            --disable-extras \
            --disable-icuio \
            --disable-layoutex \
            --prefix="${ICU_HOST_BUILD}/install"

        make -j "${NPROC}"
        make install
    )

    # -----------------------------------------------------------------
    # Step 2: Cross-compile ICU for wasm32-wasi
    # -----------------------------------------------------------------
    msg "  ICU: cross-compiling for wasm32-wasi"

    rm -rf "${ICU_WASM_BUILD}"
    mkdir -p "${ICU_WASM_BUILD}"

    (
        cd "${ICU_WASM_BUILD}"

        # Re-export cross-compile env (subshell above cleared them)
        export CC="${WASI_SDK_PATH}/bin/clang"
        export CXX="${WASI_SDK_PATH}/bin/clang++"
        export AR="${WASI_SDK_PATH}/bin/llvm-ar"
        export RANLIB="${WASI_SDK_PATH}/bin/llvm-ranlib"
        export NM="${WASI_SDK_PATH}/bin/llvm-nm"
        export STRIP="${WASI_SDK_PATH}/bin/llvm-strip"
        ICU_STUBS="${SCRIPT_DIR}/icu-stubs"
        export CFLAGS="--target=wasm32-wasi --sysroot=${WASI_SDK_PATH}/share/wasi-sysroot -O2 -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL -isystem ${SJLJ_STUB} -fno-exceptions -DU_HAVE_TZNAME=0"
        export CXXFLAGS="${CFLAGS} -fno-exceptions -isystem ${ICU_STUBS}"
        export LDFLAGS="-lwasi-emulated-mman -lwasi-emulated-signal"

        # ICU's configure for cross-compilation
        "${ICU_SRC}/configure" \
            --host=wasm32-unknown-wasi \
            --with-cross-build="${ICU_HOST_BUILD}" \
            --prefix="${SYSROOT}" \
            --disable-shared \
            --enable-static \
            --disable-extras \
            --disable-icuio \
            --disable-layoutex \
            --disable-tests \
            --disable-samples \
            --with-data-packaging=static

        # Create the lib directory (ICU's Makefile expects ../lib/ to exist)
        mkdir -p "${ICU_WASM_BUILD}/lib"

        # Only build stubdata and common (icuuc).
        # Build stubdata first (common depends on it), then common.
        make -C stubdata -j "${NPROC}"
        make -C common -j "${NPROC}"

        # Install only the pieces we built
        make -C stubdata install
        make -C common install
    )

    echo "  ICU installed to ${SYSROOT}"
}

# ===========================================================================
# Main
# ===========================================================================
msg "Tectonic WASI dependency builder"
echo "  TECTONIC_ROOT : ${TECTONIC_ROOT}"
echo "  WASI_SDK_PATH : ${WASI_SDK_PATH}"
echo "  SYSROOT       : ${SYSROOT}"
echo "  TOOLCHAIN     : ${TOOLCHAIN_FILE}"
echo "  PARALLELISM   : ${NPROC}"

build_sjlj_stub() {
    msg "Building setjmp/longjmp stub"

    mkdir -p "${SYSROOT}/lib"
    "${CC}" ${COMMON_CFLAGS} -D_GNU_SOURCE -c "${SJLJ_STUB}/setjmp.c" -o "${BUILD_DIR}/setjmp.o"
    "${AR}" rcs "${SYSROOT}/lib/libsetjmp_stub.a" "${BUILD_DIR}/setjmp.o"

    echo "  sjlj stub installed to ${SYSROOT}/lib/libsetjmp_stub.a"
}

build_sjlj_stub
build_zlib
build_libpng
build_freetype
build_graphite2
build_icu

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
msg "Build complete!"
echo "Static libraries in ${SYSROOT}/lib/:"
echo ""
find "${SYSROOT}/lib" -name '*.a' | sort
echo ""
echo "Done."
