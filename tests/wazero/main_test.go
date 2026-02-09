// Copyright 2024 the Tectonic Project
// Licensed under the MIT License.

// Package main provides an integration test that loads the Tectonic WASI module
// and verifies end-to-end LaTeX compilation.
package main

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/imports/wasi_snapshot_preview1"
)

func TestTectonicCompileDefaults(t *testing.T) {
	// Locate the WASM module
	projectRoot := os.Getenv("TECTONIC_ROOT")
	if projectRoot == "" {
		// Try to find it relative to this test
		projectRoot = filepath.Join("..", "..")
	}

	wasmPath := filepath.Join(projectRoot, "target", "wasm32-wasip1", "release", "tectonic_wasi.wasm")
	wasmBytes, err := os.ReadFile(wasmPath)
	if err != nil {
		t.Skipf("WASM module not found at %s (run build-wasi.sh first): %v", wasmPath, err)
	}

	// Prepare test directories
	tmpDir := t.TempDir()

	inputDir := filepath.Join(tmpDir, "input")
	outputDir := filepath.Join(tmpDir, "output")
	bundleDir := os.Getenv("TECTONIC_BUNDLE_DIR")
	fontsDir := os.Getenv("TECTONIC_FONT_DIR")
	cacheDir := filepath.Join(tmpDir, "cache")

	if bundleDir == "" {
		t.Skip("TECTONIC_BUNDLE_DIR not set (need a TeX Live bundle directory)")
	}
	if fontsDir == "" {
		fontsDir = filepath.Join(tmpDir, "fonts")
		os.MkdirAll(fontsDir, 0o755)
	}

	for _, dir := range []string{inputDir, outputDir, cacheDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatalf("failed to create dir %s: %v", dir, err)
		}
	}

	// Write a simple test.tex
	texContent := `\documentclass{article}
\begin{document}
Hello, Tectonic WASI!
\end{document}
`
	if err := os.WriteFile(filepath.Join(inputDir, "input.tex"), []byte(texContent), 0o644); err != nil {
		t.Fatalf("failed to write test.tex: %v", err)
	}

	// Set up wazero runtime
	ctx := context.Background()

	config := wazero.NewRuntimeConfig().
		WithCloseOnContextDone(true)

	rt := wazero.NewRuntimeWithConfig(ctx, config)
	defer rt.Close(ctx)

	// Instantiate WASI
	wasi_snapshot_preview1.MustInstantiate(ctx, rt)

	// Configure the module with filesystem mounts
	fsConfig := wazero.NewFSConfig().
		WithDirMount(inputDir, "/input").
		WithDirMount(outputDir, "/output").
		WithDirMount(bundleDir, "/bundle").
		WithDirMount(fontsDir, "/fonts").
		WithDirMount(cacheDir, "/cache")

	modConfig := wazero.NewModuleConfig().
		WithStdout(os.Stdout).
		WithStderr(os.Stderr).
		WithFSConfig(fsConfig).
		WithEnv("TECTONIC_FONT_DIR", "/fonts").
		WithEnv("TECTONIC_CACHE_DIR", "/cache")

	// Compile the module
	compiled, err := rt.CompileModule(ctx, wasmBytes)
	if err != nil {
		t.Fatalf("failed to compile WASM module: %v", err)
	}

	// Instantiate with the config
	mod_, err := rt.InstantiateModule(ctx, compiled, modConfig)
	if err != nil {
		t.Fatalf("failed to instantiate module: %v", err)
	}
	defer mod_.Close(ctx)

	// Call tectonic_compile_defaults
	fn := mod_.ExportedFunction("tectonic_compile_defaults")
	if fn == nil {
		t.Fatal("exported function 'tectonic_compile_defaults' not found")
	}

	results, err := fn.Call(ctx)
	if err != nil {
		t.Fatalf("tectonic_compile_defaults call failed: %v", err)
	}

	if len(results) == 0 || results[0] != 0 {
		rc := int32(-1)
		if len(results) > 0 {
			rc = int32(results[0])
		}
		t.Fatalf("tectonic_compile_defaults returned %d (expected 0)", rc)
	}

	// Verify PDF output
	pdfPath := filepath.Join(outputDir, "input.pdf")
	pdfData, err := os.ReadFile(pdfPath)
	if err != nil {
		t.Fatalf("PDF output not found at %s: %v", pdfPath, err)
	}

	if len(pdfData) < 100 {
		t.Fatalf("PDF output is suspiciously small: %d bytes", len(pdfData))
	}

	// Check PDF magic bytes
	if string(pdfData[:5]) != "%PDF-" {
		t.Fatalf("output file does not look like a PDF (starts with %q)", string(pdfData[:5]))
	}

	t.Logf("Success! PDF output is %d bytes", len(pdfData))
}
