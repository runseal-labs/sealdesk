package main

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/tailscale/tailcat"
)

func TestNodeKeyPersistedBeforeRelayBootstrap(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "tailcat-server.json")
	pk, fresh, err := loadOrCreateKey(path)
	if err != nil || !fresh || pk.Private.IsZero() {
		t.Fatal("expected a fresh in-memory key", fresh, err)
	}
	if pk.Public.RegionID != 0 || len(pk.Public.Region) != 0 {
		t.Fatal("fresh key pinned a region")
	}

	persistFreshNodeKey(path, pk, true)
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal("node key was not written before relay bootstrap")
	}
	// Windows has no POSIX mode bits; saveJSON still asks for 0600, and Go
	// reports the synthesized permission (typically 0666).
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("key file mode = %o, want 600", info.Mode().Perm())
	}
	loaded, fresh, err := loadOrCreateKey(path)
	if err != nil || fresh || loaded.Private.IsZero() || !loaded.Private.Equal(pk.Private) || loaded.Public.PresharedKey != pk.Public.PresharedKey {
		t.Fatal("pre-bootstrap key file was not readable", fresh, err)
	}
	if loaded.Public.RegionID != 0 || len(loaded.Public.Region) != 0 {
		t.Fatal("region was pinned before relay bootstrap")
	}

	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	persistFreshNodeKey(path, pk, false)
	after, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatal("a second persist of an existing key rewrote the file")
	}

	existing := filepath.Join(dir, "existing.json")
	original := tailcat.NewPrivateKey()
	original.Public.RegionID = 4
	stored := &serverKeyFile{PrivateKey: *original}
	if err := saveJSON(existing, stored); err != nil {
		t.Fatal(err)
	}
	beforeExisting, err := os.ReadFile(existing)
	if err != nil {
		t.Fatal(err)
	}
	persistFreshNodeKey(existing, stored, false)
	afterExisting, err := os.ReadFile(existing)
	if err != nil || !bytes.Equal(beforeExisting, afterExisting) {
		t.Fatal("existing node key was overwritten")
	}
	kept, fresh, err := loadOrCreateKey(existing)
	if err != nil || fresh || !kept.Private.Equal(original.Private) || kept.Public.RegionID != 4 {
		t.Fatal("existing node key did not survive", fresh, err)
	}
}
