package main

import (
	"context"
	"crypto/x509"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/derp"
	"tailscale.com/derp/derphttp"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/net/netmon"
	"tailscale.com/net/netns"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"
	"tailscale.com/types/logger"
)

const startupTimeout = 10 * time.Second

func derpMapSource(source string) (string, error) {
	source = strings.TrimSpace(source)
	if source == "" {
		return tailcat.DefaultDERPMapURL, nil
	}
	u, err := url.Parse(source)
	if err != nil || u.Scheme != "https" || u.Hostname() == "" || u.User != nil || u.Fragment != "" || u.Opaque != "" {
		return "", errors.New("DERP map configuration: use an HTTPS URL without credentials or fragment")
	}
	if port := u.Port(); port != "" {
		n, err := strconv.Atoi(port)
		if err != nil || n < 1 || n > 65535 {
			return "", errors.New("DERP map configuration: invalid HTTPS port")
		}
	}
	return u.String(), nil
}

// Upstream DERP errors sometimes discard their wrapped causes. Only return
// fixed classifications; URLs, response bodies, and pairing secrets stay private.
func transportError(ctx context.Context, stage string, err error) error {
	detail := "connection failed"
	var netErr net.Error
	var dnsErr *net.DNSError
	var unknownAuthority x509.UnknownAuthorityError
	var hostname x509.HostnameError
	var invalidCert x509.CertificateInvalidError
	message := strings.ToLower(err.Error())
	switch {
	case errors.Is(ctx.Err(), context.DeadlineExceeded), errors.Is(err, context.DeadlineExceeded), errors.As(err, &netErr) && netErr.Timeout(), strings.Contains(message, "deadline exceeded"), strings.Contains(message, "i/o timeout"):
		detail = "timeout"
	case errors.Is(ctx.Err(), context.Canceled), errors.Is(err, context.Canceled):
		detail = "cancelled"
	case errors.Is(err, syscall.ECONNREFUSED), strings.Contains(message, "connection refused"), strings.Contains(message, "actively refused"):
		detail = "connection refused"
	case errors.As(err, &dnsErr), strings.Contains(message, "no such host"):
		detail = "DNS lookup failed"
	case errors.As(err, &unknownAuthority), errors.As(err, &hostname), errors.As(err, &invalidCert), strings.Contains(message, "x509:"), strings.Contains(message, "certificate"):
		detail = "TLS certificate validation failed"
	}
	return fmt.Errorf("%s: %s", stage, detail)
}

func validDERPRegion(region *tailcfg.DERPRegion) bool {
	if region == nil || !region.RegionID.IsValid() || len(region.Nodes) == 0 {
		return false
	}
	hasRelay := false
	for _, node := range region.Nodes {
		if node == nil || node.RegionID != region.RegionID || node.HostName == "" || strings.ContainsAny(node.HostName, "/?#@ \t\r\n") || node.InsecureForTests || node.DERPPort < 0 || node.DERPPort > 65535 {
			return false
		}
		hasRelay = hasRelay || !node.STUNOnly
	}
	return hasRelay
}

func fetchDERPMap(ctx context.Context, source, mode string) (*tailcfg.DERPMap, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, source, nil)
	if err != nil {
		return nil, errors.New("DERP map configuration: invalid HTTPS URL")
	}
	request.Header.Set("Tailcat-Mode", mode)
	client := &http.Client{
		Timeout: startupTimeout,
		CheckRedirect: func(request *http.Request, previous []*http.Request) error {
			if _, err := derpMapSource(request.URL.String()); err != nil {
				return errors.New("unsafe DERP map redirect")
			}
			if len(previous) >= 10 {
				return errors.New("too many DERP map redirects")
			}
			return nil
		},
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, transportError(ctx, "DERP map fetch", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("DERP map fetch: HTTP %d", response.StatusCode)
	}
	const maxMapSize = 8 << 20
	body, err := io.ReadAll(io.LimitReader(response.Body, maxMapSize+1))
	if err != nil {
		return nil, transportError(ctx, "DERP map fetch", err)
	}
	var dm tailcfg.DERPMap
	if len(body) > maxMapSize || json.Unmarshal(body, &dm) != nil || len(dm.Regions) == 0 {
		return nil, errors.New("DERP map fetch: invalid DERP map")
	}
	for id, region := range dm.Regions {
		if !validDERPRegion(region) || id != region.RegionID {
			return nil, errors.New("DERP map fetch: invalid DERP map")
		}
	}
	return &dm, nil
}

