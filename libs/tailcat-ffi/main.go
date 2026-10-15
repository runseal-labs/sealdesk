// Package main is built with -buildmode=c-archive and linked into RustDesk.
// It replaces the rendezvous / relay servers with a tailcat tunnel: DERP is
// only used to meet, then WireGuard upgrades to a direct UDP path.
//
// Every exported string result is malloc'ed; free it with TcFree. Errors are
// returned as strings with the "ERR:" prefix.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unsafe"

	"github.com/tailscale/tailcat"
	"rsc.io/qr"
	"tailscale.com/types/key"
)

var (
	mu          sync.Mutex
	server      *tailcat.Server
	serverConns *incomingConnections
	clients     clientCache
)

func cstr(s string) *C.char { return C.CString(s) }

func cerr(err error) *C.char { return cstr("ERR:" + err.Error()) }

func logf(format string, args ...any) { log.Printf("tailcat: "+format, args...) }

//export TcFree
func TcFree(p *C.char) { C.free(unsafe.Pointer(p)) }

//export TcStopServe
func TcStopServe() *C.char {
	mu.Lock()
	defer mu.Unlock()
	if server == nil {
		return cstr("")
	}
	serverConns.close()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := server.DrainTCP(ctx); err != nil && !errors.Is(err, context.DeadlineExceeded) {
		logf("drain stopped server: %v", err)
	}
	err := server.Close()
	if err != nil {
		return cerr(err)
	}
	server = nil
	serverConns = nil
	return cstr("")
}

func loadOrCreateKey(path string) (*tailcat.PrivateKey, bool, error) {
	b, err := os.ReadFile(path)
	if err == nil {
		var pk tailcat.PrivateKey
		if err := json.Unmarshal(b, &pk); err != nil {
			return nil, false, fmt.Errorf("parse %s: %w", path, err)
		}
		return &pk, false, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return nil, false, err
	}
	return tailcat.NewPrivateKey(), true, nil
}

func saveJSON(path string, v any) error {
	b, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o600); err != nil {
		return err
	}
	if err := os.Rename(tmp, path); err != nil {
		return err
	}
	return os.Chmod(path, 0o600)
}

// TcServe starts the controlled-side tunnel. Every TCP connection on any
// tunnel port is proxied to 127.0.0.1:targetPort (RustDesk's loopback-only
// listener). keyPath persists the identity and DERP region, so the address
// stays the same across restarts. allow is a comma-separated nodekey list;
// empty allows anyone holding the address. Returns the tailcat address.
//
//export TcServe
func TcServe(keyPath, allow *C.char, targetPort C.int, localToken *C.char) *C.char {
	mu.Lock()
	defer mu.Unlock()
	if server != nil {
		return cstr(string(server.TailcatAddr()))
	}
	token, err := decodeLocalToken(C.GoString(localToken))
	if err != nil {
		return cerr(err)
	}
	path := C.GoString(keyPath)
	pk, fresh, err := loadOrCreateKey(path)
	if err != nil {
		return cerr(err)
	}
	s := &tailcat.Server{
		Key:          pk.Private,
		PresharedKey: pk.Public.PresharedKey,
		RegionID:     pk.Public.RegionID,
		Logf:         logf,
	}
	if len(pk.Public.Region) > 0 {
		if pk.Public.Region[0] == nil {
			return cerr(errors.New("invalid saved DERP region"))
		}
		s.Region = pk.Public.Region[0]
	}
	if list := strings.TrimSpace(C.GoString(allow)); list != "" {
		allowed := map[key.NodePublic]bool{}
		for _, k := range strings.Split(list, ",") {
			var nk key.NodePublic
			if err := nk.UnmarshalText([]byte(strings.TrimSpace(k))); err != nil {
				return cerr(fmt.Errorf("bad allow key %q: %w", k, err))
			}
			allowed[nk] = true
		}
		s.AllowClient = func(k key.NodePublic) bool { return allowed[k] }
	}
	target := fmt.Sprintf("127.0.0.1:%d", int(targetPort))
	identities := &peerRegistry{}
	connections := &incomingConnections{}
	s.OnTCP = func(port uint16) func(net.Conn) {
		if port != 21118 {
			return nil
		}
		return func(c net.Conn) {
			peer, ok := s.PeerKey(c.RemoteAddr())
			if !ok {
				c.Close()
				return
			}
			address, err := identities.address(peer)
			if err != nil {
				c.Close()
				return
			}
			proxyToLocal(c, target, token, address, connections)
		}
	}
	if err := s.Start(); err != nil {
		connections.close()
		if closeErr := s.Close(); closeErr != nil {
			logf("cleanup failed server: %v", closeErr)
		}
		return cerr(err)
	}
	addr := s.TailcatAddr()
	if fresh || (pk.Public.RegionID == 0 && len(pk.Public.Region) == 0) {
		// Pin the region picked at first start so the address never changes.
		if ci, err := tailcat.ParseAddr(addr); err == nil {
			pk.Public = ci
		}
		if err := saveJSON(path, pk); err != nil {
			logf("save key: %v", err)
		}
	}
	server = s
	serverConns = connections
	return cstr(string(addr))
}

