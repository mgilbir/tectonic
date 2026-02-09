#!/usr/bin/env bash
# setup-toolchain.sh
# Phase 0: Install the toolchain components needed to build Tectonic as a WASI library.
#   1. wasi-sdk 25       (C/C++ cross-compiler targeting wasm32-wasi)
#   2. Rust wasm32-wasip1 target
#   3. wazero CLI         (pure-Go WebAssembly runtime, used for testing)

set -euo pipefail

WASI_SDK_VERSION=25
WASI_SDK_DIR="/opt/wasi-sdk"
WAZERO_VERSION="1.8.2"

# ── helpers ──────────────────────────────────────────────────────────────────

info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[OK]\033[0m    %s\n' "$*"; }
err()   { printf '\033[1;31m[ERR]\033[0m   %s\n' "$*" >&2; }

need_cmd() {
    if ! command -v "$1" &>/dev/null; then
        err "Required command '$1' not found. Please install it first."
        exit 1
    fi
}

# ── 1. wasi-sdk ──────────────────────────────────────────────────────────────

install_wasi_sdk() {
    if [ -x "${WASI_SDK_DIR}/bin/clang" ]; then
        local installed_version
        installed_version=$("${WASI_SDK_DIR}/bin/clang" --version 2>/dev/null | head -1 || true)
        ok "wasi-sdk already installed at ${WASI_SDK_DIR}"
        info "  ${installed_version}"
        return 0
    fi

    need_cmd curl
    need_cmd tar

    info "Installing wasi-sdk ${WASI_SDK_VERSION} to ${WASI_SDK_DIR} ..."

    local arch
    arch="$(uname -m)"
    local os
    os="$(uname -s | tr '[:upper:]' '[:lower:]')"

    # Map architecture names to wasi-sdk naming convention
    case "${arch}" in
        x86_64)  arch="x86_64" ;;
        aarch64|arm64) arch="arm64" ;;
        *)
            err "Unsupported architecture: ${arch}"
            exit 1
            ;;
    esac

    case "${os}" in
        linux)  os="linux" ;;
        darwin) os="macos" ;;
        *)
            err "Unsupported OS: ${os}"
            exit 1
            ;;
    esac

    local tarball="wasi-sdk-${WASI_SDK_VERSION}.0-${arch}-${os}.tar.gz"
    local url="https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_SDK_VERSION}/${tarball}"

    info "Downloading ${url} ..."
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "${tmpdir}"' EXIT

    curl -fSL --retry 3 -o "${tmpdir}/${tarball}" "${url}"

    info "Extracting to ${WASI_SDK_DIR} ..."
    sudo mkdir -p "${WASI_SDK_DIR}"
    sudo tar xzf "${tmpdir}/${tarball}" -C "${WASI_SDK_DIR}" --strip-components=1

    if [ ! -x "${WASI_SDK_DIR}/bin/clang" ]; then
        err "Installation failed: ${WASI_SDK_DIR}/bin/clang not found after extraction."
        exit 1
    fi

    ok "wasi-sdk ${WASI_SDK_VERSION} installed successfully."
}

# ── 2. Rust wasm32-wasip1 target ─────────────────────────────────────────────

install_rust_target() {
    need_cmd rustup

    if rustup target list --installed | grep -q 'wasm32-wasip1'; then
        ok "Rust target wasm32-wasip1 is already installed."
        return 0
    fi

    info "Adding Rust target wasm32-wasip1 ..."
    rustup target add wasm32-wasip1

    if ! rustup target list --installed | grep -q 'wasm32-wasip1'; then
        err "Failed to install Rust target wasm32-wasip1."
        exit 1
    fi

    ok "Rust target wasm32-wasip1 installed successfully."
}

# ── 3. wazero CLI ────────────────────────────────────────────────────────────

install_wazero() {
    if command -v wazero &>/dev/null; then
        local installed_version
        installed_version="$(wazero version 2>/dev/null || echo unknown)"
        ok "wazero already installed (version: ${installed_version})."
        return 0
    fi

    need_cmd go

    info "Installing wazero CLI v${WAZERO_VERSION} via 'go install' ..."
    go install "github.com/tetratelabs/wazero/cmd/wazero@v${WAZERO_VERSION}"

    # Verify it landed on PATH (GOBIN or GOPATH/bin)
    if ! command -v wazero &>/dev/null; then
        local gobin
        gobin="$(go env GOBIN)"
        [ -z "${gobin}" ] && gobin="$(go env GOPATH)/bin"
        if [ -x "${gobin}/wazero" ]; then
            info "wazero installed to ${gobin}/wazero but it is not on your PATH."
            info "Add the following to your shell profile:"
            info "  export PATH=\"${gobin}:\${PATH}\""
        else
            err "Failed to install wazero. Ensure Go is correctly configured."
            exit 1
        fi
    fi

    ok "wazero CLI installed successfully."
}

# ── main ─────────────────────────────────────────────────────────────────────

main() {
    info "=== Phase 0: Tectonic WASI toolchain setup ==="
    echo

    install_wasi_sdk
    echo

    install_rust_target
    echo

    install_wazero
    echo

    info "=== Setup complete ==="
    info "wasi-sdk : ${WASI_SDK_DIR}"
    info "Rust     : $(rustc --version) + wasm32-wasip1"
    if command -v wazero &>/dev/null; then
        info "wazero   : $(wazero version 2>/dev/null || echo 'installed')"
    fi
}

main "$@"
