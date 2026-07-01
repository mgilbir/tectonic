// Copyright 2024 the Tectonic Project
// Licensed under the MIT License.

//! Simplified processing driver for the WASI environment.
//!
//! This is a stripped-down version of the main Tectonic driver that works
//! within the WASI sandbox. It omits: network bundles, shell escape, file
//! watching, async/tokio, process spawning, and HTML output.

use sha2::Digest;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tectonic_bridge_core::{CoreBridgeLauncher, DriverHooks};
use tectonic_bundles::dir::DirBundle;
use tectonic_engine_bibtex::BibtexEngine;
use tectonic_engine_xdvipdfmx::XdvipdfmxEngine;
use tectonic_engine_xetex::{TexEngine, TexOutcome};
use tectonic_errors::{anyhow, Error, Result};
use tectonic_io_base::digest::DigestData;
use tectonic_io_base::filesystem::{FilesystemIo, FilesystemPrimaryInputIo};
use tectonic_io_base::stdstreams::BufferedPrimaryIo;
use tectonic_io_base::{
    normalize_tex_path, InputFeatures, InputHandle, InputOrigin, IoProvider, OpenResult,
    OutputHandle,
};
use tectonic_status_base::{MessageKind, StatusBackend};

const MAX_TEX_PASSES: usize = 6;

/// Compile a TeX document.
pub fn compile(input_path: &str, output_dir: &str, bundle_dir: &str) -> Result<()> {
    let mut status = WasiStatusBackend;

    // Determine the input directory and file name
    let input_full = Path::new(input_path);
    let (input_dir, tex_name) = if input_full.is_absolute() {
        let parent = input_full
            .parent()
            .unwrap_or_else(|| Path::new("/input"));
        let name = input_full
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("input.tex");
        (parent.to_path_buf(), name.to_string())
    } else {
        (PathBuf::from("/input"), input_path.to_string())
    };

    // Strip .tex extension for output base name
    let base_name = tex_name.strip_suffix(".tex").unwrap_or(&tex_name);
    let xdv_name = format!("{base_name}.xdv");
    let pdf_name = format!("{base_name}.pdf");

    // Set up the format name
    let format_name = "latex".to_string();

    // Set up the I/O stack
    let bundle = DirBundle::new(PathBuf::from(bundle_dir));
    let filesystem = FilesystemIo::new(&input_dir, false, true, HashSet::new());
    let primary_input = FilesystemPrimaryInputIo::new(input_dir.join(&tex_name));
    let mem = MemoryIo::new(true);

    // Set up format cache
    let cache_dir = std::env::var("TECTONIC_CACHE_DIR").unwrap_or_else(|_| "/cache".to_string());
    let _ = std::fs::create_dir_all(&cache_dir);

    let mut bridge_state = WasiBridgeState {
        bundle,
        filesystem,
        primary_input,
        mem,
        events: HashMap::new(),
    };

    // Phase 1: Multi-pass TeX processing
    let mut prev_digest: Option<[u8; 32]> = None;

    for pass in 0..MAX_TEX_PASSES {
        eprintln!("tectonic: running TeX pass {}", pass + 1);

        let mut launcher = CoreBridgeLauncher::new(&mut bridge_state, &mut status);

        let result = TexEngine::default().process(&mut launcher, &format_name, &tex_name)?;

        match result {
            TexOutcome::Errors => {
                return Err(anyhow::anyhow!("TeX engine reported errors"));
            }
            _ => {}
        }

        // Check for BibTeX needs (look for \bibdata in .aux)
        if pass == 0 {
            let aux_name = format!("{base_name}.aux");
            if let Some(aux_data) = bridge_state.mem.get_file(&aux_name) {
                let aux_str = String::from_utf8_lossy(&aux_data);
                if aux_str.contains("\\bibdata") {
                    eprintln!("tectonic: running BibTeX");
                    let mut launcher =
                        CoreBridgeLauncher::new(&mut bridge_state, &mut status);
                    let _ = BibtexEngine::default().process(&mut launcher, &aux_name);
                }
            }
        }

        // Check if we need to rerun - compare aux file digest
        let aux_name = format!("{base_name}.aux");
        let current_digest = bridge_state.mem.get_file(&aux_name).map(|data| {
            let mut hasher = sha2::Sha256::new();
            hasher.update(&data);
            let hash: [u8; 32] = hasher.finalize().into();
            hash
        });

        if pass > 0 && prev_digest == current_digest {
            eprintln!("tectonic: output converged after {} passes", pass + 1);
            break;
        }

        prev_digest = current_digest;

        if pass == MAX_TEX_PASSES - 1 {
            eprintln!(
                "tectonic: warning: reached max {} TeX passes",
                MAX_TEX_PASSES
            );
        }
    }

    // Phase 2: XDV-to-PDF conversion
    if bridge_state.mem.has_file(&xdv_name) {
        eprintln!("tectonic: running xdvipdfmx");
        let mut launcher = CoreBridgeLauncher::new(&mut bridge_state, &mut status);

        XdvipdfmxEngine::default().process(&mut launcher, &xdv_name, &pdf_name)?;

        // Remove intermediate XDV file
        bridge_state.mem.remove_file(&xdv_name);
    }

    // Phase 3: Write output files
    let output_path = Path::new(output_dir);
    let _ = std::fs::create_dir_all(output_path);

    if let Some(pdf_data) = bridge_state.mem.get_file(&pdf_name) {
        let out_file = output_path.join(&pdf_name);
        std::fs::write(&out_file, &pdf_data)?;
        eprintln!("tectonic: wrote {}", out_file.display());
    } else {
        return Err(anyhow::anyhow!("no PDF output was generated"));
    }

    Ok(())
}

