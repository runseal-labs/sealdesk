package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
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
			name: "success at 45s is still reachable",
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
			name: "success older than 45s is unreachable",
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

	mu.Lock()
	startSelfProbeLocked("tc-probe-test-address", key.NewNode())
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
