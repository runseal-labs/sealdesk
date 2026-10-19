package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
)

func TestDeriveServeState(t *testing.T) {
	now := time.UnixMilli(1_700_000_000_000)
	freshAt := now.Add(-5 * time.Second)
	cases := []struct {
		name string
		in   serveStateInput
		want serveStatusJSON
	}{
		{
			name: "stopped ignores a fresh probe",
			in: serveStateInput{
				InitStage: "key", InitError: "missing",
				ProbeDone: true, ProbeOK: true, ProbeAge: time.Second,
				ProbeAt: freshAt, ProbeRTT: 20 * time.Millisecond,
				SelfRelay: "nyc",
			},
			want: serveStatusJSON{State: "stopped", RegionCode: "nyc", InitStage: "key", InitError: "missing"},
		},
		{
			name: "reachable on a fresh successful probe",
			in: serveStateInput{
				Running: true, Since: 30 * time.Second,
				SelfRelay: "nyc", HomeRegion: 1, RegionCode: "old",
				ProbeDone: true, ProbeOK: true, ProbeAge: 5 * time.Second,
				ProbeAt: freshAt, ProbeRTT: 20 * time.Millisecond,
			},
			want: serveStatusJSON{
				Running: true, State: "reachable", HomeRegion: 1, RegionCode: "nyc",
				DERPConnected: true, SinceMs: 30000,
				ProbeRTTMs: 20, LastProbeMs: freshAt.UnixMilli(),
			},
		},
		{
			name: "success at the fresh limit is still reachable",
			in: serveStateInput{
				Running: true, Since: time.Minute,
				ProbeDone: true, ProbeOK: true, ProbeAge: selfProbeFresh,
				ProbeAt: now, ProbeRTT: time.Millisecond,
			},
			want: serveStatusJSON{
				Running: true, State: "reachable", DERPConnected: true,
				SinceMs: 60000, ProbeRTTMs: 1, LastProbeMs: now.UnixMilli(),
			},
		},
		{
			name: "success older than the fresh limit is unreachable",
			in: serveStateInput{
				Running: true, Since: time.Minute,
				SelfRelay: "nyc",
				ProbeDone: true, ProbeOK: true, ProbeAge: selfProbeFresh + time.Nanosecond,
				ProbeAt: now.Add(-selfProbeFresh - time.Nanosecond), ProbeRTT: 8 * time.Millisecond,
			},
			want: serveStatusJSON{
				Running: true, State: "unreachable", RegionCode: "nyc", SinceMs: 60000,
				ProbeRTTMs: 8, LastProbeMs: now.Add(-selfProbeFresh - time.Nanosecond).UnixMilli(),
			},
		},
		{
			name: "relay code alone is not reachable during grace",
			in: serveStateInput{
				Running: true, Since: selfProbeGrace - time.Millisecond,
				SelfRelay: "nyc", HomeRegion: 4,
			},
			want: serveStatusJSON{
				Running: true, State: "connecting", HomeRegion: 4,
				RegionCode: "nyc", SinceMs: 29999,
			},
		},
		{
			name: "no probe at 30s is unreachable",
			in:   serveStateInput{Running: true, Since: selfProbeGrace, RegionCode: "sea"},
			want: serveStatusJSON{
				Running: true, State: "unreachable", RegionCode: "sea", SinceMs: 30000,
			},
		},
		{
			name: "failed probe during grace is unreachable",
			in: serveStateInput{
				Running: true, Since: time.Second,
				SelfRelay: "nyc",
				ProbeDone: true, ProbeAge: 100 * time.Millisecond,
				ProbeAt: now, ProbeErr: "context deadline exceeded",
			},
			want: serveStatusJSON{
				Running: true, State: "unreachable", RegionCode: "nyc", SinceMs: 1000,
				LastProbeMs: now.UnixMilli(), LastError: "context deadline exceeded",
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := deriveServeState(tc.in)
			if got != tc.want {
				t.Fatalf("deriveServeState() = %+v, want %+v", got, tc.want)
			}
		})
	}
}

