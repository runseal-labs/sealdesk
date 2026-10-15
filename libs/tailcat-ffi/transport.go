package main

import (
	"bytes"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"io"
	"net"
	"sync"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
)

const localTokenSize = 32

func decodeLocalToken(text string) ([]byte, error) {
	token, err := hex.DecodeString(text)
	if err != nil || len(token) != localTokenSize {
		return nil, errors.New("invalid local transport token")
	}
	return token, nil
}

type peerRegistry struct {
	once sync.Once
	salt [32]byte
	err  error
}

func (r *peerRegistry) address(peer key.NodePublic) ([16]byte, error) {
	r.once.Do(func() { _, r.err = rand.Read(r.salt[:]) })
	if r.err != nil {
		return [16]byte{}, r.err
	}
	digest := hmac.New(sha256.New, r.salt[:])
	digest.Write(peer.AppendTo(nil))
	var address [16]byte
	copy(address[:], digest.Sum(nil))
	address[0] = 0xfd
	return address, nil
}

type incomingConnections struct {
	mu     sync.Mutex
	closed bool
	items  map[net.Conn]struct{}
}

func (connections *incomingConnections) add(conn net.Conn) bool {
	connections.mu.Lock()
	defer connections.mu.Unlock()
	if connections.closed {
		conn.Close()
		return false
	}
	if connections.items == nil {
		connections.items = make(map[net.Conn]struct{})
	}
	connections.items[conn] = struct{}{}
	return true
}

func (connections *incomingConnections) remove(conn net.Conn) {
	connections.mu.Lock()
	delete(connections.items, conn)
	connections.mu.Unlock()
}

func (connections *incomingConnections) close() {
	connections.mu.Lock()
	connections.closed = true
	items := connections.items
	connections.items = nil
	connections.mu.Unlock()
	for conn := range items {
		conn.Close()
	}
}

func proxyToLocal(peer net.Conn, target string, token []byte, address [16]byte, connections *incomingConnections) {
	if !connections.add(peer) {
		return
	}
	defer connections.remove(peer)
	defer peer.Close()
	local, err := net.DialTimeout("tcp", target, 5*time.Second)
	if err != nil {
		return
	}
	if !connections.add(local) {
		return
	}
	defer connections.remove(local)
	defer local.Close()
	if err := local.SetWriteDeadline(time.Now().Add(5 * time.Second)); err != nil {
		return
	}
	header := append(append([]byte{}, token...), address[:]...)
	if _, err := io.Copy(local, bytes.NewReader(header)); err != nil {
		return
	}
	if err := local.SetWriteDeadline(time.Time{}); err != nil {
		return
	}
	tailcat.ProxyConns(peer, local)
}

type localForward struct {
	listener *net.TCPListener
	upstream net.Conn
	release  func()
	once     sync.Once
	finished chan struct{}
}

var forwards = struct {
	sync.Mutex
	items map[string]*localForward
}{items: make(map[string]*localForward)}

func (f *localForward) close() {
	f.once.Do(func() {
		f.listener.Close()
		f.upstream.Close()
		if f.release != nil {
			f.release()
		}
	})
}

func closeForward(address string) {
	forwards.Lock()
	forward := forwards.items[address]
	forwards.Unlock()
	if forward != nil {
		forward.close()
	}
}

func removeForward(address string, forward *localForward) {
	forwards.Lock()
	if forwards.items[address] == forward {
		delete(forwards.items, address)
	}
	forwards.Unlock()
}

type drainingLocalConn struct {
	*net.TCPConn
	drain func()
}

func (conn *drainingLocalConn) Close() error {
	conn.drain()
	return conn.TCPConn.Close()
}

func newLocalForward(upstream net.Conn, token []byte, wait time.Duration, release func(), drain func()) (string, error) {
	listener, err := net.ListenTCP("tcp", &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		upstream.Close()
		if release != nil {
			release()
		}
		return "", err
	}
	forward := &localForward{listener: listener, upstream: upstream, release: release, finished: make(chan struct{})}
	address := listener.Addr().String()
	forwards.Lock()
	forwards.items[address] = forward
	forwards.Unlock()
	go func() {
		defer close(forward.finished)
		defer forward.close()
		defer removeForward(address, forward)
		deadline := time.Now().Add(wait)
		if err := listener.SetDeadline(deadline); err != nil {
			return
		}
		for {
			local, err := listener.AcceptTCP()
			if err != nil {
				return
			}
			if err := local.SetReadDeadline(minTime(deadline, time.Now().Add(time.Second))); err != nil {
				local.Close()
				continue
			}
			var received [localTokenSize]byte
			_, err = io.ReadFull(local, received[:])
			if err != nil || subtle.ConstantTimeCompare(received[:], token) != 1 {
				local.Close()
				continue
			}
			listener.Close()
			removeForward(address, forward)
			if err := local.SetDeadline(time.Now().Add(time.Second)); err != nil {
				local.Close()
				return
			}
			if _, err := local.Write([]byte{1}); err != nil {
				local.Close()
				return
			}
			if err := local.SetDeadline(time.Time{}); err != nil {
				local.Close()
				return
			}
			if drain == nil {
				tailcat.ProxyConns(local, upstream)
			} else {
				tailcat.ProxyConns(&drainingLocalConn{TCPConn: local, drain: drain}, upstream)
			}
			return
		}
	}()
	return address, nil
}

func minTime(left, right time.Time) time.Time {
	if left.Before(right) {
		return left
	}
	return right
}
