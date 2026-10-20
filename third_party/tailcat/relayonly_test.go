// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package tailcat

import "testing"

func TestSetPeerRelayOnlyBeforeStart(t *testing.T) {
	client := NewClient(Addr("tcnotused"))
	if client.relayOnly.Load() {
		t.Fatal("relay-only defaults on")
	}
	client.SetPeerRelayOnly(true)
	if !client.relayOnly.Load() {
		t.Fatal("relay-only did not stick before start")
	}
	client.SetPeerRelayOnly(false)
	if client.relayOnly.Load() {
		t.Fatal("clearing relay-only before start failed")
	}
}
