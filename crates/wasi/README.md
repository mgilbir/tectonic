# Tectonic WASI

Tectonic compiled as a WASI reactor library for embedding in other programs via
[wazero](https://wazero.io/) or any other WebAssembly runtime.

The module exports C-callable functions that compile LaTeX documents to PDF.
The host mounts directories via WASI filesystem preopens — no network access or
system font discovery is needed.

## Quick start

```bash
# 1. Install wasi-sdk and Rust target
wasi-deps/setup-toolchain.sh

# 2. Build external C/C++ dependencies (zlib, libpng, FreeType2, Graphite2, ICU)
WASI_SDK_PATH=~/wasi-sdk wasi-deps/build-wasi-deps.sh

# 3. Build the WASM module
WASI_SDK_PATH=~/wasi-sdk build-wasi.sh
# → target/wasm32-wasip1/release/tectonic_wasi.wasm (≈5 MB)
```

## Exported API

### `tectonic_compile_defaults() -> i32`

Compile the first `.tex` file found in `/input/` using default directory paths.
Returns 0 on success, 1 on TeX error, 2 on panic.

### `tectonic_compile(input_path, input_len, output_path, output_len, bundle_path, bundle_len) -> i32`

Compile with explicit paths. Arguments are pointer/length pairs to UTF-8
strings in WASM linear memory.

## Filesystem layout

The host mounts these directories before calling the exports:

| Mount point | Contents |
|-------------|----------|
| `/input/`   | TeX source files (`.tex`, `.bib`, etc.) |
| `/output/`  | PDF output is written here |
| `/bundle/`  | TeX Live support files (flat directory of `.tfm`, `.sty`, `.cls`, etc.) |
| `/fonts/`   | Font files (`.ttf`, `.otf`, `.ttc`) |
| `/cache/`   | Format file cache (`latex.fmt`) |

The bundle directory must contain a pre-compiled format file named `latex.fmt`.
Generate it once with native Tectonic, then reuse it for all WASI compilations:

```bash
tectonic --only-cached -p '\documentclass{article}\begin{document}x\end{document}'
cp ~/.cache/Tectonic/formats/*-latex-*.fmt /path/to/bundle/latex.fmt
```

## Example: wazero (Go)

```go
ctx := context.Background()
rt := wazero.NewRuntime(ctx)
defer rt.Close(ctx)

wasi_snapshot_preview1.MustInstantiate(ctx, rt)

fsConfig := wazero.NewFSConfig().
    WithDirMount("./input", "/input").
    WithDirMount("./output", "/output").
    WithDirMount("./bundle", "/bundle").
    WithDirMount("./fonts", "/fonts").
    WithDirMount("./cache", "/cache")

config := wazero.NewModuleConfig().
    WithStderr(os.Stderr).
    WithFSConfig(fsConfig)

wasmBytes, _ := os.ReadFile("tectonic_wasi.wasm")
compiled, _ := rt.CompileModule(ctx, wasmBytes)
mod, _ := rt.InstantiateModule(ctx, compiled, config)

results, err := mod.ExportedFunction("tectonic_compile_defaults").Call(ctx)
if err != nil || results[0] != 0 {
    log.Fatal("compilation failed")
}
// PDF is now at ./output/input.pdf
```

A complete integration test is at `tests/wazero/main_test.go`:

```bash
TECTONIC_BUNDLE_DIR=/path/to/bundle TECTONIC_FONT_DIR=/path/to/fonts go test -v ./tests/wazero/
```

## Architecture decisions

### Why a `bin` crate instead of `cdylib`

For `wasm32-wasip1`, a `cdylib` crate adds the `-shared` linker flag which
requires all object code (including pre-built C/C++ static libraries) to be
compiled with `-fPIC`. Recompiling every dependency with PIC is impractical.

A `bin` crate produces a normal WASI command module. We export specific
functions via `-Wl,--export=` linker flags in `.cargo/config.toml`. The empty
`fn main()` serves as the WASI `_start` entry point (unused when calling
exports directly).

### Why we stub setjmp/longjmp instead of using WASM exception handling

Tectonic's C engine code uses `setjmp`/`longjmp` for fatal error recovery.
In the TeX engine entry points, `setjmp` sets up a catch point, and
`_tt_abort()` calls `longjmp` to bail out on errors.

wasi-sdk implements `setjmp`/`longjmp` via the WebAssembly exception handling
proposal, enabled with `-mllvm -wasm-enable-sjlj`. This generates **legacy**
exception handling instructions (`try`/`catch`/`delegate`). The problem:

- **wazero** does not support any form of WASM exception handling, and has no
  plans to add it. It targets Core WebAssembly 2.0 only.
- **wasmtime** supports the **new** exception handling proposal (`try_table`),
  but not the legacy format that wasi-sdk 25 emits.
- Upgrading to wasi-sdk 26 would allow emitting new-style exceptions via
  `-mllvm -wasm-use-legacy-eh=false`, but wazero would still reject them.

Our solution: provide stub implementations in `wasi-deps/sjlj-stub/`:

- `setjmp()` always returns 0 (the "normal" code path)
- `longjmp()` prints an error message and calls `abort()`

The header is injected via `-isystem wasi-deps/sjlj-stub` so it overrides the
system `setjmp.h` (which has `#error` without the exception handling flag).

**Tradeoff:** TeX errors that would normally be caught by `longjmp` instead
cause a WASM trap. The host runtime catches the trap and knows compilation
failed. For documents that compile successfully, `longjmp` is never called and
the stub has zero overhead. This is an acceptable tradeoff because:

1. Most TeX documents either compile or have clear errors visible in stderr
2. The host can create a fresh WASM instance for each compilation
3. The module works on every WASM runtime without feature negotiations

### Font discovery without fontconfig

Native Tectonic uses fontconfig (Linux) or CoreText (macOS) for system font
discovery. Neither exists in WASI.

The `WasiFsBackend` in `crates/xetex_layout/src/manager/wasi_fs.rs` scans a
mounted `/fonts/` directory for `.ttf`/`.otf`/`.ttc` files and extracts font
metadata using FreeType. The `TECTONIC_FONT_DIR` environment variable overrides
the default path.

### Network-free bundle

The WASI crate depends on `tectonic_bundles` with `default-features = false`,
which disables all network bundle types. Only `DirBundle` (flat filesystem
directory) is available. The host must provide a complete bundle directory
containing all the TeX Live support files the document needs.

### Simplified driver

The WASI driver (`src/driver.rs`, ~370 lines) is a stripped-down version of the
main Tectonic driver (~2300 lines). It omits: network bundles, shell escape,
file watching, async/tokio, process spawning, and HTML output. It keeps:
multi-pass TeX with convergence detection (SHA-256 of `.aux` files), BibTeX
integration, and XDV-to-PDF conversion via xdvipdfmx.

## External C/C++ dependencies

Six libraries are cross-compiled to `wasm32-wasip1` by `wasi-deps/build-wasi-deps.sh`:

| Library | Version | Purpose |
|---------|---------|---------|
| zlib | 1.3.1 | Compression (used by PDF I/O code) |
| libpng | 1.6.43 | PNG image support |
| FreeType2 | 2.13.3 | Font rasterization |
| Graphite2 | 1.3.14 | Smart font rendering |
| ICU | 74.2 | Unicode (stubdata + icuuc only) |

HarfBuzz is vendored and compiled via the `cc` crate in `bridge_harfbuzz` (not
built as an external dependency). It is compiled with `HB_NO_MT=1` since WASI
has no threading.

## Linker configuration

The `.cargo/config.toml` sets these flags for `wasm32-wasip1`:

- `-Wl,--export=tectonic_compile` / `--export=tectonic_compile_defaults` — reactor exports
- `-lc++ -lc++abi` — C++ runtime for HarfBuzz
- `-lwasi-emulated-mman` — `mmap`/`munmap` emulation
- `-lwasi-emulated-signal` — signal emulation

The `pdf_io` crate's build script additionally links `-lz` on wasm32 because
its C code calls zlib functions directly (the Rust side uses `flate2` with
`rust_backend` which is pure Rust and doesn't provide C symbols).
