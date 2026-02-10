// Copyright 2024 the Tectonic Project
// Licensed under the MIT License.

// Full workflow integration test: download bundle → generate format (WASM) →
// compile document (WASM) → verify PDF. Proves no native tectonic install is
// needed.
package main

import (
	"archive/tar"
	"context"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"testing"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/imports/wasi_snapshot_preview1"
)

const bundleTarURL = "https://data1.fullyjustified.net/tlextras-2022.0r0.tar"

// fetchBundle downloads and extracts the TeX Live bundle tar to a local cache
// directory. Subsequent calls reuse the cached extraction.
func fetchBundle(t *testing.T) string {
	t.Helper()

	cacheDir := filepath.Join(os.TempDir(), "tectonic-test-bundle")
	markerFile := filepath.Join(cacheDir, ".extracted")

	if _, err := os.Stat(markerFile); err == nil {
		t.Log("Using cached bundle from", cacheDir)
		return cacheDir
	}

	if err := os.MkdirAll(cacheDir, 0o755); err != nil {
		t.Fatalf("failed to create cache dir: %v", err)
	}

	t.Logf("Downloading bundle from %s ...", bundleTarURL)
	resp, err := http.Get(bundleTarURL)
	if err != nil {
		t.Skipf("failed to download bundle (network issue?): %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Skipf("bundle download returned HTTP %d", resp.StatusCode)
	}

	t.Logf("Extracting bundle (%.1f GB) ...", float64(resp.ContentLength)/(1024*1024*1024))

	tr := tar.NewReader(resp.Body)
	count := 0
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatalf("tar read error after %d files: %v", count, err)
		}

		if hdr.Typeflag != tar.TypeReg {
			continue
		}

		name := filepath.Base(hdr.Name)
		if name == "." || name == ".." {
			continue
		}

		dest := filepath.Join(cacheDir, name)
		f, err := os.Create(dest)
		if err != nil {
			t.Fatalf("failed to create %s: %v", name, err)
		}
		if _, err := io.Copy(f, tr); err != nil {
			f.Close()
			t.Fatalf("failed to write %s: %v", name, err)
		}
		f.Close()
		count++

		if count%25000 == 0 {
			t.Logf("  extracted %d files ...", count)
		}
	}

	t.Logf("Extracted %d files to %s", count, cacheDir)

	if err := os.WriteFile(markerFile, []byte("ok"), 0o644); err != nil {
		t.Fatalf("failed to write marker: %v", err)
	}

	return cacheDir
}