func TestServeStatusJSONShape(t *testing.T) {
	at := time.UnixMilli(1_700_000_123_456)
	out := deriveServeState(serveStateInput{
		Running: true, Since: 2 * time.Second,
		SelfRelay: "nyc", HomeRegion: 2,
		InitStage: initStageStart, InitError: "bind",
		ProbeDone: true, ProbeOK: true, ProbeAge: time.Second,
		ProbeAt: at, ProbeRTT: 42 * time.Millisecond,
	})
	raw, err := json.Marshal(out)
	if err != nil {
		t.Fatal(err)
	}
	var fields map[string]any
	if err := json.Unmarshal(raw, &fields); err != nil {
		t.Fatal(err)
	}
	wantKeys := []string{
		"running", "state", "home_region", "region_code",
		"derp_connected", "init_stage", "init_error", "since_ms",
		"probe_rtt_ms", "last_probe_ms", "last_error",
	}
	if len(fields) != len(wantKeys) {
		t.Fatalf("keys = %v", fields)
	}
	for _, k := range wantKeys {
		if _, ok := fields[k]; !ok {
			t.Fatalf("missing %q in %s", k, raw)
		}
	}
	if fields["running"] != true || fields["state"] != "reachable" ||
		fields["derp_connected"] != true || fields["region_code"] != "nyc" ||
		fields["home_region"] != float64(2) || fields["since_ms"] != float64(2000) ||
		fields["init_stage"] != "start" || fields["init_error"] != "bind" ||
		fields["probe_rtt_ms"] != float64(42) || fields["last_probe_ms"] != float64(at.UnixMilli()) ||
		fields["last_error"] != "" {
		t.Fatalf("shape = %s", raw)
	}
	if !strings.Contains(string(raw), `"since_ms":2000`) || !strings.Contains(string(raw), `"probe_rtt_ms":42`) {
		t.Fatalf("durations not ints: %s", raw)
	}
}

func TestReachableProbeRTTJSONIsNonZero(t *testing.T) {
	at := time.UnixMilli(1_700_000_000_000)
	out := deriveServeState(serveStateInput{
		Running: true, Since: time.Second,
		ProbeDone: true, ProbeOK: true, ProbeAge: time.Second,
		ProbeAt: at, ProbeRTT: 200 * time.Microsecond,
	})
	if out.State != serveStateReachable {
		t.Fatalf("state = %q", out.State)
	}
	raw, err := json.Marshal(out)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(raw), `"probe_rtt_ms":`) {
		t.Fatalf("missing probe_rtt_ms: %s", raw)
	}
	var fields map[string]any
	if err := json.Unmarshal(raw, &fields); err != nil {
		t.Fatal(err)
	}
	rtt, _ := fields["probe_rtt_ms"].(float64)
	if rtt <= 0 {
		t.Fatalf("reachable probe_rtt_ms = %v, json %s", fields["probe_rtt_ms"], raw)
	}
}

func TestScrubProbeError(t *testing.T) {
	addr := "tcABCDEFGHIJKLMNOPQRSTUVWXYZ012345"
	pub := "nodekey:00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
	msg := fmt.Sprintf("dial %s as %s and privkey:ffee and discokey:ab", addr, pub)
	got := scrubProbeError(msg, addr, pub)
	if strings.Contains(got, addr) || strings.Contains(got, "00112233") || strings.Contains(got, "privkey:ff") || strings.Contains(got, "discokey:ab") {
		t.Fatalf("secret left in %q", got)
	}
	if !strings.Contains(got, "dial [redacted]") {
		t.Fatalf("scrubbed = %q", got)
	}
}

func TestAllowWithProbe(t *testing.T) {
	probe := key.NewNode().Public()
	other := key.NewNode().Public()
	denied := key.NewNode().Public()
	wrapped := allowWithProbe(func(k key.NodePublic) bool { return k == other }, probe)
	if !wrapped(probe) || !wrapped(other) || wrapped(denied) {
		t.Fatalf("allow list: probe %v other %v denied %v", wrapped(probe), wrapped(other), wrapped(denied))
	}
	all := allowWithProbe(nil, probe)
	if !all(probe) || !all(denied) {
		t.Fatal("nil allow hook should still allow every client")
	}
}

