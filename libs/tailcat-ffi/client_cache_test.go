package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"tailscale.com/ipn/ipnstate"
)

type fakeTunnelClient struct {
	closed     atomic.Int32
	closing    chan struct{}
	allowClose <-chan struct{}
}

func (client *fakeTunnelClient) DialTCPPort(context.Context, uint16) (net.Conn, error) {
	return nil, errors.New("offline")
}
func (client *fakeTunnelClient) DiscoPing(context.Context) (*ipnstate.PingResult, error) {
	return nil, errors.New("offline")
}
func (client *fakeTunnelClient) Close() error {
	client.closed.Add(1)
	if client.closing != nil {
		close(client.closing)
	}
	if client.allowClose != nil {
		<-client.allowClose
	}
	return nil
}

func TestClientReleasedOnlyAfterLastSessionAndProbe(t *testing.T) {
	cache := &clientCache{}
	client := &fakeTunnelClient{}
	create := func() (tunnelClient, error) { return client, nil }
	_, firstRelease, _, err := cache.acquire(context.Background(), "host\x00key", create)
	if err != nil {
		t.Fatal(err)
	}
	_, secondRelease, _, err := cache.acquire(context.Background(), "host\x00key", func() (tunnelClient, error) {
		t.Fatal("active tunnel was not reused")
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	_, probeRelease, err := cache.acquireExisting("host\x00")
	if err != nil {
		t.Fatal(err)
	}
	firstRelease()
	firstRelease()
	secondRelease()
	if client.closed.Load() != 0 {
		t.Fatal("active path probe lost its tunnel")
	}
	probeRelease()
	probeRelease()
	if client.closed.Load() != 1 || len(cache.entries) != 0 {
		t.Fatal("idle tunnel resources retained")
	}
	if _, _, err := cache.acquireExisting("host\x00"); err == nil {
		t.Fatal("idle tunnel remained cached")
	}
}

func TestClientDestinationChurnDoesNotRetainResources(t *testing.T) {
	cache := &clientCache{}
	for destination := 0; destination < 10_000; destination++ {
		client := &fakeTunnelClient{}
		_, release, _, err := cache.acquire(context.Background(), fmt.Sprint(destination), func() (tunnelClient, error) { return client, nil })
		if err != nil {
			t.Fatal(err)
		}
		release()
		if client.closed.Load() != 1 || len(cache.entries) != 0 {
			t.Fatal("destination churn leaked a client")
		}
	}
}

func TestClientFailureAndActiveLimitRecover(t *testing.T) {
	cache := &clientCache{}
	if _, _, _, err := cache.acquire(context.Background(), "failed", func() (tunnelClient, error) { return nil, errors.New("bad key") }); err == nil {
		t.Fatal("creation failure not returned")
	}
	releases := []func(){}
	create := func() (tunnelClient, error) { return &fakeTunnelClient{}, nil }
	for destination := 0; destination < 64; destination++ {
		_, release, _, err := cache.acquire(context.Background(), fmt.Sprint(destination), create)
		if err != nil {
			t.Fatal(err)
		}
		releases = append(releases, release)
	}
	if _, _, _, err := cache.acquire(context.Background(), "overflow", create); err == nil {
		t.Fatal("active resource cap missing")
	}
	releases[0]()
	_, release, _, err := cache.acquire(context.Background(), "overflow", create)
	if err != nil {
		t.Fatal("active resource cap became permanent", err)
	}
	release()
	for _, release := range releases {
		release()
	}
	if len(cache.entries) != 0 {
		t.Fatal("closed clients remained in cache")
	}
}

func TestConcurrentClientLeasesReuseOneTunnelAndCloseOnce(t *testing.T) {
	cache := &clientCache{}
	client := &fakeTunnelClient{}
	var creations atomic.Int32
	var ready sync.WaitGroup
	var finished sync.WaitGroup
	ready.Add(64)
	finished.Add(64)
	for worker := 0; worker < 64; worker++ {
		go func() {
			defer finished.Done()
			_, release, _, err := cache.acquire(context.Background(), "host\x00key", func() (tunnelClient, error) { creations.Add(1); return client, nil })
			ready.Done()
			if err != nil {
				t.Error(err)
				return
			}
			ready.Wait()
			release()
		}()
	}
	finished.Wait()
	if creations.Load() != 1 || client.closed.Load() != 1 || len(cache.entries) != 0 {
		t.Fatal("concurrent leases lost reference ownership")
	}
}

func TestDrainingClientWaitIsCancellableAndPreservesOtherSessions(t *testing.T) {
	cache := &clientCache{}
	client := &fakeTunnelClient{}
	create := func() (tunnelClient, error) { return client, nil }
	_, firstRelease, firstDrain, err := cache.acquire(context.Background(), "host", create)
	if err != nil {
		t.Fatal(err)
	}
	_, secondRelease, secondDrain, err := cache.acquire(context.Background(), "host", create)
	if err != nil {
		t.Fatal(err)
	}
	firstDrain()
	reused, release, _, err := cache.acquire(context.Background(), "host", func() (tunnelClient, error) {
		t.Fatal("one draining session replaced another live session")
		return nil, nil
	})
	if err != nil || reused != client {
		t.Fatal("active session lost reuse", err)
	}
	release()
	secondDrain()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, _, _, err := cache.acquire(ctx, "host", create); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("draining wait ignored cancellation", err)
	}
	if client.closed.Load() != 0 {
		t.Fatal("draining client closed before its leases ended")
	}
	firstRelease()
	secondRelease()
	firstDrain()
	secondDrain()
	if client.closed.Load() != 1 {
		t.Fatal("draining client did not close exactly once")
	}
}

func TestReplacementClientStartsAfterOldClientFinishesClosing(t *testing.T) {
	cache := &clientCache{}
	allowClose := make(chan struct{})
	client := &fakeTunnelClient{closing: make(chan struct{}), allowClose: allowClose}
	_, release, drain, err := cache.acquire(context.Background(), "host", func() (tunnelClient, error) { return client, nil })
	if err != nil {
		t.Fatal(err)
	}
	drain()
	finished := make(chan struct{})
	go func() { release(); close(finished) }()
	<-client.closing
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, _, _, err := cache.acquire(ctx, "host", func() (tunnelClient, error) {
		t.Fatal("replacement overlapped the old network engine")
		return nil, nil
	}); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	close(allowClose)
	<-finished
	replacement := &fakeTunnelClient{}
	got, release, _, err := cache.acquire(context.Background(), "host", func() (tunnelClient, error) { return replacement, nil })
	if err != nil || got != replacement {
		t.Fatal("replacement did not recover", err)
	}
	release()
}

func TestCancelledForwardReleasesUnderlyingClient(t *testing.T) {
	cache := &clientCache{}
	client := &fakeTunnelClient{}
	_, release, _, err := cache.acquire(context.Background(), "host", func() (tunnelClient, error) { return client, nil })
	if err != nil {
		t.Fatal(err)
	}
	upstream, peer := net.Pipe()
	defer peer.Close()
	address, err := newLocalForward(upstream, make([]byte, localTokenSize), time.Second, release, nil)
	if err != nil {
		t.Fatal(err)
	}
	forward := forwardState(t, address)
	closeForward(address)
	waitClosed(t, forward)
	if client.closed.Load() != 1 || len(cache.entries) != 0 {
		t.Fatal("cancelled forward leaked its client")
	}
}
