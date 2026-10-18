package main

import (
	"context"
	"encoding/json"
	"regexp"
	"strings"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
	"tailscale.com/types/logger"
)

const (
	// selfProbeInterval is how often the serve self-probe sends a meow.
	// The first probe runs as soon as the goroutine starts.
	selfProbeInterval = 15 * time.Second
	// selfProbeFresh is how long a successful probe keeps the server reachable.
	selfProbeFresh = 45 * time.Second
	// selfProbeGrace is how long a running server may wait for the first
	// completed probe before the status flips from connecting to unreachable.
	selfProbeGrace = 30 * time.Second
)

const (
	serveStateStopped     = "stopped"
	serveStateConnecting  = "connecting"
	serveStateReachable   = "reachable"
	serveStateUnreachable = "unreachable"

	initStageConfig = "config"
	initStageKey    = "key"
	initStageAllow  = "allow"
	initStageMap    = "map"
	initStageRelay  = "relay"
	// initStageStart remains for callers that record a pre-classification failure.
	initStageStart = "start"
)

// probeSecretRE matches key material and tailcat addresses that must not
// appear in last_error. Tailcat addresses embed the server public key and
// the pre-shared key.
var probeSecretRE = regexp.MustCompile(`(?i)(?:nodekey:|privkey:|discokey:)[0-9a-f]+|tc[A-Za-z0-9_-]{16,}`)

// lastInitError is the most recent TcServe failure. Stage is "config", "key",
// "allow", "map", "relay", or "" when the last attempt succeeded or none has
// been made. Guarded by mu, as is serveStartedAt and the self-probe fields below.
type lastInitError struct {
	Stage string
	Msg   string
	At    time.Time
}

// probeResult is the last completed self-probe. Guarded by mu.
type probeResult struct {
	completed bool
	ok        bool
	at        time.Time
	rtt       time.Duration
	err       string
}

var (
	lastInitErr    lastInitError
	serveStartedAt time.Time

	probeGen     uint64
	probeCancel  context.CancelFunc
	probeDone    chan struct{}
	probeRes     probeResult
	probeAddr    string
	probePubText string
	probePing    = defaultProbePing
)

// defaultProbePing is Client.Ping against the controlled server's own
// tailcat address. Ping's meow/meowed handshake uses DERP and times out
// internally after 10s; ctx cancellation (TcStopServe) aborts it sooner.
//
// Ping times only the first meowed on this client. It closes meowWait once,
// and a later Ping selects that closed channel immediately, returning a
// sub-millisecond latency and a nil error even when the meow was not sent.
// probeLoop therefore passes a client that has not pinged before.
func defaultProbePing(ctx context.Context, cl *tailcat.Client) (time.Duration, error) {
	res, err := cl.Ping(ctx)
	if err != nil {
		return 0, err
	}
	return res.Latency, nil
}

// allowWithProbe admits the self-probe key in addition to user.
//
// A nil user hook allows every client, matching a nil Server.AllowClient.
// Otherwise the server does not ack meow from a key the hook rejects
// (locoBackend.onMeow returns false, and no "meowed" is sent), so a probe
// key that is not pre-authorized can never succeed while an allow list is set.
//
// A successful meow still admits that key as a tailcat client: it is added
// to the server network map once and shows up as a connected peer. Later
// meows from the same key are acked without consulting AllowClient again.
// Ping does not open a TCP flow, so OnTCP is not called, and WireGuard does
// not install the peer until a handshake arrives. The probe reuses one
// ephemeral key, so this is a single extra peer for the life of the serve.
func allowWithProbe(user func(key.NodePublic) bool, probe key.NodePublic) func(key.NodePublic) bool {
	return func(k key.NodePublic) bool {
		if k == probe {
			return true
		}
		if user == nil {
			return true
		}
		return user(k)
	}
}

