// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

// Package webdemo serves the tailcat browser app (the js/wasm build
// of tailcat in the web/ directory) from a distribution directory of
// prebuilt static files, as produced by cmd/tailcat-webdist. It is
// used by cmd/tailcat-web, the browser integration tests, and
// external servers that host the demo (such as tailcat.dev).
package webdemo

import (
	"fmt"
	"io/fs"
	"net/http"
	"strings"
)

// distFiles are the files a dist FS must contain.
var distFiles = []string{"index.html", "app.js", "wasm_exec.js", "main.wasm"}

// Handler returns an HTTP handler serving the web app from dist, a
// filesystem containing the files built by wasmbuild.Dist: index.html
// at "/", app.js, wasm_exec.js, and main.wasm, the latter served
// precompressed (from main.wasm.zst or main.wasm.gz, if present in
// dist) when the client's Accept-Encoding allows.
//
// The page's asset URLs are all relative, so the handler may be
// mounted under a path prefix with http.StripPrefix.
func Handler(dist fs.FS) (http.Handler, error) {
	sizes := map[string]int64{}
	for _, name := range distFiles {
		fi, err := fs.Stat(dist, name)
		if err != nil {
			return nil, fmt.Errorf("webdemo: incomplete dist: %w", err)
		}
		sizes[name] = fi.Size()
	}
	for _, name := range []string{"main.wasm.zst", "main.wasm.gz"} {
		if fi, err := fs.Stat(dist, name); err == nil {
			sizes[name] = fi.Size()
		}
	}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /{$}", func(w http.ResponseWriter, r *http.Request) {
		http.ServeFileFS(w, r, dist, "index.html")
	})
	mux.HandleFunc("GET /app.js", func(w http.ResponseWriter, r *http.Request) {
		http.ServeFileFS(w, r, dist, "app.js")
	})
	mux.HandleFunc("GET /wasm_exec.js", func(w http.ResponseWriter, r *http.Request) {
		http.ServeFileFS(w, r, dist, "wasm_exec.js")
	})
	mux.HandleFunc("GET /main.wasm", func(w http.ResponseWriter, r *http.Request) {
		// The wasm binary is tens of MB; serve it precompressed.
		// Content-Type is set before ServeFileFS so it isn't sniffed
		// from the compressed file's extension. The transfer size
		// goes in X-Compressed-Size because reverse proxies may drop
		// Content-Length, and the page can't compute it itself: its
		// body stream sees only decompressed bytes.
		w.Header().Set("Content-Type", "application/wasm")
		w.Header().Set("Vary", "Accept-Encoding")
		w.Header().Set("X-Uncompressed-Size", fmt.Sprint(sizes["main.wasm"]))
		name := "main.wasm"
		ae := r.Header.Get("Accept-Encoding")
		switch {
		case strings.Contains(ae, "zstd") && sizes["main.wasm.zst"] > 0:
			w.Header().Set("Content-Encoding", "zstd")
			name += ".zst"
		case strings.Contains(ae, "gzip") && sizes["main.wasm.gz"] > 0:
			w.Header().Set("Content-Encoding", "gzip")
			name += ".gz"
		}
		w.Header().Set("X-Compressed-Size", fmt.Sprint(sizes[name]))
		http.ServeFileFS(w, r, dist, name)
	})
	return mux, nil
}
