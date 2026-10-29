// Package main is built with -buildmode=c-archive and linked into RustDesk.
// It replaces the rendezvous / relay servers with a tailcat tunnel. WireGuard
// uses direct UDP when available; DERP handles meeting and remains the encrypted
// data relay when a direct path cannot be established.
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
	"encoding/hex"
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
	"tailscale.com/types/logger"
)

var (
	mu          sync.Mutex
	server      *tailcat.Server
	serverConns *incomingConnections
	clients     clientCache
)

func cstr(s string) *C.char { return C.CString(s) }

func goStringFree(p *C.char) string {
	s := C.GoString(p)
	C.free(unsafe.Pointer(p))
	return s
}

func cerr(err error) *C.char { return cstr("ERR:" + err.Error()) }

func logf(format string, args ...any) { log.Printf("tailcat: "+format, args...) }

//export TcFree
func TcFree(p *C.char) { C.free(unsafe.Pointer(p)) }

//export TcStopServe
func TcStopServe() *C.char {
	mu.Lock()
	defer mu.Unlock()
	stopSelfProbeLocked()
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
	serveStartedAt = time.Time{}
	return cstr("")
}

type serverKeyFile struct {
	tailcat.PrivateKey
	DERPMapURL string `json:",omitempty"`
}

// TcServeStatus reports whether the controlled-side tunnel is running and
// whether a self-probe meow over its home DERP has succeeded recently. The
// result is a JSON object; free it with TcFree.
//
//export TcServeStatus
func TcServeStatus() *C.char { return cstr(marshalServeStatus()) }