// ---- Memory I/O ----

/// Simple in-memory file collection for intermediates.
struct MemoryIo {
    files: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    stdout_allowed: bool,
}

impl MemoryIo {
    fn new(stdout_allowed: bool) -> Self {
        MemoryIo {
            files: Rc::new(RefCell::new(HashMap::new())),
            stdout_allowed,
        }
    }

    fn get_file(&self, name: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(name).cloned()
    }

    fn has_file(&self, name: &str) -> bool {
        self.files.borrow().contains_key(name)
    }

    fn remove_file(&mut self, name: &str) {
        self.files.borrow_mut().remove(name);
    }
}

struct MemoryItem {
    files: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    name: String,
    data: Cursor<Vec<u8>>,
}

impl Read for MemoryItem {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.data.read(buf)
    }
}

impl Write for MemoryItem {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.data.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.data.flush()
    }
}

impl Drop for MemoryItem {
    fn drop(&mut self) {
        let data = self.data.get_ref().clone();
        self.files.borrow_mut().insert(self.name.clone(), data);
    }
}

impl InputFeatures for MemoryItem {
    fn get_size(&mut self) -> Result<usize> {
        Ok(self.data.get_ref().len())
    }

    fn try_seek(&mut self, pos: std::io::SeekFrom) -> Result<u64> {
        Ok(std::io::Seek::seek(&mut self.data, pos)?)
    }
}

impl IoProvider for MemoryIo {
    fn output_open_name(&mut self, name: &str) -> OpenResult<OutputHandle> {
        let name = normalize_tex_path(name).to_string();
        // Truncate like a real file open-for-write: preloading the previous
        // pass's contents would leave stale tail bytes behind whenever the
        // new contents are shorter (corrupting e.g. the .xdv when a rerun
        // shrinks a resolved reference).
        let item = MemoryItem {
            files: self.files.clone(),
            name: name.clone(),
            data: Cursor::new(Vec::new()),
        };
        OpenResult::Ok(OutputHandle::new(&name, item))
    }

