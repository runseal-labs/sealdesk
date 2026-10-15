package main

import (
	"bytes"
	"encoding/hex"
	"io"
	"net"
	"sync"
	"testing"
	"time"

	"tailscale.com/types/key"
)

func TestPeerIdentityIsStableAndDistinct(t *testing.T) {
	registry := &peerRegistry{}
	attacker, controller := key.NewNode().Public(), key.NewNode().Public()
	first, err := registry.address(attacker)
	if err != nil {
		t.Fatal(err)
	}
	for attempt := 0; attempt < 100; attempt++ {
		again, err := registry.address(attacker)
		if err != nil || again != first {
			t.Fatal("reconnection changed the attacker's quota identity")
		}
	}
	second, err := registry.address(controller)
	if err != nil || second == first || second[0] != 0xfd {
		t.Fatal("controllers must have distinct private IPv6 quota buckets")
	}
	for attempt := 0; attempt < 10_000; attempt++ {
		if _, err := registry.address(key.NewNode().Public()); err != nil {
			t.Fatal("identity churn exhausted stateless quota mapping", err)
		}
	}
	if same, err := registry.address(attacker); err != nil || same != first {
		t.Fatal("key churn changed an existing peer identity")
	}
}

func TestIncomingConnectionsStopClosesCurrentAndLateSockets(t *testing.T) {
	connections := &incomingConnections{}
	var ready, finished sync.WaitGroup
	ready.Add(64)
	finished.Add(64)
	for worker := 0; worker < 64; worker++ {
		go func() {
			defer finished.Done()
			server, client := net.Pipe()
			defer server.Close()
			defer client.Close()
			if !connections.add(server) {
				t.Error("connection refused before shutdown")
			}
			ready.Done()
			client.SetReadDeadline(time.Now().Add(2 * time.Second))
			var message [1]byte
			if _, err := client.Read(message[:]); err != io.EOF {
				t.Errorf("stopped socket did not reach EOF: %v", err)
			}
			connections.remove(server)
		}()
	}
	ready.Wait()
	connections.close()
	finished.Wait()
	connections.close()
	server, client := net.Pipe()
	defer client.Close()
	if connections.add(server) {
		t.Fatal("late connection accepted after shutdown")
	}
	client.SetReadDeadline(time.Now().Add(time.Second))
	var message [1]byte
	if _, err := client.Read(message[:]); err != io.EOF {
		t.Fatalf("late socket did not close: %v", err)
	}
}

func TestLocalTokenValidation(t *testing.T) {
	for _, text := range []string{"", "secret", hex.EncodeToString(make([]byte, 31)), hex.EncodeToString(make([]byte, 33))} {
		if _, err := decodeLocalToken(text); err == nil {
			t.Fatal("accepted malformed token")
		}
	}
	if token, err := decodeLocalToken(hex.EncodeToString(make([]byte, 32))); err != nil || len(token) != 32 {
		t.Fatal("refused correctly sized token")
	}
}

func TestPeerIdentityIsProcessSaltedAndSafeUnderConcurrentChurn(t *testing.T) {
	registry := &peerRegistry{}
	peer := key.NewNode().Public()
	var workers sync.WaitGroup
	addresses := make(chan [16]byte, 64)
	for worker := 0; worker < 64; worker++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			address, err := registry.address(peer)
			if err != nil {
				t.Error(err)
				return
			}
			addresses <- address
		}()
	}
	workers.Wait()
	close(addresses)
	expected, err := registry.address(peer)
	if err != nil {
		t.Fatal(err)
	}
	for address := range addresses {
		if address != expected {
			t.Fatal("concurrent initialization changed peer identity")
		}
	}
	other, err := (&peerRegistry{}).address(peer)
	if err != nil || other == expected {
		t.Fatal("server salt did not isolate the identity mapping")
	}
}