// TestFullWorkflow verifies the complete WASI workflow:
//  1. Fetch the TeX Live bundle (download + extract tar)
//  2. Generate latex.fmt via tectonic_generate_format() — no native tectonic!
//  3. Compile a LaTeX document via tectonic_compile_defaults()
//  4. Verify the resulting PDF
func TestFullWorkflow(t *testing.T) {
	if os.Getenv("TECTONIC_TEST_FULL_WORKFLOW") == "" {
		t.Skip("skipping: set TECTONIC_TEST_FULL_WORKFLOW=1 to enable (downloads ~3 GB bundle)")
	}

	// ---- Step 1: Get the bundle ----
	bundleDir := fetchBundle(t)

	// Remove any pre-existing format file to prove we generate it ourselves.
	os.Remove(filepath.Join(bundleDir, "latex.fmt"))

	// ---- Step 2: Set up working directories ----
	tmpDir := t.TempDir()
	inputDir := filepath.Join(tmpDir, "input")
	outputDir := filepath.Join(tmpDir, "output")
	fontsDir := filepath.Join(tmpDir, "fonts")
	fmtCacheDir := filepath.Join(tmpDir, "cache")

	for _, d := range []string{inputDir, outputDir, fontsDir, fmtCacheDir} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatalf("mkdir %s: %v", d, err)
		}
	}

	if err := os.WriteFile(filepath.Join(inputDir, "input.tex"), []byte(
		"\\documentclass{article}\n\\begin{document}\nHello from the full WASI workflow!\n\\end{document}\n",
	), 0o644); err != nil {
		t.Fatalf("write input.tex: %v", err)
	}

	// ---- Step 3: Load WASM module ----
	projectRoot := filepath.Join("..", "..")
	wasmPath := filepath.Join(projectRoot, "target", "wasm32-wasip1", "release", "tectonic_wasi.wasm")
	wasmBytes, err := os.ReadFile(wasmPath)
	if err != nil {
		t.Skipf("WASM module not found at %s (run build-wasi.sh first): %v", wasmPath, err)
	}

	ctx := context.Background()
	rt := wazero.NewRuntimeWithConfig(ctx, wazero.NewRuntimeConfig().WithCloseOnContextDone(true))
	defer rt.Close(ctx)

	wasi_snapshot_preview1.MustInstantiate(ctx, rt)

	compiled, err := rt.CompileModule(ctx, wasmBytes)
	if err != nil {
		t.Fatalf("compile WASM: %v", err)
	}

	// ---- Step 4: Generate format via WASM ----
	t.Log("Phase 1: Generating latex.fmt via tectonic_generate_format() ...")
	{
		modConfig := wazero.NewModuleConfig().
			WithStdout(os.Stdout).
			WithStderr(os.Stderr).
			WithName("tectonic-fmt").
			WithFSConfig(wazero.NewFSConfig().
				WithDirMount(bundleDir, "/bundle").
				WithDirMount(fontsDir, "/fonts").
				WithDirMount(fmtCacheDir, "/cache"))

		mod, err := rt.InstantiateModule(ctx, compiled, modConfig)
		if err != nil {
			t.Fatalf("instantiate for format gen: %v", err)
		}

		fn := mod.ExportedFunction("tectonic_generate_format")
		if fn == nil {
			t.Fatal("tectonic_generate_format export not found")
		}

		results, err := fn.Call(ctx)
		if err != nil {
			t.Fatalf("tectonic_generate_format failed: %v", err)
		}
		if len(results) == 0 || results[0] != 0 {
			rc := int32(-1)
			if len(results) > 0 {
				rc = int32(results[0])
			}
			t.Fatalf("tectonic_generate_format returned %d (expected 0)", rc)
		}

		mod.Close(ctx)
	}

	// Verify format file was created.
	fmtPath := filepath.Join(fmtCacheDir, "latex.fmt")
	fmtData, err := os.ReadFile(fmtPath)
	if err != nil {
		// The job name might produce a different filename — check what's there.
		entries, _ := os.ReadDir(fmtCacheDir)
		names := make([]string, 0, len(entries))
		for _, e := range entries {
			names = append(names, e.Name())
		}
		t.Fatalf("latex.fmt not generated in cache dir (files: %v): %v", names, err)
	}
	t.Logf("Phase 1 complete: latex.fmt is %d bytes", len(fmtData))

	// Copy format to bundle so the compiler finds it via input_open_format.
	if err := os.WriteFile(filepath.Join(bundleDir, "latex.fmt"), fmtData, 0o644); err != nil {
		t.Fatalf("copy format to bundle: %v", err)
	}

	// ---- Step 5: Compile document via WASM ----
	t.Log("Phase 2: Compiling document via tectonic_compile_defaults() ...")
	{
		modConfig := wazero.NewModuleConfig().
			WithStdout(os.Stdout).
			WithStderr(os.Stderr).
			WithName("tectonic-compile").
			WithFSConfig(wazero.NewFSConfig().
				WithDirMount(inputDir, "/input").
				WithDirMount(outputDir, "/output").
				WithDirMount(bundleDir, "/bundle").
				WithDirMount(fontsDir, "/fonts").
				WithDirMount(fmtCacheDir, "/cache"))

		mod, err := rt.InstantiateModule(ctx, compiled, modConfig)
		if err != nil {
			t.Fatalf("instantiate for compile: %v", err)
		}

		fn := mod.ExportedFunction("tectonic_compile_defaults")
		if fn == nil {
			t.Fatal("tectonic_compile_defaults export not found")
		}

		results, err := fn.Call(ctx)
		if err != nil {
			t.Fatalf("tectonic_compile_defaults failed: %v", err)
		}
		if len(results) == 0 || results[0] != 0 {
			rc := int32(-1)
			if len(results) > 0 {
				rc = int32(results[0])
			}
			t.Fatalf("tectonic_compile_defaults returned %d (expected 0)", rc)
		}

		mod.Close(ctx)
	}

	// ---- Step 6: Verify PDF ----
	pdfPath := filepath.Join(outputDir, "input.pdf")
	pdfData, err := os.ReadFile(pdfPath)
	if err != nil {
		t.Fatalf("PDF not found: %v", err)
	}

	if len(pdfData) < 100 {
		t.Fatalf("PDF too small: %d bytes", len(pdfData))
	}

	if string(pdfData[:5]) != "%PDF-" {
		t.Fatalf("output doesn't start with %%PDF- magic bytes")
	}

	t.Logf("Success! Full WASI workflow produced a %d-byte PDF", len(pdfData))
	t.Log("Proven: fetch bundle → generate format (WASM) → compile (WASM) → PDF")
}