// startSelfProbeLocked launches the DERP self-probe. mu must be held.
func startSelfProbeLocked(addr tailcat.Addr, priv key.NodePrivate, source string) {
	stopSelfProbeLocked()
	probeAddr = string(addr)
	if text, err := priv.Public().MarshalText(); err == nil {
		probePubText = string(text)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	probeCancel = cancel
	probeDone = done
	gen := probeGen
	go probeLoop(ctx, gen, addr, priv, source, done)
}

// stopSelfProbeLocked cancels the self-probe, if any, and forgets its last
// result. It does not wait for the goroutine: the loop exits on ctx.Done
// without needing mu, then closes its client. mu must be held.
func stopSelfProbeLocked() {
	probeGen++
	if probeCancel != nil {
		probeCancel()
		probeCancel = nil
	}
	probeDone = nil
	probeRes = probeResult{}
	probeAddr = ""
	probePubText = ""
}

func probeLoop(ctx context.Context, gen uint64, addr tailcat.Addr, priv key.NodePrivate, source string, done chan struct{}) {
	defer close(done)
	ticker := time.NewTicker(selfProbeInterval)
	defer ticker.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		// A new client each round so Ping waits for this meow. Reusing one
		// client reports success with a sub-ms latency after the first ack.
		rtt, err := probeOnce(ctx, addr, priv, source)
		if ctx.Err() != nil {
			return
		}
		if !noteProbe(gen, rtt, err) {
			return
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

func probeOnce(ctx context.Context, addr tailcat.Addr, priv key.NodePrivate, source string) (time.Duration, error) {
	cl := tailcat.NewClient(addr)
	cl.Key = priv
	cl.Logf = logger.Discard
	cl.DERPMapURL = source
	defer cl.Close()
	return probePing(ctx, cl)
}

// noteProbe records one completed probe. It reports false when gen is no
// longer the active probe (TcStopServe or a newer TcServe).
func noteProbe(gen uint64, rtt time.Duration, err error) bool {
	mu.Lock()
	defer mu.Unlock()
	if gen != probeGen {
		return false
	}
	probeRes = probeResult{completed: true, at: time.Now()}
	if err != nil {
		probeRes.err = scrubProbeError(err.Error(), probeAddr, probePubText)
		return true
	}
	probeRes.ok = true
	probeRes.rtt = rtt
	return true
}

// scrubProbeError removes tailcat addresses and typed key encodings from a
// probe error before it is copied into status JSON.
func scrubProbeError(msg, addr, pubText string) string {
	if addr != "" {
		msg = strings.ReplaceAll(msg, addr, "[redacted]")
	}
	if pubText != "" {
		msg = strings.ReplaceAll(msg, pubText, "[redacted]")
	}
	return probeSecretRE.ReplaceAllString(msg, "[redacted]")
}

// setLastInitErrLocked records a TcServe failure. Msg is err.Error() with
// keyPath removed so the status JSON does not repeat the identity file
// location. Callers must not append key material, the preshared key, or file
// contents; mu must be held.
func setLastInitErrLocked(stage string, err error, keyPath string) {
	msg := ""
	if err != nil {
		msg = err.Error()
		if keyPath != "" {
			msg = strings.ReplaceAll(msg, keyPath, "")
		}
	}
	lastInitErr = lastInitError{Stage: stage, Msg: msg, At: time.Now()}
}

func clearLastInitErrLocked() {
	lastInitErr = lastInitError{}
}

// serveStatusJSON is the TcServeStatus document.
type serveStatusJSON struct {
	Running       bool   `json:"running"`
	State         string `json:"state"`
	HomeRegion    int    `json:"home_region"`
	RegionCode    string `json:"region_code"`
	DERPConnected bool   `json:"derp_connected"`
	InitStage     string `json:"init_stage"`
	InitError     string `json:"init_error"`
	SinceMs       int64  `json:"since_ms"`
	ProbeRTTMs    int64  `json:"probe_rtt_ms"`
	LastProbeMs   int64  `json:"last_probe_ms"`
	LastError     string `json:"last_error"`
}

// serveStateInput is the slice of state deriveServeState needs. SelfRelay is
// only a region code (ipnstate.Status.Self.Relay). Reachability comes from
// the self-probe, not from Relay: magicsock setNearestDERP stores the home
// region and dials it asynchronously, so a non-empty Relay can be set while
// DERP TLS is still blocked.
type serveStateInput struct {
	Running    bool
	Since      time.Duration
	SelfRelay  string
	HomeRegion int
	RegionCode string
	InitStage  string
	InitError  string

	// ProbeDone is set once a probe has finished, success or failure.
	// ProbeAge is how long ago it finished. ProbeAt is that completion
	// time; last_probe_ms is its Unix millisecond timestamp.
	ProbeDone bool
	ProbeOK   bool
	ProbeAge  time.Duration
	ProbeAt   time.Time
	ProbeRTT  time.Duration
	ProbeErr  string
}

// probeRTTMs is the probe round trip in milliseconds, rounded up.
// Duration.Milliseconds truncates, so a sub-millisecond meow would be stored
// as 0 and the share chip would omit the reading. A positive duration is at
// least 1.
func probeRTTMs(d time.Duration) int64 {
	if d <= 0 {
		return 0
	}
	ms := (int64(d) + int64(time.Millisecond) - 1) / int64(time.Millisecond)
	if ms < 1 {
		return 1
	}
	return ms
}

func deriveServeState(in serveStateInput) serveStatusJSON {
	code := in.RegionCode
	if in.SelfRelay != "" {
		code = in.SelfRelay
	}
	out := serveStatusJSON{
		Running:    in.Running,
		HomeRegion: in.HomeRegion,
		RegionCode: code,
		InitStage:  in.InitStage,
		InitError:  in.InitError,
	}
	if !in.Running {
		out.State = serveStateStopped
		return out
	}
	since := in.Since
	if since < 0 {
		since = 0
	}
	out.SinceMs = since.Milliseconds()
	if in.ProbeDone {
		if !in.ProbeAt.IsZero() {
			out.LastProbeMs = in.ProbeAt.UnixMilli()
		}
		if in.ProbeOK {
			out.ProbeRTTMs = probeRTTMs(in.ProbeRTT)
		} else {
			out.LastError = in.ProbeErr
		}
	}
	age := in.ProbeAge
	if age < 0 {
		age = 0
	}
	switch {
	case in.ProbeDone && in.ProbeOK && age <= selfProbeFresh:
		out.State = serveStateReachable
		out.DERPConnected = true
	case !in.ProbeDone && since < selfProbeGrace:
		out.State = serveStateConnecting
	default:
		out.State = serveStateUnreachable
	}
	return out
}

// currentServeStatusLocked snapshots the controlled-side tunnel. mu must be held.
func currentServeStatusLocked() serveStatusJSON {
	in := serveStateInput{
		InitStage: lastInitErr.Stage,
		InitError: lastInitErr.Msg,
	}
	if server == nil {
		return deriveServeState(in)
	}
	in.Running = true
	if !serveStartedAt.IsZero() {
		in.Since = time.Since(serveStartedAt)
	}
	in.HomeRegion = int(server.RegionID)
	if server.Region != nil {
		if in.HomeRegion == 0 {
			in.HomeRegion = int(server.Region.RegionID)
		}
		in.RegionCode = server.Region.RegionCode
	}
	if st := server.Status(); st != nil && st.Self != nil {
		in.SelfRelay = st.Self.Relay
	}
	if probeRes.completed {
		in.ProbeDone = true
		in.ProbeOK = probeRes.ok
		in.ProbeAt = probeRes.at
		in.ProbeAge = time.Since(probeRes.at)
		in.ProbeRTT = probeRes.rtt
		in.ProbeErr = probeRes.err
	}
	return deriveServeState(in)
}

func marshalServeStatus() string {
	mu.Lock()
	defer mu.Unlock()
	b, err := json.Marshal(currentServeStatusLocked())
	if err != nil {
		return "ERR:" + err.Error()
	}
	return string(b)
}
