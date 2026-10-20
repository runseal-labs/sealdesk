package main

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/tailscale/tailcat"
)

func TestNodePublicHexMatchesNodeKey(t *testing.T) {
	pk := tailcat.NewPrivateKey()
	pk.Public.RegionID = 1
	got, err := nodePublicHex(string(pk.Public.Addr()))
	if err != nil {
		t.Fatal(err)
	}
	want := pk.Private.Public().UntypedHexString()
	if got != want {
		t.Fatalf("node public = %s, want %s", got, want)
	}
	if _, err := nodePublicHex("tcnot-an-address"); err == nil {
		t.Fatal("expected an unreadable address to fail")
	}
}

func TestServerKeyFileStoresNodeSecretAsText(t *testing.T) {
	pk := tailcat.NewPrivateKey()
	raw, err := json.Marshal(serverKeyFile{PrivateKey: *pk})
	if err != nil {
		t.Fatal(err)
	}
	var doc map[string]any
	if err := json.Unmarshal(raw, &doc); err != nil {
		t.Fatal(err)
	}
	encoded, ok := doc["Private"].(string)
	if !ok || !strings.HasPrefix(encoded, "privkey:") {
		t.Fatalf("Private = %#v", doc["Private"])
	}
}