func loadOrCreateKey(path string) (*serverKeyFile, bool, error) {
	b, err := os.ReadFile(path)
	if err == nil {
		var pk serverKeyFile
		if err := json.Unmarshal(b, &pk); err != nil {
			return nil, false, fmt.Errorf("parse %s: %w", path, err)
		}
		return &pk, false, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return nil, false, err
	}
	return &serverKeyFile{PrivateKey: *tailcat.NewPrivateKey()}, true, nil
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

// persistFreshNodeKey writes a freshly created node key before relay bootstrap.
// Existing files are not rewritten. A failed write must not abort start: the
// relay can still come up on the in-memory key, matching the post-start save.
// Only the filesystem error is logged, never the key.
func persistFreshNodeKey(path string, pk *serverKeyFile, fresh bool) {
	if !fresh {
		return
	}
	if err := saveJSON(path, pk); err != nil {
		logf("save new node key before relay bootstrap failed: %v", err)
	}
}

func stageForServeErr(err error) string {
	msg := ""
	if err != nil {
		msg = err.Error()
	}
	switch {
	case strings.HasPrefix(msg, "DERP map configuration:"),
		strings.HasPrefix(msg, "host configuration:"),
		msg == "invalid local transport token":
		return initStageConfig
	case strings.HasPrefix(msg, "host identity:"), msg == "invalid saved DERP region":
		return initStageKey
	case strings.HasPrefix(msg, "host allow list:"):
		return initStageAllow
	case strings.HasPrefix(msg, "DERP map "), strings.HasPrefix(msg, "DERP relay selection:"):
		return initStageMap
	case strings.HasPrefix(msg, "DERP relay/"):
		return initStageRelay
	default:
		return initStageConfig
	}
}

func recordServeFailureLocked(err error, keyPath string) {
	setLastInitErrLocked(stageForServeErr(err), err, keyPath)
}

// TcServe starts the controlled-side tunnel. Every TCP connection on any
// tunnel port is proxied to 127.0.0.1:targetPort (RustDesk's loopback-only
// listener). keyPath persists the identity and DERP region, so the address
// stays the same across restarts. allow is a comma-separated nodekey list;
// empty allows anyone holding the address. Returns the tailcat address.
//
//export TcServe
func TcServe(keyPath, allow *C.char, targetPort C.int, localToken *C.char) *C.char {
	return TcServeWithDERPMap(keyPath, allow, targetPort, localToken, nil)
}

//export TcServeWithDERPMap
func TcServeWithDERPMap(keyPath, allow *C.char, targetPort C.int, localToken, derpMapURL *C.char) *C.char {
	source, err := derpMapSource(C.GoString(derpMapURL))
	if err != nil {
		mu.Lock()
		recordServeFailureLocked(err, "")
		mu.Unlock()
		return cerr(err)
	}
	mu.Lock()
	defer mu.Unlock()
	if server != nil {
		if server.DERPMapURL != source {
			err = errors.New("host configuration: stop host before changing DERP map")
			recordServeFailureLocked(err, "")
			return cerr(err)
		}
		return cstr(string(server.TailcatAddr()))
	}
	token, err := decodeLocalToken(C.GoString(localToken))
	if err != nil {
		recordServeFailureLocked(err, "")
		return cerr(err)
	}
	path := C.GoString(keyPath)
	pk, fresh, err := loadOrCreateKey(path)
	if err != nil {
		err = errors.New("host identity: could not read or parse saved key")
		recordServeFailureLocked(err, path)
		return cerr(err)
	}
	// Persist a new identity before relay bootstrap so LAN discovery can seal
	// a possession proof while the relay is still unreachable. An existing key
	// is left untouched; the region is written back only after start succeeds.
	persistFreshNodeKey(path, pk, fresh)
	previousSource, _ := derpMapSource(pk.DERPMapURL)
	sourceChanged := previousSource != source
	if sourceChanged {
		pk.Public.Region = nil
		pk.Public.RegionID = 0
	}
	s := &tailcat.Server{
		Key:          pk.Private,
		PresharedKey: pk.Public.PresharedKey,
		RegionID:     pk.Public.RegionID,
		Logf:         logger.Discard,
	}
	if len(pk.Public.Region) > 0 {
		if pk.Public.Region[0] == nil {
			err = errors.New("invalid saved DERP region")
			recordServeFailureLocked(err, path)
			return cerr(err)
		}
		s.Region = pk.Public.Region[0]
	}
	if list := strings.TrimSpace(C.GoString(allow)); list != "" {
		allowed := map[key.NodePublic]bool{}
		for _, k := range strings.Split(list, ",") {
			var nk key.NodePublic
			if err := nk.UnmarshalText([]byte(strings.TrimSpace(k))); err != nil {
				err = errors.New("host allow list: invalid node key")
				recordServeFailureLocked(err, path)
				return cerr(err)
			}
			allowed[nk] = true
		}
		s.AllowClient = func(k key.NodePublic) bool { return allowed[k] }
	}
	// Generated before Start so the copied AllowClient hook already admits it.
	// The server never acks a meow AllowClient rejects.
	probeKey := key.NewNode()
	s.AllowClient = allowWithProbe(s.AllowClient, probeKey.Public())
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
	ctx, cancel := context.WithTimeout(context.Background(), startupTimeout)
	defer cancel()
	if err := prepareServer(ctx, s, source); err != nil {
		recordServeFailureLocked(err, path)
		return cerr(err)
	}
	if err := s.StartContext(ctx); err != nil {
		wrapped := transportError(ctx, "DERP relay/bootstrap", err)
		recordServeFailureLocked(wrapped, path)
		connections.close()
		if closeErr := s.Close(); closeErr != nil {
			logf("cleanup failed server: %v", closeErr)
		}
		return cerr(wrapped)
	}
	clearLastInitErrLocked()
	serveStartedAt = time.Now()
	addr := s.TailcatAddr()
	if fresh || sourceChanged || (pk.Public.RegionID == 0 && len(pk.Public.Region) == 0) {
		// Pin the region picked at first start so the address never changes.
		if ci, err := tailcat.ParseAddr(addr); err == nil {
			pk.Public = ci
		}
		if source != tailcat.DefaultDERPMapURL {
			pk.DERPMapURL = source
		} else {
			pk.DERPMapURL = ""
		}
		if err := saveJSON(path, pk); err != nil {
			logf("save key: %v", err)
		}
	}
	server = s
	serverConns = connections
	startSelfProbeLocked(addr, probeKey, source)
	return cstr(string(addr))
}

func clientFor(ctx context.Context, addr, keyPath, source string) (tunnelClient, func(), func(), error) {
	id := addr + "\x00" + keyPath + "\x00" + source
	return clients.acquire(ctx, id, func() (tunnelClient, error) {
		cl := tailcat.NewClient(tailcat.Addr(addr))
		cl.Logf = logger.Discard
		cl.DERPMapURL = source
		if keyPath != "" {
			k, err := clientKey(keyPath)
			if err != nil {
				return nil, errors.New("controller identity: could not read or save key")
			}
			cl.Key = k
		}
		return &configuredClient{Client: cl}, nil
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
	return TcForwardWithDERPMap(addr, keyPath, timeoutMs, localToken, nil)
}

//export TcForwardWithDERPMap
func TcForwardWithDERPMap(addr, keyPath *C.char, timeoutMs C.int, localToken, derpMapURL *C.char) *C.char {
	source, err := derpMapSource(C.GoString(derpMapURL))
	if err != nil {
		return cerr(err)
	}
	token, err := decodeLocalToken(C.GoString(localToken))
	if err != nil {
		return cerr(err)
	}
	a := C.GoString(addr)
	ctx, cancel := context.WithTimeout(context.Background(), min(time.Duration(timeoutMs)*time.Millisecond, startupTimeout))
	defer cancel()
	cl, release, drain, err := clientFor(ctx, a, C.GoString(keyPath), source)
	if err != nil {
		return cerr(err)
	}
	// Dial once up front so connect errors (bad address, not allowed, peer
	// offline) surface here instead of as a silent EOF later.
	first, err := cl.DialTCPPort(ctx, 21118)
	if err != nil {
		release()
		return cerr(err)
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

// nodePublicHex is the controlled node's Curve25519 public key carried in a
// tailcat address ("p" / ServerPublic), as 64 hex chars with no prefix.
func nodePublicHex(addr string) (string, error) {
	ci, err := tailcat.ParseAddr(tailcat.Addr(addr))
	if err != nil {
		return "", err
	}
	raw := ci.ServerPublic.Raw32()
	return hex.EncodeToString(raw[:]), nil
}

// TcNodePublic returns that public key so a controller can check a LAN
// possession proof. It does not return a private key. Free the result with TcFree.
//
//export TcNodePublic
func TcNodePublic(addr *C.char) *C.char {
	got, err := nodePublicHex(C.GoString(addr))
	if err != nil {
		return cerr(err)
	}
	return cstr(got)
}

func main() {}
