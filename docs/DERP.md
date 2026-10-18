# DERP map configuration and startup failures

On the desktop home page, open **Relay settings** in either sharing or controller
mode. Enter an HTTPS URL serving a Tailcat/Tailscale DERP map, then **Save**. The
setting persists as `tailcat-derp-map-url` in SealDesk's existing options. An empty
setting uses `https://tailcat.dev/derpmap.json`, unchanged from the default. Choose
**Reset to default**, then **Save**, to remove the override. URL credentials and
fragments are not accepted; do not put passwords, API tokens, or pairing codes in
the URL. Use only a map provider you trust.

Saving while sharing stops and restarts sharing, disconnecting current sessions.
A new source can select a different relay and change the pairing address; share
the newly generated code. With an unchanged source, an already saved embedded
relay remains pinned to preserve the address. Use **Reset address** if that
relay has disappeared and you need a fresh selection from the same source. Controller changes apply to newly opened tunnels;
existing connections continue on their original tunnel. Host and client both
receive the saved source. Tailcat addresses containing an embedded relay still
use that relay, because it is where that peer can be reached; changing the map
cannot move a remote host. Compact region-ID addresses resolve against the
configured map. Both devices need compatible region definitions.

Startup has a bounded network budget. Failures appear in the sharing status
instead of an indefinite “Generating pairing code” state. Correct the settings
or network issue and choose **Retry**. Failure stages distinguish:

- **DERP map configuration**: malformed or non-HTTPS URL.
- **DERP map fetch / validation**: timeout, DNS/connection refusal, HTTP status,
  invalid JSON or unusable relay definitions.
- **Relay/bootstrap**: a map was available, but the selected DERP relay could not
  complete a TLS/DERP connection. A reachable map alone does not make remote
  control possible.

TLS certificate and hostname verification stay enabled for maps and relays;
redirects cannot downgrade map requests to HTTP. Errors do not include URL
credentials, request bodies, private keys, or pairing codes. The application does
not silently switch map providers, disable TLS, or add an offline map cache.
Local network discovery and direct LAN sessions are independent of map fetching.

## Reproduce and verify

Use an isolated development configuration and synthetic pairing credentials.
Run `go -C libs/tailcat-ffi test -race ./...` and the transport archive smoke test
in `tests/build_tailcat_archive.sh`. The Go regressions cover map configuration,
fetch failures, propagation to both roles, and relay/bootstrap failure. The C
smoke test exercises the configured exports across the FFI boundary. Flutter
widget regressions are in `flutter/test/sealdesk_home_page_test.dart`; run them
with the pinned Flutter SDK and source-matched generated bridge files described
in [BUILDING.md](../BUILDING.md).

For desktop acceptance, try an invalid URL, a trusted HTTPS server returning
invalid map JSON or HTTP errors, an unreachable/slow map endpoint, and a valid
map whose relay is unreachable. Confirm the error stage, change the source,
retry, and check that no obsolete QR code remains. Stop sharing during startup,
retry repeatedly, restart the app to verify persistence, and reset to default.
These tests require actual app/runtime verification; transport tests alone do
not establish end-to-end remote-control support on every platform.