func TestProbeLoopRecordsAndTcStopServeStopsIt(t *testing.T) {
	mu.Lock()
	if server != nil || probeCancel != nil {
		mu.Unlock()
		t.Fatal("server or probe already running")
	}
	savedPing := probePing
	savedGen := probeGen
	savedRes := probeRes
	savedAddr := probeAddr
	savedPub := probePubText
	mu.Unlock()

	entered := make(chan struct{})
	var calls int
	probePing = func(ctx context.Context, cl *tailcat.Client) (time.Duration, error) {
		calls++
		if calls == 1 {
			close(entered)
			return 1234 * time.Millisecond, nil
		}
		<-ctx.Done()
		return 0, ctx.Err()
	}
	t.Cleanup(func() {
		setProbeUi(false, false, false)
		setProbeIncoming(false)
		mu.Lock()
		cancel := probeCancel
		done := probeDone
		mu.Unlock()
		if cancel != nil {
			cancel()
		}
		if done != nil {
			select {
			case <-done:
			case <-time.After(3 * time.Second):
				t.Errorf("probe goroutine still running after cleanup")
			}
		}
		mu.Lock()
		probePing = savedPing
		probeGen = savedGen
		probeRes = savedRes
		probeAddr = savedAddr
		probePubText = savedPub
		probeCancel = nil
		probeDone = nil
		mu.Unlock()
	})

	setProbeUi(true, false, false)
	mu.Lock()
	startSelfProbeLocked("tc-probe-test-address", key.NewNode(), "https://tailcat.example/derpmap.json")
	done := probeDone
	mu.Unlock()

	select {
	case <-entered:
	case <-time.After(2 * time.Second):
		t.Fatal("probe did not start")
	}
	deadline := time.Now().Add(2 * time.Second)
	for {
		mu.Lock()
		got := probeRes
		mu.Unlock()
		if got.completed {
			if !got.ok || got.rtt != 1234*time.Millisecond || got.err != "" || got.at.IsZero() {
				t.Fatalf("recorded %+v", got)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("probe result was not recorded")
		}
		time.Sleep(5 * time.Millisecond)
	}

	if got := goStringFree(TcStopServe()); got != "" {
		t.Fatalf("TcStopServe = %q", got)
	}
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("TcStopServe left the probe goroutine running")
	}
	mu.Lock()
	if probeCancel != nil || probeRes.completed {
		mu.Unlock()
		t.Fatal("probe state was not cleared")
	}
	mu.Unlock()
}

func TestLastInitErrRecordAndClear(t *testing.T) {
	mu.Lock()
	if server != nil {
		mu.Unlock()
		t.Fatal("server already running")
	}
	savedErr := lastInitErr
	savedAt := serveStartedAt
	mu.Unlock()
	t.Cleanup(func() {
		mu.Lock()
		lastInitErr = savedErr
		serveStartedAt = savedAt
		mu.Unlock()
	})

	keyPath := "/var/lib/sealdesk/identity.key"
	keyErr := fmt.Errorf("parse %s: %w", keyPath, errors.New("invalid character"))
	mu.Lock()
	setLastInitErrLocked(initStageKey, keyErr, keyPath)
	got := lastInitErr
	mu.Unlock()
	if got.Stage != "key" {
		t.Fatalf("stage = %q", got.Stage)
	}
	if got.At.IsZero() {
		t.Fatal("timestamp not set")
	}
	if strings.Contains(got.Msg, keyPath) || strings.Contains(got.Msg, "preshared") {
		t.Fatalf("message leaked path or key material: %q", got.Msg)
	}
	if got.Msg != "parse : invalid character" {
		t.Fatalf("msg = %q", got.Msg)
	}
	stopped := marshalServeStatus()
	if !strings.Contains(stopped, `"state":"stopped"`) ||
		!strings.Contains(stopped, `"init_stage":"key"`) ||
		!strings.Contains(stopped, `"init_error":"parse : invalid character"`) ||
		strings.Contains(stopped, keyPath) {
		t.Fatalf("status json = %s", stopped)
	}

	mu.Lock()
	setLastInitErrLocked(initStageStart, errors.New("listen failed"), keyPath)
	if lastInitErr.Stage != "start" || lastInitErr.Msg != "listen failed" {
		t.Fatalf("overwrite = %+v", lastInitErr)
	}
	clearLastInitErrLocked()
	if lastInitErr != (lastInitError{}) {
		t.Fatalf("not cleared: %+v", lastInitErr)
	}
	mu.Unlock()
	cleared := marshalServeStatus()
	want := `{"running":false,"state":"stopped","home_region":0,"region_code":"","derp_connected":false,"init_stage":"","init_error":"","since_ms":0,"probe_rtt_ms":0,"last_probe_ms":0,"last_error":""}`
	if cleared != want {
		t.Fatalf("cleared json = %s", cleared)
	}
}

