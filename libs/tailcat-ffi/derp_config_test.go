package main

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/derp/derpserver"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"
	"tailscale.com/types/logger"
)

func testDERPRegion(t *testing.T) *tailcfg.DERPRegion {
	t.Helper()
	relay := derpserver.New(key.NewNode(), logger.Discard)
	https := httptest.NewTLSServer(derpserver.Handler(relay))
	t.Cleanup(https.Close)
	t.Cleanup(func() { relay.Close() })
	_, port, err := net.SplitHostPort(https.Listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	n, err := strconv.Atoi(port)
	if err != nil {
		t.Fatal(err)
	}
	cert := sha256.Sum256(https.Certificate().Raw)
	return &tailcfg.DERPRegion{RegionID: 1, RegionCode: "test", Nodes: []*tailcfg.DERPNode{{
		Name: "test", RegionID: 1, HostName: "127.0.0.1", IPv4: "127.0.0.1", IPv6: "none",
		DERPPort: n, STUNPort: -1, CertName: fmt.Sprintf("sha256-raw:%x", cert),
	}}}
}

func trustMapServer(t *testing.T, server *httptest.Server) {
	t.Helper()
	previous := http.DefaultTransport
	http.DefaultTransport = server.Client().Transport
	t.Cleanup(func() { server.Client().CloseIdleConnections(); http.DefaultTransport = previous })
}

func TestConfiguredDERPHostAndClient(t *testing.T) {
	region := testDERPRegion(t)
	var hostFetches, clientFetches atomic.Int32
	maps := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.Header.Get("Tailcat-Mode") {
		case "server":
			hostFetches.Add(1)
		case "client":
			clientFetches.Add(1)
		default:
			t.Error("missing role in DERP map request")
		}
		json.NewEncoder(w).Encode(&tailcfg.DERPMap{Regions: map[tailcfg.DERPRegionID]*tailcfg.DERPRegion{1: region}})
	}))
	defer maps.Close()
	trustMapServer(t, maps)
	ctx, cancel := context.WithTimeout(context.Background(), startupTimeout)
	defer cancel()
	identity := tailcat.NewPrivateKey()
	host := &tailcat.Server{Key: identity.Private, PresharedKey: identity.Public.PresharedKey, RegionID: 1, Logf: logger.Discard}
	if err := prepareServer(ctx, host, maps.URL+"/host"); err != nil {
		t.Fatal(err)
	}
	ci := identity.Public
	ci.RegionID = 1
	addr := string(ci.Addr())
	first, releaseFirst, _, err := clientFor(ctx, addr, "", maps.URL+"/client")
	if err != nil {
		t.Fatal(err)
	}
	defer releaseFirst()
	if err := first.(*configuredClient).prepare(ctx); err != nil {
		t.Fatal(err)
	}
	reused, releaseReused, _, err := clientFor(ctx, addr, "", maps.URL+"/client")
	if err != nil {
		t.Fatal(err)
	}
	defer releaseReused()
	if reused != first {
		t.Fatal("same-source active tunnel was not reused")
	}
	other, releaseOther, _, err := clientFor(ctx, addr, "", maps.URL+"/other-client")
	if err != nil {
		t.Fatal(err)
	}
	defer releaseOther()
	if other == first {
		t.Fatal("changed DERP source reused the old tunnel")
	}
	if err := other.(*configuredClient).prepare(ctx); err != nil {
		t.Fatal(err)
	}
	if hostFetches.Load() != 1 || clientFetches.Load() != 2 {
		t.Fatalf("map requests: host=%d client=%d", hostFetches.Load(), clientFetches.Load())
	}
	t.Run("tunnel echo", func(t *testing.T) {
		host.OnTCP = func(uint16) func(net.Conn) {
			return func(conn net.Conn) { defer conn.Close(); io.Copy(conn, conn) }
		}
		if err := host.StartContext(ctx); err != nil {
			if strings.Contains(err.Error(), "netmon.New") && (errors.Is(err, syscall.EPERM) || errors.Is(err, syscall.EACCES)) {
				t.Skipf("system route inspection unavailable in this sandbox: %v", err)
			}
			t.Fatal(err)
		}
		defer host.Close()
		for _, client := range []tunnelClient{first, other} {
			conn, err := client.DialTCPPort(ctx, 21118)
			if err != nil {
				t.Fatal(err)
			}
			defer conn.Close()
			conn.SetDeadline(time.Now().Add(2 * time.Second))
			if _, err := conn.Write([]byte("sealdesk")); err != nil {
				t.Fatal(err)
			}
			var reply [8]byte
			if _, err := io.ReadFull(conn, reply[:]); err != nil || string(reply[:]) != "sealdesk" {
				t.Fatal("configured relay did not carry application data", err)
			}
		}
	})
}

