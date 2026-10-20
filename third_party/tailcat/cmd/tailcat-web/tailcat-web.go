// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

// The tailcat-web command is a development server for the tailcat
// browser app in the web/ directory. It builds the dist files
// (js/wasm binary, wasm_exec.js, and the static page) at startup and
// serves them via the webdemo package, plus a same-origin
// /derpmap.json proxy so the browser's DERP map fetch (which sends a
// Tailcat-Mode header) doesn't require CORS support upstream.
package main

import (
	"flag"
	"io"
	"log"
	"net/http"
	"os"
	"time"

	"github.com/tailscale/tailcat"
	"github.com/tailscale/tailcat/internal/wasmbuild"
	"github.com/tailscale/tailcat/webdemo"
)

var (
	flagListen     = flag.String("listen", "localhost:8080", "HTTP listen address")
	flagDERPMapURL = flag.String("derpmap-url", tailcat.DefaultDERPMapURL, "upstream URL of the JSON DERP map, proxied at /derpmap.json")
	flagWebDir     = flag.String("web-dir", "web", "path to the web/ directory with index.html and app.js")
)

func main() {
	flag.Parse()

	distDir, err := os.MkdirTemp("", "tailcat-web")
	if err != nil {
		log.Fatal(err)
	}
	defer os.RemoveAll(distDir)

	t0 := time.Now()
	log.Printf("building dist in %s ...", distDir)
	if err := wasmbuild.Dist(*flagWebDir, distDir); err != nil {
		log.Fatal(err)
	}
	log.Printf("built dist in %v", time.Since(t0).Round(time.Millisecond))

	app, err := webdemo.Handler(os.DirFS(distDir))
	if err != nil {
		log.Fatal(err)
	}

	mux := http.NewServeMux()
	mux.Handle("/", app)
	mux.HandleFunc("/derpmap.json", func(w http.ResponseWriter, r *http.Request) {
		req, err := http.NewRequestWithContext(r.Context(), "GET", *flagDERPMapURL, nil)
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		req.Header.Set("Tailcat-Mode", r.Header.Get("Tailcat-Mode"))
		res, err := http.DefaultClient.Do(req)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadGateway)
			return
		}
		defer res.Body.Close()
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(res.StatusCode)
		io.Copy(w, res.Body)
	})

	log.Printf("serving tailcat web app at http://%s/", *flagListen)
	log.Fatal(http.ListenAndServe(*flagListen, mux))
}
