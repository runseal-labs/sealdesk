package main

import (
	"context"
	"errors"
	"net"
	"strings"
	"sync"

	"tailscale.com/ipn/ipnstate"
)

type tunnelClient interface {
	DialTCPPort(context.Context, uint16) (net.Conn, error)
	DiscoPing(context.Context) (*ipnstate.PingResult, error)
	Close() error
}

type clientEntry struct {
	client     tunnelClient
	references int
	active     int
	closed     chan struct{}
}

type clientCache struct {
	mu      sync.Mutex
	entries map[string]*clientEntry
}

func (cache *clientCache) acquire(ctx context.Context, id string, create func() (tunnelClient, error)) (tunnelClient, func(), func(), error) {
	for {
		if err := ctx.Err(); err != nil {
			return nil, nil, nil, err
		}
		cache.mu.Lock()
		entry := cache.entries[id]
		if entry != nil && entry.active == 0 {
			closed := entry.closed
			cache.mu.Unlock()
			select {
			case <-ctx.Done():
				return nil, nil, nil, ctx.Err()
			case <-closed:
				continue
			}
		}
		if entry == nil {
			if len(cache.entries) >= 64 {
				cache.mu.Unlock()
				return nil, nil, nil, errors.New("too many active tailcat destinations")
			}
			client, err := create()
			if err != nil {
				cache.mu.Unlock()
				return nil, nil, nil, err
			}
			entry = &clientEntry{client: client, closed: make(chan struct{})}
			if cache.entries == nil {
				cache.entries = make(map[string]*clientEntry)
			}
			cache.entries[id] = entry
		}
		client, release, drain, err := cache.borrowLocked(id, entry, true)
		cache.mu.Unlock()
		return client, release, drain, err
	}
}

func (cache *clientCache) acquireExisting(prefix string) (tunnelClient, func(), error) {
	cache.mu.Lock()
	defer cache.mu.Unlock()
	for id, entry := range cache.entries {
		if strings.HasPrefix(id, prefix) && entry.active > 0 {
			client, release, _, err := cache.borrowLocked(id, entry, false)
			return client, release, err
		}
	}
	return nil, nil, errors.New("no active tunnel")
}

func (cache *clientCache) borrowLocked(id string, entry *clientEntry, active bool) (tunnelClient, func(), func(), error) {
	entry.references++
	if active {
		entry.active++
	}
	draining := !active
	released := false
	drain := func() {
		cache.mu.Lock()
		if !released && !draining {
			draining = true
			entry.active--
		}
		cache.mu.Unlock()
	}
	var once sync.Once
	release := func() {
		once.Do(func() {
			cache.mu.Lock()
			released = true
			if !draining {
				entry.active--
			}
			entry.references--
			closeClient := entry.references == 0
			cache.mu.Unlock()
			if closeClient {
				if err := entry.client.Close(); err != nil {
					logf("close idle client: %v", err)
				}
				cache.mu.Lock()
				delete(cache.entries, id)
				close(entry.closed)
				cache.mu.Unlock()
			}
		})
	}
	return entry.client, release, drain, nil
}
