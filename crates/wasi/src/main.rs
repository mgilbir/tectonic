// Copyright 2024 the Tectonic Project
// Licensed under the MIT License.

//! Tectonic compiled as a WASI reactor library for embedding via wazero.
//!
//! The host mounts directories via WASI filesystem:
//! - `/input/` — TeX source files
//! - `/output/` — PDF output
//! - `/bundle/` — TeX Live support files (DirBundle)
//! - `/fonts/` — font files (.ttf/.otf)
//! - `/cache/` — format file cache

mod driver;

fn main() {
    // WASI command entry point — not used when calling exports directly.
    // The host (wazero) calls tectonic_compile() or tectonic_compile_defaults() instead.
}

/// Version of the host-facing ABI this module implements.
///
/// The contract this number covers: the exported function names and signatures
/// (tectonic_compile, tectonic_compile_defaults, tectonic_generate_format), the
/// reserved proc_exit status for a controlled engine abort (see the sjlj stub),
/// the recognized environment variables (TECTONIC_MAX_PASSES, TECTONIC_CACHE_DIR,
/// TECTONIC_FONT_DIR), and the guest mount layout.
///
/// The embedding host reads this once at startup and refuses a module whose ABI
/// it was not built for, so a rebuild from an incompatible source fails loudly
/// instead of drifting into subtle misbehavior. Returning a constant means it is
/// callable without running _initialize.
///
/// Bump this whenever any part of that contract changes.
#[no_mangle]
pub extern "C" fn tectonic_abi_version() -> i32 {
    1
}

/// Compile a TeX document to PDF.
///
/// # Arguments
/// * `input_path_ptr`/`input_path_len` — path to the main .tex file (relative to /input/)
/// * `output_dir_ptr`/`output_dir_len` — path to the output directory
/// * `bundle_dir_ptr`/`bundle_dir_len` — path to the TeX bundle directory
///
/// # Returns
/// 0 on success, non-zero on error.
///
/// # Safety
/// The pointer arguments must point to valid UTF-8 strings of the specified lengths.
#[no_mangle]
pub unsafe extern "C" fn tectonic_compile(
    input_path_ptr: *const u8,
    input_path_len: u32,
    output_dir_ptr: *const u8,
    output_dir_len: u32,
    bundle_dir_ptr: *const u8,
    bundle_dir_len: u32,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let input_path = std::str::from_utf8(std::slice::from_raw_parts(
            input_path_ptr,
            input_path_len as usize,
        ))
        .map_err(|e| anyhow::anyhow!("invalid UTF-8 in input_path: {e}"))?;

        let output_dir = std::str::from_utf8(std::slice::from_raw_parts(
            output_dir_ptr,
            output_dir_len as usize,
        ))
        .map_err(|e| anyhow::anyhow!("invalid UTF-8 in output_dir: {e}"))?;

        let bundle_dir = std::str::from_utf8(std::slice::from_raw_parts(
            bundle_dir_ptr,
            bundle_dir_len as usize,
        ))
        .map_err(|e| anyhow::anyhow!("invalid UTF-8 in bundle_dir: {e}"))?;

        driver::compile(input_path, output_dir, bundle_dir)
    });

    match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            eprintln!("tectonic error: {e}");
            1
        }
        Err(_) => {
            eprintln!("tectonic panic during compilation");
            2
        }
    }
}

/// Compile a TeX document using default paths.
///
/// Uses `/input/`, `/output/`, and `/bundle/` as the default directories.
/// The input file is the first .tex file found in /input/, or "input.tex".
///
/// # Returns
/// 0 on success, non-zero on error.
#[no_mangle]
pub extern "C" fn tectonic_compile_defaults() -> i32 {
    let result = std::panic::catch_unwind(|| {
        // Find the primary .tex file
        let input_path =
            find_primary_tex_file("/input/").unwrap_or_else(|| "input.tex".to_string());

        driver::compile(&input_path, "/output", "/bundle")
    });

    match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            eprintln!("tectonic error: {e}");
            1
        }
        Err(_) => {
            eprintln!("tectonic panic during compilation");
            2
        }
    }
}

/// Generate a TeX format file (latex.fmt) in initex mode.
///
/// Uses `/bundle/` as the TeX support files directory.
/// The generated format file is written to `/cache/latex.fmt`.
///
/// # Returns
/// 0 on success, non-zero on error.
#[no_mangle]
pub extern "C" fn tectonic_generate_format() -> i32 {
    let result = std::panic::catch_unwind(|| driver::generate_format("/bundle"));

    match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            eprintln!("tectonic error: {e}");
            1
        }
        Err(_) => {
            eprintln!("tectonic panic during format generation");
            2
        }
    }
}

/// Find the primary .tex file in the input directory.
fn find_primary_tex_file(dir: &str) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("tex") {
            return path.to_str().map(|s| s.to_string());
        }
    }
    None
}
