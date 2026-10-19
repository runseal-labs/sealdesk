package main

import "C"

import (
	"context"
	"encoding/json"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
	"tailscale.com/types/logger"
)

const (
	// selfProbeInterval is the steady gap between successful self-probes while
	// probing is allowed. It used to be 15s, which built a new DERP client on
	// every tick. Foreground checks now wait a full minute.
	selfProbeInterval = 60 * time.Second
	// legacySelfProbeInterval is the pre-throttle cadence, kept so measurements
	// compare against the old loop rather than a guessed rate.
	legacySelfProbeInterval = 15 * time.Second
	// selfProbeFresh is how long a successful probe keeps the server reachable.
	// Three steady intervals, so a 60s success does not flap to unreachable.
	selfProbeFresh = 3 * selfProbeInterval
	// selfProbeGrace is how long a running server may wait for the first
	// completed probe before the status flips from connecting to unreachable.
	selfProbeGrace = 30 * time.Second
	// selfProbeBackoffMax caps the failure backoff. The controlled service's
	// own DERP session is not this timer and is left running.
	selfProbeBackoffMax = 10 * time.Minute
	// probeSetupPackets counts one new DERP client per attempt (TLS dial plus
	// the engine that sends it). Meow frames are counted separately.
	probeSetupPackets = 1
	// Client.Ping resends meow once a second until ack or this timeout.
	probePingTimeout = 10 * time.Second
	probeMeowEvery   = time.Second
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

// probeDemand is who currently wants a self-probe. The serve DERP session
// stays up either way; this only gates the extra meow poll.
type probeDemand struct {
	shareVisible bool
	background   bool
	screenOff    bool
	incoming     bool
}

// allowed is true only while the share page is on screen or a session is
// already up, and never while idle in the background or with the screen off.
func (d probeDemand) allowed() bool {
	if !d.incoming && (d.background || d.screenOff) {
		return false
	}
	return d.shareVisible || d.incoming
}

// probeDelay is the wait after an attempt. Zero failures keeps the steady
// interval. Each consecutive failure doubles it, capped at selfProbeBackoffMax.
func probeDelay(failures int) time.Duration {
	if failures <= 0 {
		return selfProbeInterval
	}
	delay := selfProbeInterval
	for i := 1; i < failures; i++ {
		if delay >= selfProbeBackoffMax/2 {
			return selfProbeBackoffMax
		}
		delay *= 2
	}
	if delay > selfProbeBackoffMax {
		return selfProbeBackoffMax
	}
	return delay
}

// probePackets is the modeled packet count of one attempt: one DERP client
// setup plus the meow frames Ping actually sends. Success is one meow.
// Failure runs until probePingTimeout, one meow per probeMeowEvery.
func probePackets(ok bool) int {
	meows := 1
	if !ok {
		meows = int(probePingTimeout / probeMeowEvery)
	}
	return probeSetupPackets + meows
}

type probeTraffic struct {
	Probes  int
	Packets int
}

// simulateLegacyTraffic replays the old loop: a probe at once, then the 15s
// ticker, including the case where a long attempt has already consumed a tick.
func simulateLegacyTraffic(window, probeDur time.Duration, ok bool) probeTraffic {
	var out probeTraffic
	if window <= 0 {
		return out
	}
	t := time.Duration(0)
	nextTick := legacySelfProbeInterval
	for t < window {
		out.Probes++
		out.Packets += probePackets(ok)
		finished := t + probeDur
		for nextTick <= finished {
			nextTick += legacySelfProbeInterval
		}
		t = nextTick
		nextTick += legacySelfProbeInterval
	}
	return out
}

// probeSpan is one stretch of demand over a simulated window.
type probeSpan struct {
	End      time.Duration
	Demand   probeDemand
	OK       bool
	ProbeDur time.Duration
}

// simulateThrottledTraffic replays probeLoop's wait/backoff/park decisions
// without opening a socket. Spans must be sorted, non-empty, and cover time
// from 0 through the last End, which is the window.
func simulateThrottledTraffic(spans []probeSpan) probeTraffic {
	var out probeTraffic
	if len(spans) == 0 {
		return out
	}
	window := spans[len(spans)-1].End
	var failures int
	due := true
	var next time.Duration
	t := time.Duration(0)
	spanAt := func(at time.Duration) probeSpan {
		for _, sp := range spans {
			if at < sp.End {
				return sp
			}
		}
		return spans[len(spans)-1]
	}
	for t < window {
		sp := spanAt(t)
		if !sp.Demand.allowed() {
			failures = 0
			due = true
			if sp.End <= t {
				break
			}
			t = sp.End
			continue
		}
		if !due && next > t {
			wake := next
			if sp.End < wake {
				wake = sp.End
			}
			if window < wake {
				wake = window
			}
			if wake <= t {
				break
			}
			t = wake
			continue
		}
		out.Probes++
		out.Packets += probePackets(sp.OK)
		t += sp.ProbeDur
		if sp.OK {
			failures = 0
		} else {
			failures++
		}
		next = t + probeDelay(failures)
		due = false
	}
	return out
}

type probeUIState struct {
	share      bool
	background bool
	screenOff  bool
	incoming   bool
}

var (
	probeUIMu sync.Mutex
	probeUI   probeUIState
	// probeForce is set when demand transitions from not allowed to allowed.
	// The loop may still be inside a probe or about to arm a backoff, so the
	// flag, not a wake alone, is what schedules the immediate follow-up.
	probeForce bool
	// probeNotify wakes probeLoop when demand changes. A full buffer means a
	// wake is already pending.
	probeNotify = make(chan struct{}, 1)
)

// probeDelayNow is the live loop's wait. Tests replace it so a visibility
// return can be checked against a long backoff without sleeping that long.
var probeDelayNow = probeDelay

func demandFromUI(ui probeUIState) probeDemand {
	return probeDemand{
		shareVisible: ui.share,
		background:   ui.background,
		screenOff:    ui.screenOff,
		incoming:     ui.incoming,
	}
}

// loadProbeDemand returns the current demand and whether visibility just
// became allowed again. The force flag is one-shot: a return to the share
// page, foreground, or screen-on must probe now, not after the old backoff.
func loadProbeDemand() (probeDemand, bool) {
	probeUIMu.Lock()
	defer probeUIMu.Unlock()
	force := probeForce
	probeForce = false
	return demandFromUI(probeUI), force
}

func wakeProbe() {
	select {
	case probeNotify <- struct{}{}:
	default:
	}
}

func setProbeUi(share, background, screen bool) {
	probeUIMu.Lock()
	prev := demandFromUI(probeUI).allowed()
	changed := probeUI.share != share || probeUI.background != background || probeUI.screenOff != screen
	probeUI.share = share
	probeUI.background = background
	probeUI.screenOff = screen
	next := demandFromUI(probeUI).allowed()
	if changed && !prev && next {
		probeForce = true
	}
	probeUIMu.Unlock()
	if changed {
		wakeProbe()
	}
}

func setProbeIncoming(incoming bool) {
	probeUIMu.Lock()
	changed := probeUI.incoming != incoming
	probeUI.incoming = incoming
	probeUIMu.Unlock()
	if changed {
		wakeProbe()
	}
}

//export TcSetProbeUi
func TcSetProbeUi(share, background, screen C.int) {
	setProbeUi(share != 0, background != 0, screen != 0)
}

//export TcSetProbeIncoming
func TcSetProbeIncoming(incoming C.int) {
	setProbeIncoming(incoming != 0)
}

func waitProbeSignal(ctx context.Context) bool {
	select {
	case <-ctx.Done():
		return false
	case <-probeNotify:
		return true
	}
}

func waitProbeDelay(ctx context.Context, d time.Duration) bool {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-probeNotify:
		return true
	case <-timer.C:
		return true
	}
}

func probeLoop(ctx context.Context, gen uint64, addr tailcat.Addr, priv key.NodePrivate, source string, done chan struct{}) {
	defer close(done)
	// Pausing this loop does not close the server DERP session. That session
	// is what keeps the controlled side reachable; the meow here is only a
	// status poll.
	var failures int
	var next time.Time
	due := true
	for {
		if ctx.Err() != nil {
			return
		}
		demand, force := loadProbeDemand()
		if !demand.allowed() {
			failures = 0
			due = true
			next = time.Time{}
			if !waitProbeSignal(ctx) {
				return
			}
			continue
		}
		// Coming back to the share page, foreground, or screen-on probes
		// immediately. The previous failure backoff must not keep a stale
		// unreachable result on screen.
		if force {
			failures = 0
			due = true
			next = time.Time{}
		}
		if !due && !next.IsZero() {
			if wait := time.Until(next); wait > 0 {
				if !waitProbeDelay(ctx, wait) {
					return
				}
				continue
			}
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
		if err != nil {
			failures++
		} else {
			failures = 0
		}
		next = time.Now().Add(probeDelayNow(failures))
		due = false
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