func clientFor(ctx context.Context, addr, keyPath string) (tunnelClient, func(), func(), error) {
	id := addr + "\x00" + keyPath
	return clients.acquire(ctx, id, func() (tunnelClient, error) {
		cl := tailcat.NewClient(tailcat.Addr(addr))
		cl.Logf = logf
		if keyPath != "" {
			k, err := clientKey(keyPath)
			if err != nil {
				return nil, err
			}
			cl.Key = k
		}
		return cl, nil
	})
}

type clientKeyFile struct {
	Private key.NodePrivate
	Public  key.NodePublic
}

func clientKey(path string) (key.NodePrivate, error) {
	var f clientKeyFile
	b, err := os.ReadFile(path)
	if err == nil {
		err = json.Unmarshal(b, &f)
		return f.Private, err
	}
	if !errors.Is(err, os.ErrNotExist) {
		return key.NodePrivate{}, err
	}
	f.Private = key.NewNode()
	f.Public = f.Private.Public()
	return f.Private, saveJSON(path, f)
}

// TcClientPubkey returns this machine's controller identity ("nodekey:..."),
// creating it at keyPath if needed. Give it to the controlled side's allow list.
//
//export TcClientPubkey
func TcClientPubkey(keyPath *C.char) *C.char {
	k, err := clientKey(C.GoString(keyPath))
	if err != nil {
		return cerr(err)
	}
	b, _ := k.Public().MarshalText()
	return cstr(string(b))
}

// TcForward opens a loopback listener whose connections are tunnelled to the
// peer's RustDesk. Returns "127.0.0.1:port". Repeated calls for the same
// address reuse the tunnel (and its direct path) but get a new listener.
//
//export TcForward
func TcForward(addr, keyPath *C.char, timeoutMs C.int, localToken *C.char) *C.char {
	token, err := decodeLocalToken(C.GoString(localToken))
	if err != nil {
		return cerr(err)
	}
	a := C.GoString(addr)
	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutMs)*time.Millisecond)
	defer cancel()
	cl, release, drain, err := clientFor(ctx, a, C.GoString(keyPath))
	if err != nil {
		return cerr(err)
	}
	// Dial once up front so connect errors (bad address, not allowed, peer
	// offline) surface here instead of as a silent EOF later.
	first, err := cl.DialTCPPort(ctx, 21118)
	if err != nil {
		release()
		return cerr(fmt.Errorf("tunnel to peer failed: %w", err))
	}
	local, err := newLocalForward(first, token, 5*time.Second, release, drain)
	if err != nil {
		return cerr(err)
	}
	return cstr(local)
}

//export TcCloseForward
func TcCloseForward(address *C.char) { closeForward(C.GoString(address)) }

// TcPath reports how traffic to the peer currently flows: "direct <ip:port>"
// or "derp". It sends one disco ping; call after TcForward.
//
//export TcPath
func TcPath(addr *C.char, timeoutMs C.int) *C.char {
	prefix := C.GoString(addr) + "\x00"
	cl, release, err := clients.acquireExisting(prefix)
	if err != nil {
		return cerr(err)
	}
	defer release()
	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeoutMs)*time.Millisecond)
	defer cancel()
	r, err := cl.DiscoPing(ctx)
	if err != nil {
		return cerr(err)
	}
	if r.Endpoint != "" {
		return cstr("direct " + r.Endpoint)
	}
	return cstr("derp")
}

// TcQR renders s as a QR code with Unicode half blocks, for terminals and logs.
//
//export TcQR
func TcQR(s *C.char) *C.char {
	code, err := qr.Encode(C.GoString(s), qr.L)
	if err != nil {
		return cerr(err)
	}
	const quiet = 2
	n := code.Size
	at := func(x, y int) bool { return x >= 0 && y >= 0 && x < n && y < n && code.Black(x, y) }
	var b strings.Builder
	for y := -quiet; y < n+quiet; y += 2 {
		for x := -quiet; x < n+quiet; x++ {
			top, bot := at(x, y), at(x, y+1)
			switch {
			case top && bot:
				b.WriteRune(' ')
			case top:
				b.WriteRune('▄')
			case bot:
				b.WriteRune('▀')
			default:
				b.WriteRune('█')
			}
		}
		b.WriteByte('\n')
	}
	return cstr(b.String())
}

// TcQRPNG writes the QR code of s to path as a PNG (for the GUI).
//
//export TcQRPNG
func TcQRPNG(s, path *C.char) *C.char {
	code, err := qr.Encode(C.GoString(s), qr.L)
	if err != nil {
		return cerr(err)
	}
	code.Scale = 6
	if err := os.WriteFile(C.GoString(path), code.PNG(), 0o644); err != nil {
		return cerr(err)
	}
	return cstr("")
}

func main() {}