    fn output_open_stdout(&mut self) -> OpenResult<OutputHandle> {
        if self.stdout_allowed {
            OpenResult::Ok(OutputHandle::new("", std::io::stdout()))
        } else {
            OpenResult::NotAvailable
        }
    }

    fn input_open_name(
        &mut self,
        name: &str,
        _status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        let name = normalize_tex_path(name).to_string();
        let data = match self.files.borrow().get(&name) {
            Some(d) => d.clone(),
            None => return OpenResult::NotAvailable,
        };
        let item = MemoryItem {
            files: self.files.clone(),
            name: name.clone(),
            data: Cursor::new(data),
        };
        OpenResult::Ok(InputHandle::new(&name, item, InputOrigin::Other))
    }
}

// ---- Bridge State (DriverHooks implementation) ----

struct WasiBridgeState {
    bundle: DirBundle,
    filesystem: FilesystemIo,
    primary_input: FilesystemPrimaryInputIo,
    mem: MemoryIo,
    events: HashMap<String, ()>,
}

impl DriverHooks for WasiBridgeState {
    fn io(&mut self) -> &mut dyn IoProvider {
        self
    }

    fn event_output_closed(&mut self, name: String, _digest: DigestData) {
        self.events.insert(name, ());
    }
}

impl IoProvider for WasiBridgeState {
    fn output_open_name(&mut self, name: &str) -> OpenResult<OutputHandle> {
        self.mem.output_open_name(name)
    }

    fn output_open_stdout(&mut self) -> OpenResult<OutputHandle> {
        self.mem.output_open_stdout()
    }

    fn input_open_name(
        &mut self,
        name: &str,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        // Try memory first, then filesystem, then bundle
        let name_norm = normalize_tex_path(name).to_string();

        match self.mem.input_open_name(&name_norm, status) {
            OpenResult::Ok(h) => return OpenResult::Ok(h),
            OpenResult::Err(e) => return OpenResult::Err(e),
            OpenResult::NotAvailable => {}
        }

        match self.filesystem.input_open_name(&name_norm, status) {
            OpenResult::Ok(h) => return OpenResult::Ok(h),
            OpenResult::Err(e) => return OpenResult::Err(e),
            OpenResult::NotAvailable => {}
        }

        self.bundle.input_open_name(&name_norm, status)
    }

    fn input_open_primary(&mut self, status: &mut dyn StatusBackend) -> OpenResult<InputHandle> {
        self.primary_input.input_open_primary(status)
    }

    fn input_open_format(
        &mut self,
        name: &str,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        // Try bundle first for format files
        match self.bundle.input_open_name(name, status) {
            OpenResult::Ok(h) => return OpenResult::Ok(h),
            OpenResult::Err(e) => return OpenResult::Err(e),
            OpenResult::NotAvailable => {}
        }

        self.input_open_name(name, status)
    }

    fn write_format(
        &mut self,
        name: &str,
        data: &[u8],
        _status: &mut dyn StatusBackend,
    ) -> Result<()> {
        let cache_dir =
            std::env::var("TECTONIC_CACHE_DIR").unwrap_or_else(|_| "/cache".to_string());
        let path = Path::new(&cache_dir).join(name);
        std::fs::write(&path, data)?;
        Ok(())
    }
}

// ---- Simple status backend ----

struct WasiStatusBackend;

impl StatusBackend for WasiStatusBackend {
    fn report(&mut self, kind: MessageKind, args: std::fmt::Arguments<'_>, err: Option<&Error>) {
        let prefix = match kind {
            MessageKind::Note => "note",
            MessageKind::Warning => "warning",
            MessageKind::Error => "error",
        };
        if let Some(e) = err {
            eprintln!("tectonic {prefix}: {args}: {e}");
        } else {
            eprintln!("tectonic {prefix}: {args}");
        }
    }

    fn dump_error_logs(&mut self, output: &[u8]) {
        // In WASI, just write to stderr
        let _ = std::io::stderr().write_all(output);
    }
}