func TestForwardPreservesHalfCloseAndReleasesClientAfterResponse(t *testing.T) {
	listener, err := net.ListenTCP("tcp", &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	remote, err := net.DialTCP("tcp", nil, listener.Addr().(*net.TCPAddr))
	if err != nil {
		t.Fatal(err)
	}
	defer remote.Close()
	upstream, err := listener.AcceptTCP()
	if err != nil {
		t.Fatal(err)
	}
	token := bytes.Repeat([]byte{9}, localTokenSize)
	released := make(chan struct{})
	draining := make(chan struct{})
	address, err := newLocalForward(upstream, token, time.Second, func() { close(released) }, func() { close(draining) })
	if err != nil {
		t.Fatal(err)
	}
	forward := forwardState(t, address)
	localAddress, err := net.ResolveTCPAddr("tcp", address)
	if err != nil {
		t.Fatal(err)
	}
	local, err := net.DialTCP("tcp", nil, localAddress)
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close()
	local.SetDeadline(time.Now().Add(time.Second))
	remote.SetDeadline(time.Now().Add(time.Second))
	local.Write(token)
	var ack [1]byte
	if _, err := io.ReadFull(local, ack[:]); err != nil {
		t.Fatal(err)
	}
	local.Write([]byte("request"))
	if err := local.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	request, err := io.ReadAll(remote)
	if err != nil || string(request) != "request" {
		t.Fatal("request half-close was lost", err)
	}
	select {
	case <-released:
		t.Fatal("client released before its response")
	default:
	}
	select {
	case <-draining:
		t.Fatal("request half-close retired a live response")
	default:
	}
	remote.Write([]byte("response"))
	remote.CloseWrite()
	response, err := io.ReadAll(local)
	if err != nil || string(response) != "response" {
		t.Fatal("response truncated by half-close", err)
	}
	waitClosed(t, forward)
	select {
	case <-draining:
	default:
		t.Fatal("completed data transfer did not start draining")
	}
	select {
	case <-released:
	default:
		t.Fatal("finished response did not release the client")
	}
}

func forwardState(t *testing.T, address string) *localForward {
	t.Helper()
	forwards.Lock()
	defer forwards.Unlock()
	forward := forwards.items[address]
	if forward == nil {
		t.Fatal("forward not registered")
	}
	return forward
}

func waitClosed(t *testing.T, forward *localForward) {
	t.Helper()
	select {
	case <-forward.finished:
	case <-time.After(2 * time.Second):
		t.Fatal("forward did not release its resources")
	}
}

func TestForwardRequiresTokenAndClosesListenerBeforePayload(t *testing.T) {
	upstream, peer := net.Pipe()
	defer peer.Close()
	token := bytes.Repeat([]byte{42}, localTokenSize)
	address, err := newLocalForward(upstream, token, time.Second, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	forward := forwardState(t, address)
	t.Cleanup(forward.close)
	untrusted, err := net.DialTimeout("tcp", address, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	untrusted.SetDeadline(time.Now().Add(time.Second))
	untrusted.Write(make([]byte, localTokenSize))
	var acknowledgement [1]byte
	if _, err := untrusted.Read(acknowledgement[:]); err == nil {
		t.Fatal("another local user borrowed the tunnel without its capability")
	}
	untrusted.Close()
	local, err := net.DialTimeout("tcp", address, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close()
	local.SetDeadline(time.Now().Add(time.Second))
	if _, err := local.Write(token); err != nil {
		t.Fatal(err)
	}
	if _, err := io.ReadFull(local, acknowledgement[:]); err != nil || acknowledgement != [1]byte{1} {
		t.Fatal("authorized client did not receive acknowledgement", err)
	}
	if borrowed, err := net.DialTimeout("tcp", address, 100*time.Millisecond); err == nil {
		borrowed.Close()
		t.Fatal("single-use listener remained open after authentication")
	}
	closeForward(address)
	go func() {
		peer.Write([]byte("frame"))
	}()
	var payload [5]byte
	if _, err := io.ReadFull(local, payload[:]); err != nil || string(payload[:]) != "frame" {
		t.Fatal("transport guard closed an already handed-off session", err)
	}
	local.Close()
	peer.Close()
	waitClosed(t, forward)
}

func TestAbandonedAndCancelledForwardsCloseBothSides(t *testing.T) {
	for _, cancel := range []bool{false, true} {
		t.Run(map[bool]string{false: "unused", true: "cancelled"}[cancel], func(t *testing.T) {
			upstream, peer := net.Pipe()
			defer peer.Close()
			address, err := newLocalForward(upstream, make([]byte, localTokenSize), 30*time.Millisecond, nil, nil)
			if err != nil {
				t.Fatal(err)
			}
			forward := forwardState(t, address)
			if cancel {
				closeForward(address)
			}
			waitClosed(t, forward)
			peer.SetReadDeadline(time.Now().Add(time.Second))
			var payload [1]byte
			if _, err := peer.Read(payload[:]); err != io.EOF {
				t.Fatal("unused upstream remained open", err)
			}
			if local, err := net.DialTimeout("tcp", address, 100*time.Millisecond); err == nil {
				local.Close()
				t.Fatal("abandoned listener remained open")
			}
			forwards.Lock()
			_, retained := forwards.items[address]
			forwards.Unlock()
			if retained {
				t.Fatal("cancelled forward retained in registry")
			}
		})
	}
}

func TestProxyMetadataPrecedesUnmodifiedRustDeskPayload(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	token := bytes.Repeat([]byte{7}, localTokenSize)
	peer, remote := net.Pipe()
	defer remote.Close()
	done := make(chan struct{})
	go func() {
		proxyToLocal(peer, listener.Addr().String(), token, [16]byte{0xfd, 2}, &incomingConnections{})
		close(done)
	}()
	local, err := listener.Accept()
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close()
	local.SetDeadline(time.Now().Add(time.Second))
	header := make([]byte, localTokenSize+16)
	if _, err := io.ReadFull(local, header); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(header[:localTokenSize], token) || !bytes.Equal(header[localTokenSize:], []byte{0xfd, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}) {
		t.Fatal("authenticated peer metadata lost")
	}
	go remote.Write([]byte("login"))
	var login [5]byte
	if _, err := io.ReadFull(local, login[:]); err != nil || string(login[:]) != "login" {
		t.Fatal("metadata corrupted RustDesk framing", err)
	}
	local.Close()
	remote.Close()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("proxy did not close")
	}
}