func resolveDERP(ctx context.Context, ci *tailcat.ConnInfo, source, mode string) error {
	if len(ci.Region) == 0 {
		dm, err := fetchDERPMap(ctx, source, mode)
		if err != nil {
			return err
		}
		if ci.RegionID != -1 && dm.Regions[ci.RegionID] == nil {
			return errors.New("DERP map fetch: requested region is missing")
		}
		if err := ci.Expand(ctx, dm); err != nil {
			return transportError(ctx, "DERP relay selection", err)
		}
	}
	if len(ci.Region) == 0 {
		return errors.New("DERP relay/bootstrap: no relay in address")
	}
	for _, region := range ci.Region {
		if !validDERPRegion(region) {
			return errors.New("DERP relay/bootstrap: invalid or insecure relay configuration")
		}
	}
	return nil
}

func probeDERP(ctx context.Context, region *tailcfg.DERPRegion, identity key.NodePrivate) error {
	if insecure, _ := strconv.ParseBool(os.Getenv("TS_DEBUG_USE_DERP_HTTP")); insecure {
		return errors.New("DERP relay/bootstrap: insecure HTTP override is enabled")
	}
	// Tailcat has no system TUN routes to bypass; match its engine/netcheck setup.
	netns.SetEnabled(false)
	client := derphttp.NewRegionClient(identity, logger.Discard, netmon.NewStatic(), func() *tailcfg.DERPRegion { return region })
	defer client.Close()
	if err := client.Connect(ctx); err != nil {
		return transportError(ctx, "DERP relay/bootstrap", err)
	}
	stop := context.AfterFunc(ctx, func() { client.Close() })
	defer stop()
	message, err := client.Recv()
	if err != nil {
		return transportError(ctx, "DERP relay/bootstrap", err)
	}
	if _, ok := message.(derp.ServerInfoMessage); !ok {
		return errors.New("DERP relay/bootstrap: invalid handshake response")
	}
	return nil
}

func prepareServer(ctx context.Context, server *tailcat.Server, source string) error {
	server.DERPMapURL = source
	ci := &tailcat.ConnInfo{RegionID: server.RegionID}
	if server.Region != nil {
		ci.Region = []*tailcfg.DERPRegion{server.Region}
	} else if ci.RegionID == 0 {
		ci.RegionID = -1
	}
	if err := resolveDERP(ctx, ci, source, "server"); err != nil {
		return err
	}
	server.Region = ci.Region[0]
	if server.Key.IsZero() {
		server.Key = key.NewNode()
	}
	return probeDERP(ctx, server.Region, server.Key)
}

// Keep network work outside clientCache's global lock, and initialize once per
// cached tunnel so another session can reuse its established direct path.
type configuredClient struct {
	*tailcat.Client
	mu    sync.Mutex
	ready bool
}

func (client *configuredClient) prepare(ctx context.Context) error {
	client.mu.Lock()
	defer client.mu.Unlock()
	if client.ready {
		return nil
	}
	ci, err := tailcat.ParseAddr(client.Server)
	if err != nil {
		return errors.New("peer tunnel: invalid address")
	}
	if err := resolveDERP(ctx, &ci, client.DERPMapURL, "client"); err != nil {
		return err
	}
	if client.Key.IsZero() {
		client.Key = key.NewNode()
	}
	if err := probeDERP(ctx, ci.Region[0], client.Key); err != nil {
		return err
	}
	client.Server = ci.Addr()
	client.ready = true
	return nil
}

func (client *configuredClient) DialTCPPort(ctx context.Context, port uint16) (net.Conn, error) {
	if err := client.prepare(ctx); err != nil {
		return nil, err
	}
	conn, err := client.Client.DialTCPPort(ctx, port)
	if err != nil {
		return nil, transportError(ctx, "peer tunnel", err)
	}
	return conn, nil
}

func (client *configuredClient) DiscoPing(ctx context.Context) (*ipnstate.PingResult, error) {
	if err := client.prepare(ctx); err != nil {
		return nil, err
	}
	result, err := client.Client.DiscoPing(ctx)
	if err != nil {
		return nil, transportError(ctx, "peer tunnel", err)
	}
	return result, nil
}
