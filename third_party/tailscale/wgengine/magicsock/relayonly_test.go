// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package magicsock

import (
	"net/netip"
	"testing"
	"time"

	"go4.org/mem"
	"tailscale.com/disco"
	"tailscale.com/tailcfg"
	"tailscale.com/tstime/mono"
	"tailscale.com/types/key"
)

func TestPeerRelayOnlyDropsDirectPathWithoutAffectingOthers(t *testing.T) {
	conn := newTestConn(t)
	conn.SetPrivateKey(key.NodePrivateFromRaw32(mem.B([]byte{0: 1, 31: 0})))

	relayKey := key.NodePublicFromRaw32(mem.B([]byte{0: 'R', 31: 1}))
	relayDisco := key.DiscoPublicFromRaw32(mem.B([]byte{31: 2}))
	otherKey := key.NodePublicFromRaw32(mem.B([]byte{0: 'O', 31: 1}))
	otherDisco := key.DiscoPublicFromRaw32(mem.B([]byte{31: 3}))
	direct := netip.MustParseAddrPort("127.0.0.1:12345")
	otherDirect := netip.MustParseAddrPort("127.0.0.1:12346")

	conn.SetNetworkMap(tailcfg.NodeView{}, nodeViews([]*tailcfg.Node{
		{
			ID:        1,
			Key:       relayKey,
			DiscoKey:  relayDisco,
			HomeDERP:  1,
			Endpoints: []netip.AddrPort{direct},
		},
		{
			ID:        2,
			Key:       otherKey,
			DiscoKey:  otherDisco,
			HomeDERP:  1,
			Endpoints: []netip.AddrPort{otherDirect},
		},
	}))

	relayEP, ok := conn.peerMap.endpointForNodeKey(relayKey)
	if !ok {
		t.Fatal("missing relay peer")
	}
	otherEP, ok := conn.peerMap.endpointForNodeKey(otherKey)
	if !ok {
		t.Fatal("missing other peer")
	}
	installDirect(relayEP, direct)
	installDirect(otherEP, otherDirect)

	if udp, derp, _ := sendPath(relayEP); !udp.ap.IsValid() || udp.ap != direct || derp.IsValid() {
		t.Fatalf("relay peer path before force = udp %v derp %v", udp, derp)
	}

	conn.SetPeerRelayOnly(relayKey, true)

	udp, derp, _ := sendPath(relayEP)
	if udp.ap.IsValid() || !derp.IsValid() || derp.Addr() != tailcfg.DerpMagicIPAddr {
		t.Fatalf("forced peer path = udp %v derp %v; want DERP only", udp, derp)
	}
	relayEP.mu.Lock()
	if relayEP.bestAddr.ap.IsValid() {
		t.Fatalf("direct best path still installed: %v", relayEP.bestAddr)
	}
	probesBefore := len(relayEP.sentPing)
	relayEP.startDiscoPingLocked(epAddr{ap: direct}, mono.Now(), pingDiscovery, 0, nil)
	if len(relayEP.sentPing) != probesBefore {
		t.Fatal("relay-only peer sent a direct disco ping")
	}
	relayEP.mu.Unlock()

	pingFrom := netip.MustParseAddrPort("127.0.0.1:9")
	conn.mu.Lock()
	conn.handlePingLocked(&disco.Ping{TxID: [12]byte{1}, NodeKey: relayKey}, epAddr{ap: pingFrom}, &discoInfo{discoKey: relayDisco}, key.NodePublic{})
	conn.mu.Unlock()
	relayEP.mu.Lock()
	if _, learned := relayEP.endpointState[pingFrom]; learned {
		t.Fatal("relay-only peer learned a direct disco candidate")
	}
	relayEP.mu.Unlock()

	if udp, _, _ := sendPath(otherEP); udp.ap != otherDirect {
		t.Fatalf("other peer direct path changed: %v", udp.ap)
	}
	otherEP.mu.Lock()
	before := len(otherEP.sentPing)
	otherEP.startDiscoPingLocked(epAddr{ap: otherDirect}, mono.Now(), pingDiscovery, 0, nil)
	sent := len(otherEP.sentPing) > before
	otherEP.mu.Unlock()
	if !sent {
		t.Fatal("other peer could not send a direct disco ping")
	}

	conn.SetPeerRelayOnly(relayKey, false)
	installDirect(relayEP, direct)
	if udp, _, _ := sendPath(relayEP); udp.ap != direct {
		t.Fatalf("direct path did not return after relay-only was cleared: %v", udp.ap)
	}
	relayEP.mu.Lock()
	before = len(relayEP.sentPing)
	relayEP.startDiscoPingLocked(epAddr{ap: direct}, mono.Now(), pingDiscovery, 0, nil)
	sent = len(relayEP.sentPing) > before
	relayEP.mu.Unlock()
	if !sent {
		t.Fatal("peer did not resume direct disco after relay-only was cleared")
	}
}

func installDirect(ep *endpoint, addr netip.AddrPort) {
	ep.mu.Lock()
	defer ep.mu.Unlock()
	ep.bestAddr = addrQuality{
		epAddr:  epAddr{ap: addr},
		latency: time.Millisecond,
	}
	ep.trustBestAddrUntil = mono.Now().Add(time.Hour)
}

func sendPath(ep *endpoint) (epAddr, netip.AddrPort, bool) {
	ep.mu.Lock()
	defer ep.mu.Unlock()
	return ep.addrForSendLocked(mono.Now())
}
