package main

import (
	"bytes"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/tailscale/tailcat"
)

func TestNodeKeyPersistedBeforeRelayBootstrap(t *testing.T) {
	mu.Lock()
	if server != nil {
		mu.Unlock()
		t.Fatal("server already running")
	}
	savedErr := lastInitErr
	mu.Unlock()
	t.Cleanup(func() {
		if stop := TcStopServe(); stop != nil {
			TcFree(stop)
		}
		mu.Lock()
		lastInitErr = savedErr
		mu.Unlock()
	})

	refused, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	refusedAddress := refused.Addr().String()
	refused.Close()
	source := "https://" + refusedAddress + "/map"
	token := strings.Repeat("ab", 32)

	dir := t.TempDir()
	path := filepath.Join(dir, "tailcat-server.json")
	if _, err := serveWithDERPMap(path, "", token, source, 21118); err == nil || strings.Contains(err.Error(), "privkey:") {
		t.Fatal("unreachable relay should fail without exposing the node key", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal("node key was not written before relay bootstrap", err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("key file mode = %o, want 600", info.Mode().Perm())
	}
	loaded, fresh, err := loadOrCreateKey(path)
	if err != nil || fresh || loaded.Private.IsZero() {
		t.Fatal("pre-bootstrap key file was not readable", err, fresh)
	}
	if loaded.Public.RegionID != 0 || len(loaded.Public.Region) != 0 {
		t.Fatal("region was pinned before relay bootstrap")
	}
	raw, err := os.ReadFile(path)
	if err != nil || !bytes.Contains(raw, []byte(`"Private"`)) || !bytes.Contains(raw, []byte("privkey:")) {
		t.Fatal("key file is missing the node private key")
	}
	if _, err := serveWithDERPMap(path, "", token, source, 21118); err == nil {
		t.Fatal("second bootstrap attempt should still fail closed")
	}
	reloaded, fresh, err := loadOrCreateKey(path)
	if err != nil || fresh || !reloaded.Private.Equal(loaded.Private) || reloaded.Public.PresharedKey != loaded.Public.PresharedKey {
		t.Fatal("a second failed bootstrap changed the node key", err, fresh)
	}
	if reloaded.Public.RegionID != 0 || len(reloaded.Public.Region) != 0 {
		t.Fatal("failed bootstrap pinned a region")
	}

	partial := filepath.Join(dir, "partial.json")
	privateText, err := loaded.Private.MarshalText()
	if err != nil {
		t.Fatal(err)
	}
	partialBody, err := json.Marshal(struct {
		Private string
	}{Private: string(privateText)})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(partial, partialBody, 0o600); err != nil {
		t.Fatal(err)
	}
	fromPartial, fresh, err := loadOrCreateKey(partial)
	if err != nil || fresh || !fromPartial.Private.Equal(loaded.Private) {
		t.Fatal("key-only file was not read", err, fresh)
	}
	if fromPartial.Public.RegionID != 0 || len(fromPartial.Public.Region) != 0 {
		t.Fatal("key-only file gained a region")
	}
	beforePartial, err := os.ReadFile(partial)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := serveWithDERPMap(partial, "", token, source, 21118); err == nil {
		t.Fatal("bootstrap of a key-only file should still fail closed")
	}
	afterPartial, err := os.ReadFile(partial)
	if err != nil || !bytes.Equal(beforePartial, afterPartial) {
		t.Fatal("key-only file was overwritten when the relay was unreachable")
	}

	existing := filepath.Join(dir, "existing.json")
	original := tailcat.NewPrivateKey()
	original.Public.RegionID = 4
	if err := saveJSON(existing, &serverKeyFile{PrivateKey: *original}); err != nil {
		t.Fatal(err)
	}
	beforeExisting, err := os.ReadFile(existing)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := serveWithDERPMap(existing, "", token, source, 21118); err == nil {
		t.Fatal("bootstrap with an existing key should still fail closed")
	}
	afterExisting, err := os.ReadFile(existing)
	if err != nil || !bytes.Equal(beforeExisting, afterExisting) {
		t.Fatal("existing node key was overwritten")
	}
	kept, fresh, err := loadOrCreateKey(existing)
	if err != nil || fresh || !kept.Private.Equal(original.Private) || kept.Public.RegionID != 4 {
		t.Fatal("existing node key did not survive a failed bootstrap", err, fresh)
	}
}
