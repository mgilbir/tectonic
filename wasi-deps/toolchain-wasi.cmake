# CMake toolchain file for cross-compiling to wasm32-wasip1 using WASI SDK.
#
# Usage:
#   cmake -DCMAKE_TOOLCHAIN_FILE=<path>/toolchain-wasi.cmake ..
#
# Expects WASI SDK installed at /opt/wasi-sdk (override with WASI_SDK_PATH env var).

# ---------------------------------------------------------------------------
# System identification
# ---------------------------------------------------------------------------
set(CMAKE_SYSTEM_NAME WASI)
set(CMAKE_SYSTEM_PROCESSOR wasm32)

# ---------------------------------------------------------------------------
# WASI SDK paths
# ---------------------------------------------------------------------------
if(DEFINED ENV{WASI_SDK_PATH})
    set(WASI_SDK_PATH "$ENV{WASI_SDK_PATH}")
else()
    set(WASI_SDK_PATH "/opt/wasi-sdk")
endif()

set(CMAKE_SYSROOT "${WASI_SDK_PATH}/share/wasi-sysroot")

# ---------------------------------------------------------------------------
# Compilers and tools
# ---------------------------------------------------------------------------
set(CMAKE_C_COMPILER   "${WASI_SDK_PATH}/bin/clang")
set(CMAKE_CXX_COMPILER "${WASI_SDK_PATH}/bin/clang++")
set(CMAKE_AR           "${WASI_SDK_PATH}/bin/llvm-ar" CACHE FILEPATH "Archiver")
set(CMAKE_RANLIB       "${WASI_SDK_PATH}/bin/llvm-ranlib" CACHE FILEPATH "Ranlib")
set(CMAKE_C_COMPILER_TARGET   "wasm32-wasi")
set(CMAKE_CXX_COMPILER_TARGET "wasm32-wasi")

# ---------------------------------------------------------------------------
# Search-path behaviour: only look inside the sysroot for libraries / headers
# but use host tools (programs) such as generators, pkg-config, etc.
# ---------------------------------------------------------------------------
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# ---------------------------------------------------------------------------
# Disable pthreads -- WASI does not support them
# ---------------------------------------------------------------------------
set(THREADS_PREFER_PTHREAD_FLAG OFF)
set(CMAKE_THREAD_LIBS_INIT "")
set(CMAKE_HAVE_THREADS_LIBRARY 0)
set(CMAKE_USE_WIN32_THREADS_INIT OFF)
set(CMAKE_USE_PTHREADS_INIT OFF)

# ---------------------------------------------------------------------------
# Misc flags helpful for most wasm builds
# ---------------------------------------------------------------------------
set(CMAKE_C_FLAGS_INIT   "-D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL -mllvm -wasm-enable-sjlj")
set(CMAKE_CXX_FLAGS_INIT "-D_WASI_EMULATED_MMAN -D_WASI_EMULATED_SIGNAL -mllvm -wasm-enable-sjlj -fno-exceptions")

# Prevent CMake from testing the compiler with a full link (it will fail
# because we do not have a full libc startup for executables by default).
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
