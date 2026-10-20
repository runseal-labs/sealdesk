# Vendored tailscale and tailcat

SealDesk links these trees instead of the module-cache copies so magicsock can
keep a single peer on DERP. Bases, for the next upstream sync:

- `tailscale` `a0e471a35b8f` (`tailscale.com v1.103.0-pre.0.20260929142145-a0e471a35b8f`)
- `tailcat` `52fbad323e9d` (`github.com/tailscale/tailcat v0.7.1-0.20261007201807-52fbad323e9d`)

Files changed on top of those commits:

- `tailscale/wgengine/magicsock/magicsock.go`
- `tailscale/wgengine/magicsock/endpoint.go`
- `tailscale/wgengine/magicsock/relayonly.go` (new)
- `tailscale/wgengine/magicsock/relayonly_test.go` (new)
- `tailcat/tailcat.go`
- `tailcat/relayonly_test.go` (new)
- `tailcat/go.mod` (`replace tailscale.com => ../tailscale`)

`Conn.SetPeerRelayOnly(nodeKey, true)` skips direct disco send and answer for
that node key, refuses a direct best path, and clears one that is already
installed. Other node keys are unchanged. `Client.SetPeerRelayOnly` applies
that to the server peer and stops UDP endpoint advertisements while it is on.