func TestDERPStartupFailureStages(t *testing.T) {
	region := testDERPRegion(t)
	refused, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	refusedAddress := refused.Addr().String()
	refused.Close()
	_, refusedPort, _ := net.SplitHostPort(refusedAddress)
	port, _ := strconv.Atoi(refusedPort)
	stall := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { <-r.Context().Done() }))
	defer stall.Close()
	_, stallPort, _ := net.SplitHostPort(stall.Listener.Addr().String())
	stallNumber, _ := strconv.Atoi(stallPort)
	badTLS := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(http.StatusOK) }))
	badTLS.Config.ErrorLog = log.New(io.Discard, "", 0)
	defer badTLS.Close()
	maps := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/http":
			w.WriteHeader(http.StatusServiceUnavailable)
			io.WriteString(w, "SECRET response body")
		case "/timeout":
			<-r.Context().Done()
		case "/invalid":
			io.WriteString(w, `{"Regions":{"1":null}}`)
		case "/empty":
			io.WriteString(w, `{"Regions":{}}`)
		case "/insecure":
			copyRegion := *region
			copyNode := *region.Nodes[0]
			copyNode.InsecureForTests = true
			copyRegion.Nodes = []*tailcfg.DERPNode{&copyNode}
			json.NewEncoder(w).Encode(&tailcfg.DERPMap{Regions: map[tailcfg.DERPRegionID]*tailcfg.DERPRegion{1: &copyRegion}})
		case "/refused-relay", "/timeout-relay", "/tls-relay":
			copyRegion := *region
			copyNode := *region.Nodes[0]
			copyNode.DERPPort = port
			if r.URL.Path == "/timeout-relay" {
				copyNode.DERPPort = stallNumber
			}
			if r.URL.Path == "/tls-relay" {
				copyNode.DERPPort = region.Nodes[0].DERPPort
				copyNode.CertName = "wrong.example.invalid"
			}
			copyRegion.Nodes = []*tailcfg.DERPNode{&copyNode}
			json.NewEncoder(w).Encode(&tailcfg.DERPMap{Regions: map[tailcfg.DERPRegionID]*tailcfg.DERPRegion{1: &copyRegion}})
		case "/downgrade":
			http.Redirect(w, r, "http://"+refusedAddress+"/SECRET", http.StatusFound)
		default:
			io.WriteString(w, "not JSON SECRET")
		}
	}))
	defer maps.Close()
	trustMapServer(t, maps)
	ci := tailcat.NewPrivateKey().Public
	ci.RegionID = 1
	for _, test := range []struct {
		name, source, want string
		timeout            time.Duration
	}{
		{"http", maps.URL + "/http?SECRET", "DERP map fetch: HTTP 503", time.Second},
		{"timeout", maps.URL + "/timeout?SECRET", "DERP map fetch: timeout", 50 * time.Millisecond},
		{"refused map", "https://" + refusedAddress + "/?SECRET", "DERP map fetch: connection refused", time.Second},
		{"bad JSON", maps.URL + "/bad?SECRET", "DERP map fetch: invalid DERP map", time.Second},
		{"null region", maps.URL + "/invalid?SECRET", "DERP map fetch: invalid DERP map", time.Second},
		{"empty map", maps.URL + "/empty?SECRET", "DERP map fetch: invalid DERP map", time.Second},
		{"unsafe TLS", maps.URL + "/insecure?SECRET", "DERP map fetch: invalid DERP map", time.Second},
		{"refused relay", maps.URL + "/refused-relay?SECRET", "DERP relay/bootstrap: connection refused", time.Second},
		{"timeout relay", maps.URL + "/timeout-relay?SECRET", "DERP relay/bootstrap: timeout", 50 * time.Millisecond},
		{"TLS relay", maps.URL + "/tls-relay?SECRET", "DERP relay/bootstrap: TLS certificate validation failed", time.Second},
		{"TLS map", strings.Replace(badTLS.URL, "127.0.0.1", "localhost", 1) + "/?SECRET", "DERP map fetch: TLS certificate validation failed", time.Second},
		{"redirect downgrade", maps.URL + "/downgrade?SECRET", "DERP map fetch: connection failed", time.Second},
	} {
		t.Run(test.name, func(t *testing.T) {
			for _, mode := range []string{"host", "client"} {
				ctx, cancel := context.WithTimeout(context.Background(), test.timeout)
				var err error
				if mode == "host" {
					err = prepareServer(ctx, &tailcat.Server{RegionID: 1}, test.source)
				} else {
					client := &configuredClient{Client: &tailcat.Client{Server: ci.Addr(), DERPMapURL: test.source, Logf: logger.Discard}}
					_, err = client.DialTCPPort(ctx, 21118)
					client.Close()
				}
				cancel()
				if err == nil || err.Error() != test.want || strings.Contains(err.Error(), "SECRET") {
					t.Fatalf("%s error = %v, want %q", mode, err, test.want)
				}
			}
		})
	}
}

func TestDERPConfigurationAndKeyCompatibility(t *testing.T) {
	if source, err := derpMapSource(""); err != nil || source != tailcat.DefaultDERPMapURL {
		t.Fatal("empty source changed the upstream default", source, err)
	}
	for _, source := range []string{"http://relay.example/map", "https://user:SECRET@relay.example/map", "https://relay.example/map#SECRET", "https://relay.example:99999/map"} {
		if _, err := derpMapSource(source); err == nil || strings.Contains(err.Error(), "SECRET") {
			t.Fatal("unsafe map source was accepted or exposed", err)
		}
	}
	path := t.TempDir() + "/host.json"
	original := tailcat.NewPrivateKey()
	if err := saveJSON(path, original); err != nil {
		t.Fatal(err)
	}
	loaded, fresh, err := loadOrCreateKey(path)
	if err != nil || fresh || !loaded.Private.Equal(original.Private) || loaded.Public.PresharedKey != original.Public.PresharedKey {
		t.Fatal("existing identity did not survive configuration migration", err)
	}
	loaded.DERPMapURL = "https://relay.example/map"
	if err := saveJSON(path, loaded); err != nil {
		t.Fatal(err)
	}
	reloaded, _, err := loadOrCreateKey(path)
	if err != nil || reloaded.DERPMapURL != loaded.DERPMapURL || !reloaded.Private.Equal(original.Private) {
		t.Fatal("configured source or identity did not survive restart", err)
	}
}
