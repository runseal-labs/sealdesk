// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

// The tailcat-webdist command builds the distribution directory of
// static files needed to serve the tailcat browser app: index.html,
// app.js, wasm_exec.js, and the js/wasm main.wasm binary with
// precompressed .zst and .gz variants. Servers such as
// cmd/tailcat-web and tailcat.dev's tailcatmd serve the result via
// the webdemo package.
package main

import (
	"flag"
	"log"
	"time"

	"github.com/tailscale/tailcat/internal/wasmbuild"
)

var (
	flagOut    = flag.String("o", "", "output directory for the dist files (required)")
	flagWebDir = flag.String("web-dir", "web", "path to the web/ directory with index.html and app.js")
)

func main() {
	flag.Parse()
	if *flagOut == "" {
		log.Fatal("tailcat-webdist: -o output directory is required")
	}
	t0 := time.Now()
	if err := wasmbuild.Dist(*flagWebDir, *flagOut); err != nil {
		log.Fatal(err)
	}
	log.Printf("built dist in %s in %v", *flagOut, time.Since(t0).Round(time.Millisecond))
}
