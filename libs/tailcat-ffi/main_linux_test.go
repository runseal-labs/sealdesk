package main

import (
	"os"
	"testing"
)

func TestSaveJSONUsesPrivateMode(t *testing.T) {
	path := t.TempDir() + "/tailcat-key.json"
	if err := saveJSON(path, map[string]string{"key": "value"}); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0o600 {
		t.Fatalf("key file mode = %o, want 600", got)
	}
}