// ---- Format generation ----

/// Generate a TeX format file (e.g. latex.fmt) in initex mode.
///
/// This eliminates the need for a native tectonic install to create format files.
/// The generated format is written to `$TECTONIC_CACHE_DIR/<name>.fmt` (default `/cache/`).
pub fn generate_format(bundle_dir: &str) -> Result<()> {
    let mut status = WasiStatusBackend;

    let bundle = DirBundle::new(PathBuf::from(bundle_dir));
    let mem = MemoryIo::new(false);
    let format_input = BufferedPrimaryIo::from_text("\\input tectonic-format-latex.tex");

    let cache_dir = std::env::var("TECTONIC_CACHE_DIR").unwrap_or_else(|_| "/cache".to_string());
    let _ = std::fs::create_dir_all(&cache_dir);

    let mut bridge_state = FormatBridgeState {
        bundle,
        mem,
        format_input,
        cache_dir,
    };

    eprintln!("tectonic: generating latex.fmt (initex mode)");

    let mut launcher = CoreBridgeLauncher::new(&mut bridge_state, &mut status);

    let result = TexEngine::default()
        .initex_mode(true)
        .process(&mut launcher, "UNUSED.fmt", "latex")?;

    match result {
        TexOutcome::Errors => {
            return Err(anyhow::anyhow!(
                "TeX engine reported errors during format generation"
            ));
        }
        _ => {}
    }

    // The format file is written to MemoryIo via output_open_name during \dump.
    // Extract it and persist to the cache directory.
    let cache_dir = &bridge_state.cache_dir;
    if let Some(fmt_data) = bridge_state.mem.get_file("latex.fmt") {
        let path = Path::new(cache_dir).join("latex.fmt");
        std::fs::write(&path, &fmt_data)?;
        eprintln!(
            "tectonic: wrote {} ({} bytes)",
            path.display(),
            fmt_data.len()
        );
    } else {
        return Err(anyhow::anyhow!(
            "format generation completed but no latex.fmt was produced"
        ));
    }

    eprintln!("tectonic: format generation complete");
    Ok(())
}

/// Bridge state for format file generation (initex mode).
struct FormatBridgeState {
    bundle: DirBundle,
    mem: MemoryIo,
    format_input: BufferedPrimaryIo,
    cache_dir: String,
}

impl DriverHooks for FormatBridgeState {
    fn io(&mut self) -> &mut dyn IoProvider {
        self
    }

    fn event_output_closed(&mut self, _name: String, _digest: DigestData) {}
}

impl IoProvider for FormatBridgeState {
    fn output_open_name(&mut self, name: &str) -> OpenResult<OutputHandle> {
        self.mem.output_open_name(name)
    }

    fn output_open_stdout(&mut self) -> OpenResult<OutputHandle> {
        OpenResult::Ok(OutputHandle::new("", std::io::stdout()))
    }

    fn input_open_name(
        &mut self,
        name: &str,
        status: &mut dyn StatusBackend,
    ) -> OpenResult<InputHandle> {
        let name_norm = normalize_tex_path(name).to_string();

        match self.mem.input_open_name(&name_norm, status) {
            OpenResult::Ok(h) => return OpenResult::Ok(h),
            OpenResult::Err(e) => return OpenResult::Err(e),
            OpenResult::NotAvailable => {}
        }

        self.bundle.input_open_name(&name_norm, status)
    }

    fn input_open_primary(&mut self, status: &mut dyn StatusBackend) -> OpenResult<InputHandle> {
        self.format_input.input_open_primary(status)
    }

    fn write_format(
        &mut self,
        name: &str,
        data: &[u8],
        _status: &mut dyn StatusBackend,
    ) -> Result<()> {
        let path = Path::new(&self.cache_dir).join(name);
        std::fs::write(&path, data)?;
        eprintln!("tectonic: wrote format file {}", path.display());
        Ok(())
    }
}
