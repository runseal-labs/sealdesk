# Maintaining the SealDesk fork

SealDesk is an independent RustDesk fork focused on user-initiated remote
desktop sessions. It retains the session, permissions, codecs, input, clipboard,
and file-transfer implementation where used. It replaces the RustDesk ID-server
connection path with Tailcat pairing and restores limited discovery/direct-IP
interoperability.

## Product boundary

- Public/Internet connections use `tc...` addresses and `SEALDESK:` pairing
  codes over Tailcat's encrypted direct or DERP transport.
- SealDesk local discovery and direct transport work without Internet access,
  with signed key exchange, encrypted sessions, and host approval.
- Hosts may remember controller keys and permissions and later revoke them.
- Original RustDesk peers use their existing discovery/direct-IP protocol and
  password or click authorization. These compatibility sessions are unencrypted
  and explicitly identified. SealDesk encrypted-handshake failures do not fall
  back to the legacy protocol.
- Both direct protocols share TCP 21118. Direct admission accepts IPv4 and IPv6
  without subnet classification. Discovery remains private-IPv4 UDP 21119.
- Incoming sessions are accepted only while sharing is enabled. Linux has no
  installed systemd daemon, autostart, or tray process.
- RustDesk hbbs/hbbr registration and relay, account synchronization, and address
  books are outside this fork's supported product scope.

The Tailcat service key is stored in local application configuration. Do not
describe it as rotating on every launch. Reset the address explicitly when
revoking it; normal stop/start is a lifecycle action, not credential revocation.

## Ownership

| Area | Fork-owned responsibility |
| --- | --- |
| `libs/tailcat-ffi/` | Go encrypted networking and C ABI |
| `src/tailcat.rs`, `src/tailcat/` | Rust transport, pairing, local channel authentication, lifecycle, recovery |
| `src/lan.rs` | Discovery, signed encrypted direct transport, shared-port protocol selection |
| `flutter/lib/**/sealdesk_*` | Pairing, scanner, discovery, and host/controller UI |
| `libs/base/src/config/keys.rs` | Shared configuration option names |
| `build.rs`, `build.py`, platform packaging | Native transport integration, branding, application lifecycle |

`libs/hbb_common` is vendored, not a Git submodule. Legacy Sciter UI sources
were removed. Keep integration changes to retained session and platform code
small; do not remove authentication, permissions, or local IPC merely because
a transport feature was removed.

## Upstream baseline and selective upgrades

The original baseline is upstream RustDesk 1.5.1:
`e910029f510fc9738424984eb3c1e7c63ede0d2a` (local tag `upstream/1.5.1`).
SealDesk's product version and retained peer-protocol version are independent.

Review upstream security, codec, and platform changes individually. Preserve
authorship and `cherry picked from` provenance; do not merge removed ID/account
features back merely to apply an unrelated fix.

### Review checkpoint: 2026-10-16

Reviewed through upstream
`53996e3c404ea5083df0775057e4149e49102084`. This is a review checkpoint,
not a claim that all intervening changes were merged.

| Upstream commit | Decision | Reason |
| --- | --- | --- |
| `0d36ac0cb` | Applied | GTK close handling avoids a destroyed window; retained with upstream authorship |
| `392188596` | In progress | Static-frame re-encode and hwcodec 0.8.0 (#16128); separate PR, merge after macOS device testing |
| `f705902ec` | Ported | Windows installer scripts use a protected temporary directory (#16554, this PR) |
| `f1fa432bc` | Deferred | F-Droid bridge changes (#16560) remain postponed |
| `e400bb755` | Deferred | F-Droid lockfile enforcement remains postponed |
| `c0750fb50` | Skipped | Removed WebRTC connection-racing path |
| `e614de83f` | Skipped | Removed executable host-configuration path |
| `53996e3c` | Ported | Russian translations (#16570, this PR); WebRTC strings omitted |
| `538126d42` | Ported | Portuguese translations (#16555, this PR); WebRTC strings omitted |
| `9c6d3fcb4` | Ported | Italian translations (#16551, this PR); WebRTC strings omitted |
| `c230c5713` | Ported | Dutch translations (#16540, this PR); WebRTC strings omitted |

After the pre-publication history cleanup, fork commits are grouped by feature;
use upstream source IDs rather than superseded local commit IDs in maintenance
records.

## Build, checks, and documentation

See [BUILDING.md](BUILDING.md) for the supported build recipe and
[docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) for focused checks.
A successful library build does not establish GUI, mobile-device, or cross-NAT
acceptance. Keep those results tied to the tested platform and source revision.

The old multilingual `docs/README-*.md` files are inherited RustDesk references.
The maintained SealDesk guides are [README.md](README.md) and
[README.zh-CN.md](README.zh-CN.md).