func TestProbeAllowedOnlyForSharePageOrSession(t *testing.T) {
	cases := []struct {
		name string
		in   probeDemand
		want bool
	}{
		{"idle foreground", probeDemand{}, false},
		{"share page", probeDemand{shareVisible: true}, true},
		{"background no session", probeDemand{background: true}, false},
		{"screen off no session", probeDemand{screenOff: true}, false},
		{"share page but background", probeDemand{shareVisible: true, background: true}, false},
		{"share page but screen off", probeDemand{shareVisible: true, screenOff: true}, false},
		{"incoming while screen off", probeDemand{screenOff: true, incoming: true}, true},
		{"incoming in background", probeDemand{background: true, incoming: true}, true},
		{"incoming on share page", probeDemand{shareVisible: true, incoming: true}, true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := tc.in.allowed(); got != tc.want {
				t.Fatalf("allowed() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestProbeDelayBackoffCapsAndResets(t *testing.T) {
	if probeDelay(0) != selfProbeInterval || probeDelay(-1) != selfProbeInterval {
		t.Fatalf("steady = %s", probeDelay(0))
	}
	want := []time.Duration{
		selfProbeInterval,
		2 * selfProbeInterval,
		4 * selfProbeInterval,
		8 * selfProbeInterval,
	}
	for i, d := range want {
		if got := probeDelay(i + 1); got != d {
			t.Fatalf("failures %d = %s, want %s", i+1, got, d)
		}
	}
	if got := probeDelay(5); got != selfProbeBackoffMax {
		t.Fatalf("cap = %s", got)
	}
	if got := probeDelay(100); got != selfProbeBackoffMax {
		t.Fatalf("large cap = %s", got)
	}
}

func TestProbeLoadPerMinute(t *testing.T) {
	const ten = 10 * time.Minute
	share := probeDemand{shareVisible: true}
	idle := probeDemand{background: true, screenOff: true}
	held := probeDemand{screenOff: true, incoming: true}

	legacyOK := simulateLegacyTraffic(ten, 0, true)
	legacyFail := simulateLegacyTraffic(ten, probePingTimeout, false)
	throttledOK := simulateThrottledTraffic([]probeSpan{{
		End: ten, Demand: share, OK: true,
	}})
	throttledFail := simulateThrottledTraffic([]probeSpan{{
		End: ten, Demand: share, OK: false, ProbeDur: probePingTimeout,
	}})
	throttledIdle := simulateThrottledTraffic([]probeSpan{{
		End: ten, Demand: idle, OK: false, ProbeDur: probePingTimeout,
	}})
	throttledHeld := simulateThrottledTraffic([]probeSpan{{
		End: ten, Demand: held, OK: true,
	}})
	switched := simulateThrottledTraffic([]probeSpan{
		{End: 5 * time.Minute, Demand: idle, OK: false, ProbeDur: probePingTimeout},
		{End: ten, Demand: share, OK: true},
	})

	if legacyOK.Probes != 40 || legacyOK.Packets != 80 {
		t.Fatalf("legacy success over 10m = %+v", legacyOK)
	}
	if legacyFail.Probes != 40 || legacyFail.Packets != 440 {
		t.Fatalf("legacy failure over 10m = %+v", legacyFail)
	}
	if throttledOK.Probes != 10 || throttledOK.Packets != 20 {
		t.Fatalf("throttled success over 10m = %+v", throttledOK)
	}
	if throttledFail.Probes != 4 || throttledFail.Packets != 44 {
		t.Fatalf("throttled failure over 10m = %+v", throttledFail)
	}
	if throttledIdle.Probes != 0 || throttledIdle.Packets != 0 {
		t.Fatalf("idle = %+v", throttledIdle)
	}
	if throttledHeld != throttledOK {
		t.Fatalf("incoming while screen off = %+v, want %+v", throttledHeld, throttledOK)
	}
	if switched.Probes != 5 || switched.Packets != 10 {
		t.Fatalf("background then share page = %+v", switched)
	}

	// One failure arms a 60s backoff. Hiding for less than that, then showing
	// the share page again, must probe immediately instead of waiting it out.
	failEnd := probePingTimeout
	hiddenEnd := failEnd + selfProbeInterval/2
	returned := simulateThrottledTraffic([]probeSpan{
		{End: failEnd, Demand: share, OK: false, ProbeDur: probePingTimeout},
		{End: hiddenEnd, Demand: idle, OK: false, ProbeDur: probePingTimeout},
		{End: hiddenEnd + time.Millisecond, Demand: share, OK: true},
	})
	if returned.Probes != 2 {
		t.Fatalf("visibility return during backoff = %+v, want 2 probes", returned)
	}

	oneOK := simulateLegacyTraffic(time.Minute, 0, true)
	oneNew := simulateThrottledTraffic([]probeSpan{{End: time.Minute, Demand: share, OK: true}})
	oneFail := simulateLegacyTraffic(time.Minute, probePingTimeout, false)
	oneNewFail := simulateThrottledTraffic([]probeSpan{{
		End: time.Minute, Demand: share, OK: false, ProbeDur: probePingTimeout,
	}})
	if oneOK.Probes != 4 || oneNew.Probes != 1 || oneFail.Probes != 4 || oneNewFail.Probes != 1 {
		t.Fatalf("one minute legacy/new success %d/%d failure %d/%d", oneOK.Probes, oneNew.Probes, oneFail.Probes, oneNewFail.Probes)
	}
	if oneNewFail.Packets != probePackets(false) || oneFail.Packets != 4*probePackets(false) {
		t.Fatalf("one minute packets legacy %d new %d", oneFail.Packets, oneNewFail.Packets)
	}
}

func TestProbeLoopParksUntilDemandChanges(t *testing.T) {
	mu.Lock()
	if server != nil || probeCancel != nil {
		mu.Unlock()
		t.Fatal("server or probe already running")
	}
	mu.Unlock()
	savedPing := probePing
	setProbeUi(false, false, false)
	setProbeIncoming(false)
	var calls atomic.Int32
	probePing = func(ctx context.Context, cl *tailcat.Client) (time.Duration, error) {
		calls.Add(1)
		return 5 * time.Millisecond, nil
	}
	t.Cleanup(func() {
		mu.Lock()
		cancel := probeCancel
		done := probeDone
		mu.Unlock()
		if cancel != nil {
			cancel()
		}
		if done != nil {
			select {
			case <-done:
			case <-time.After(2 * time.Second):
				t.Errorf("probe goroutine still running")
			}
		}
		probePing = savedPing
		setProbeUi(false, false, false)
		setProbeIncoming(false)
	})

	mu.Lock()
	startSelfProbeLocked("tc-probe-park", key.NewNode(), "https://tailcat.example/derpmap.json")
	mu.Unlock()
	time.Sleep(40 * time.Millisecond)
	if calls.Load() != 0 {
		t.Fatalf("probed while idle: %d", calls.Load())
	}
	setProbeUi(true, true, false)
	time.Sleep(40 * time.Millisecond)
	if calls.Load() != 0 {
		t.Fatalf("probed in background with no session: %d", calls.Load())
	}
	setProbeIncoming(true)
	deadline := time.Now().Add(2 * time.Second)
	for calls.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if calls.Load() != 1 {
		t.Fatalf("incoming session probes = %d", calls.Load())
	}
	setProbeIncoming(false)
	time.Sleep(40 * time.Millisecond)
	if calls.Load() != 1 {
		t.Fatalf("kept probing after the session ended: %d", calls.Load())
	}
}

func stopLeftoverProbe(t *testing.T) {
	t.Helper()
	mu.Lock()
	if server != nil {
		mu.Unlock()
		t.Fatal("server already running")
	}
	cancel := probeCancel
	done := probeDone
	mu.Unlock()
	if cancel != nil {
		cancel()
	}
	if done != nil {
		select {
		case <-done:
		case <-time.After(2 * time.Second):
			t.Fatal("probe goroutine still running")
		}
	}
	mu.Lock()
	if probeDone == done {
		probeCancel = nil
		probeDone = nil
	}
	mu.Unlock()
}

func TestProbeVisibilityReturnIgnoresBackoff(t *testing.T) {
	stopLeftoverProbe(t)

	savedPing := probePing
	savedDelay := probeDelayNow
	probeDelayNow = func(int) time.Duration { return time.Hour }
	release := make(chan struct{})
	var releaseOnce sync.Once
	letGo := func() { releaseOnce.Do(func() { close(release) }) }
	var calls atomic.Int32
	probePing = func(ctx context.Context, cl *tailcat.Client) (time.Duration, error) {
		if calls.Add(1) == 1 {
			<-release
		}
		return 0, errors.New("relay down")
	}
	setProbeUi(true, false, false)
	setProbeIncoming(false)
	select {
	case <-probeNotify:
	default:
	}
	probeUIMu.Lock()
	probeForce = false
	probeUIMu.Unlock()

	t.Cleanup(func() {
		mu.Lock()
		cancel := probeCancel
		done := probeDone
		mu.Unlock()
		if cancel != nil {
			cancel()
		}
		letGo()
		if done != nil {
			select {
			case <-done:
			case <-time.After(2 * time.Second):
				t.Errorf("probe goroutine still running")
			}
		}
		// probeLoop reads probeDelayNow until it returns. Restoring the hook
		// before done is closed races with that read under -race.
		probeDelayNow = savedDelay
		probePing = savedPing
		setProbeUi(false, false, false)
		setProbeIncoming(false)
		probeUIMu.Lock()
		probeForce = false
		probeUIMu.Unlock()
		mu.Lock()
		if probeDone == done {
			probeCancel = nil
			probeDone = nil
		}
		mu.Unlock()
	})

	mu.Lock()
	startSelfProbeLocked("tc-probe-visible", key.NewNode(), "https://tailcat.example/derpmap.json")
	mu.Unlock()
	deadline := time.Now().Add(2 * time.Second)
	for calls.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if calls.Load() != 1 {
		t.Fatalf("first probe = %d", calls.Load())
	}

	// The loop is blocked inside the probe, so it cannot observe this gap.
	// Returning to the share page must still force one probe when the attempt
	// finishes, instead of arming the hour-long failure backoff.
	setProbeUi(false, false, false)
	setProbeUi(true, false, false)
	letGo()

	deadline = time.Now().Add(time.Second)
	for calls.Load() < 2 && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	if calls.Load() != 2 {
		t.Fatalf("visibility false->true probes = %d, want 2", calls.Load())
	}
}

func TestProbeForceSetWhenVisibilityReturns(t *testing.T) {
	setProbeIncoming(false)
	cases := []struct {
		name string
		hide func()
		show func()
	}{
		{"share page", func() { setProbeUi(false, false, false) }, func() { setProbeUi(true, false, false) }},
		{"foreground", func() { setProbeUi(true, true, false) }, func() { setProbeUi(true, false, false) }},
		{"screen on", func() { setProbeUi(true, false, true) }, func() { setProbeUi(true, false, false) }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			setProbeUi(true, false, false)
			probeUIMu.Lock()
			probeForce = false
			probeUIMu.Unlock()
			tc.hide()
			tc.show()
			probeUIMu.Lock()
			force := probeForce
			probeForce = false
			probeUIMu.Unlock()
			if !force {
				t.Fatal("visibility return did not request an immediate probe")
			}
		})
	}
}
