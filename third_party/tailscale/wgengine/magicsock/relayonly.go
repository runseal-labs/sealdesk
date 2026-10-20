// Copyright (c) Tailscale Inc & contributors
// SPDX-License-Identifier: BSD-3-Clause

package magicsock

import "tailscale.com/types/key"

// SetPeerRelayOnly forces peer onto DERP when on is true. Direct disco probes
// are neither sent nor answered, an existing direct path is dropped
// immediately, and other peers are left alone. Turning it off allows direct
// path discovery again.
func (c *Conn) SetPeerRelayOnly(peer key.NodePublic, on bool) {
	if peer.IsZero() {
		return
	}
	for {
		old := c.relayOnly.Load()
		next := map[key.NodePublic]struct{}{}
		if old != nil {
			for k, v := range *old {
				next[k] = v
			}
		}
		if on {
			next[peer] = struct{}{}
		} else {
			delete(next, peer)
		}
		if c.relayOnly.CompareAndSwap(old, &next) {
			break
		}
	}
	if on {
		c.dropDirectPath(peer)
	}
}

func (c *Conn) peerRelayOnly(peer key.NodePublic) bool {
	if peer.IsZero() {
		return false
	}
	set := c.relayOnly.Load()
	if set == nil {
		return false
	}
	_, ok := (*set)[peer]
	return ok
}

// directDiscoBlocked reports whether a direct (non-DERP) disco message for
// discoKey should be ignored. An explicit node key blocks only that peer. An
// ambiguous disco key blocks only when every matching peer is relay-only, so
// a sibling that still wants a direct path is not silenced.
func (c *Conn) directDiscoBlocked(discoKey key.DiscoPublic, nodeHint key.NodePublic) bool {
	if !nodeHint.IsZero() {
		return c.peerRelayOnly(nodeHint)
	}
	matched, blocked := 0, 0
	c.peerMap.forEachEndpointWithDiscoKey(discoKey, func(ep *endpoint) bool {
		matched++
		if c.peerRelayOnly(ep.publicKey) {
			blocked++
		}
		return true
	})
	return matched > 0 && matched == blocked
}

func (de *endpoint) directPathBlocked() bool {
	return de.c != nil && de.c.peerRelayOnly(de.publicKey)
}

func (c *Conn) dropDirectPath(peer key.NodePublic) {
	c.mu.Lock()
	ep, ok := c.peerMap.endpointForNodeKey(peer)
	c.mu.Unlock()
	if !ok {
		return
	}
	ep.mu.Lock()
	defer ep.mu.Unlock()
	ep.clearBestAddrLocked()
}
