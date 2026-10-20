package main

import (
	"sync/atomic"
	"testing"
)

type recordingRelay struct {
	fakeTunnelClient
	on    atomic.Bool
	calls atomic.Int32
}

func (relay *recordingRelay) SetPeerRelayOnly(on bool) {
	relay.on.Store(on)
	relay.calls.Add(1)
}

func TestPeerForceRelayAppliesOnlyToThatTunnel(t *testing.T) {
	const forced = "tcPeerForced"
	const other = "tcPeerOther"
	forcedKey := forced + "\x00key"
	otherKey := other + "\x00key"
	forcedRelay := &recordingRelay{}
	otherRelay := &recordingRelay{}

	clients.mu.Lock()
	if clients.entries == nil {
		clients.entries = map[string]*clientEntry{}
	}
	clients.entries[forcedKey] = &clientEntry{client: forcedRelay, closed: make(chan struct{})}
	clients.entries[otherKey] = &clientEntry{client: otherRelay, closed: make(chan struct{})}
	clients.mu.Unlock()
	t.Cleanup(func() {
		setPeerForceRelay(forced, false)
		setPeerForceRelay(other, false)
		clients.mu.Lock()
		delete(clients.entries, forcedKey)
		delete(clients.entries, otherKey)
		clients.mu.Unlock()
	})

	if peerRelayOnlyEnabled(forced) || peerRelayOnlyEnabled(other) {
		t.Fatal("force relay defaults on")
	}
	setPeerForceRelay(forced, true)
	if !peerRelayOnlyEnabled(forced) || peerRelayOnlyEnabled(other) {
		t.Fatal("force relay was not stored for only the selected peer")
	}
	if !forcedRelay.on.Load() || forcedRelay.calls.Load() != 1 || otherRelay.calls.Load() != 0 {
		t.Fatalf("live tunnels: forced on=%v calls=%d other calls=%d", forcedRelay.on.Load(), forcedRelay.calls.Load(), otherRelay.calls.Load())
	}
	setPeerForceRelay(forced, false)
	if peerRelayOnlyEnabled(forced) || forcedRelay.on.Load() {
		t.Fatal("clearing force relay left the peer on DERP")
	}
	if otherRelay.calls.Load() != 0 {
		t.Fatal("clearing one peer notified another tunnel")
	}
}
