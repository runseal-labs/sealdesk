// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package webdemo

import (
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"testing/fstest"
)

func fakeDist() fstest.MapFS {
	return fstest.MapFS{
		"index.html":    {Data: []byte("<html>demo</html>")},
		"app.js":        {Data: []byte("// app")},
		"wasm_exec.js":  {Data: []byte("// exec")},
		"main.wasm":     {Data: []byte("wasm-uncompressed")},
		"main.wasm.zst": {Data: []byte("wasm-zst")},
		"main.wasm.gz":  {Data: []byte("wasm-gzip!")},
	}
}

func TestHandlerIncompleteDist(t *testing.T) {
	dist := fakeDist()
	delete(dist, "main.wasm")
	if _, err := Handler(dist); err == nil {
		t.Fatal("Handler succeeded with incomplete dist; want error")
	}
}

func TestHandler(t *testing.T) {
	h, err := Handler(fakeDist())
	if err != nil {
		t.Fatal(err)
	}
	srv := httptest.NewServer(h)
	defer srv.Close()

	get := func(path, acceptEncoding string) *http.Response {
		t.Helper()
		req, err := http.NewRequest("GET", srv.URL+path, nil)
		if err != nil {
			t.Fatal(err)
		}
		if acceptEncoding != "" {
			req.Header.Set("Accept-Encoding", acceptEncoding)
		}
		// The default transport would transparently decompress;
		// setting Accept-Encoding explicitly disables that.
		res, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { res.Body.Close() })
		return res
	}

	tests := []struct {
		path     string
		ae       string
		wantBody string
		wantHdr  map[string]string
	}{
		{path: "/", wantBody: "<html>demo</html>"},
		{path: "/app.js", wantBody: "// app"},
		{path: "/wasm_exec.js", wantBody: "// exec"},
		{
			path: "/main.wasm", ae: "zstd, gzip", wantBody: "wasm-zst",
			wantHdr: map[string]string{
				"Content-Type":        "application/wasm",
				"Content-Encoding":    "zstd",
				"X-Uncompressed-Size": "17",
				"X-Compressed-Size":   "8",
			},
		},
		{
			path: "/main.wasm", ae: "gzip", wantBody: "wasm-gzip!",
			wantHdr: map[string]string{
				"Content-Encoding":  "gzip",
				"X-Compressed-Size": "10",
			},
		},
		{
			path: "/main.wasm", ae: "identity", wantBody: "wasm-uncompressed",
			wantHdr: map[string]string{
				"Content-Encoding":  "",
				"X-Compressed-Size": "17",
			},
		},
	}
	for _, tt := range tests {
		res := get(tt.path, tt.ae)
		if res.StatusCode != 200 {
			t.Errorf("GET %s (ae=%q): status %v; want 200", tt.path, tt.ae, res.StatusCode)
			continue
		}
		body, err := io.ReadAll(res.Body)
		if err != nil {
			t.Fatal(err)
		}
		if string(body) != tt.wantBody {
			t.Errorf("GET %s (ae=%q): body %q; want %q", tt.path, tt.ae, body, tt.wantBody)
		}
		for k, v := range tt.wantHdr {
			if got := res.Header.Get(k); got != v {
				t.Errorf("GET %s (ae=%q): header %s = %q; want %q", tt.path, tt.ae, k, got, v)
			}
		}
	}

	if res := get("/nope", ""); res.StatusCode != 404 {
		t.Errorf("GET /nope: status %v; want 404", res.StatusCode)
	}
	if res := get("/main.wasm.zst", ""); res.StatusCode != 404 {
		t.Errorf("GET /main.wasm.zst: status %v; want 404", res.StatusCode)
	}

	// index.html must reference only relative asset URLs so the
	// handler can be mounted under a path prefix.
	res := get("/", "")
	b, _ := io.ReadAll(res.Body)
	if strings.Contains(string(b), `src="/`) {
		t.Errorf("index.html has absolute asset URLs; they must stay relative")
	}
}
