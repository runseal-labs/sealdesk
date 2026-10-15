# SealDesk fork

SealDesk is a RustDesk-derived application whose supported product scope is
SealDesk. The fork keeps the remote-desktop session implementation where it is
needed, but replaces the upstream connection path with Tailcat transport and
SealDesk pairing. Native desktop LAN sessions additionally use discovery and
a signed ephemeral key exchange with the retained encrypted Rust stream.

## Product boundary

Native desktop peers on the same private LAN can discover sharing hosts without
Internet access. A host approves the first connection and may remember the
controller public key and granted permissions. Revocation clears recovery
credentials and ends that controller's sessions. Only sharing mode binds the
LAN discovery and encrypted TCP listener; this does not restore upstream ID or
relay services.

SealDesk connects a host and a controller with a `tc...` address and a
`SEALDESK:` pairing code. Tailcat supplies the encrypted transport and direct
or DERP path. The fork does not use or implement the upstream RustDesk ID
(`hbbs`) or relay (`hbbr`) services, rendezvous registration, or upstream
hole-punching/relay path. A SealDesk/SealDesk session is therefore not
compatible with a stock RustDesk client or server.

The supported user-facing modes are:

```text
sealdesk --host
sealdesk --control [pairing-code-text | qr-image-path]
sealdesk --tc-reset-address
```

In a development checkout the executable is currently named `sealdesk` (for
example, `target/debug/sealdesk`). The product name shown by the SealDesk UI
and native configuration is SealDesk/SealDesk as implemented in the current
build; do not assume that a future binary rename has already happened.

## What belongs to this fork

Fork-owned code is concentrated in the Tailcat transport and SealDesk UI:

- `libs/tailcat-ffi/` — Go transport library and C ABI.
- `src/tailcat/` and `src/tailcat.rs` — Rust transport, pairing, launch,
  host, controller, and security integration.
- `flutter/lib/common/sealdesk_*`, `flutter/lib/desktop/pages/sealdesk_*`,
  `flutter/lib/mobile/**sealdesk_*`, and matching tests — pairing, QR, startup,
  and SealDesk connection UI.
- Build and CI wiring that compiles and packages the Tailcat library.

The integration hooks in upstream-shaped files should remain thin. The forked
connection files are maintained by hand because their upstream rendezvous,
hole-punching, and relay behavior was intentionally removed. The exact file groups and hook rationale are listed in this document.

## Baseline and upgrades

The original fork baseline is upstream RustDesk `1.5.1` (`upstream/1.5.1`,
commit `e910029f510fc9738424984eb3c1e7c63ede0d2a`). `libs/hbb_common` is vendored
in this repository; it is no longer a submodule.

Track upstream changes selectively rather than merging or rebasing the whole
upstream branch. Prioritize security, codecs, and platform fixes. Review each
patch against SealDesk's retained paths, preserve upstream authorship and add
`cherry picked from` provenance when backporting. Validate the affected paths
before accepting a patch. Never restore removed rendezvous, account, or
server-configuration features merely to apply an upstream patch.

### Upstream review: 2026-10-16

Reviewed through `e400bb75546cebf4f72a5ef3cf6e4a3efa2c3aa7` (10 commits after
the original baseline). This is a review checkpoint, not a fully merged baseline.

| Upstream commit | Decision | Reason / SealDesk commit |
| --- | --- | --- |
| `0d36ac0cb` | Applied | Avoid using a destroyed GTK window during close; `642d5ac26`. |
| `392188596` | Deferred | Static-frame quality refinement also changes encoder state and upgrades hwcodec to 0.8.0. Requires dedicated codec tests and Linux/Android runtime validation. |
| `f705902ec` | Deferred | Protected Windows installer temporary directories are relevant to the retained installer. Backport and verify ACL/UAC behavior before a Windows release. |
| `f1fa432bc` | Deferred | F-Droid bridge-generation fix needs adaptation to SealDesk's library names and build recipe. Current delivery uses the local arm64 build. |
| `e400bb755` | Deferred | Enforcing the bridge-stage lockfile conflicts with the recipe's extended_text downgrade and the committed lockfile; resolve the dependency mismatch first. |
| `c0750fb50` | Skipped | WebRTC connection racing has been removed. |
| `e614de83f` | Skipped | Executable host configuration parsing has been removed. |
| `538126d42` | Deferred | Portuguese shortcut translations; independent of the prioritized platform fixes. |
| `9c6d3fcb4` | Deferred | Italian shortcut translations; preserve the upstream translator's changes when importing. |
| `c230c5713` | Deferred | Dutch shortcut translations need a corrected description that retains the original guidance. |

The product version and peer protocol compatibility version are independent.
Updating this review checkpoint does not automatically change either version.

## Local development

From the repository root, a native Linux build is:

```bash
GO=/path/to/go cargo build --bin sealdesk
./target/debug/sealdesk --host
```

`build.rs` selects the Go target from Cargo's target platform and requires cgo
and a suitable C compiler. For a Flutter desktop package, use the repository
wrapper after installing the matching Flutter/platform toolchain:

```bash
python3 build.py --flutter --hwcodec
```

The wrapper and CI workflows also contain platform-specific packaging steps;
those are not interchangeable with a plain Cargo build. Generated Flutter
bridge files and signing/toolchain setup are environment-specific.

## Verification boundary

The SealDesk unit/transport checks and isolated Flutter tests are the reliable
local regression surface:

```bash
GO=/path/to/go cargo test --lib tailcat::
/path/to/go -C libs/tailcat-ffi test -race ./...
FLUTTER=/path/to/flutter bash tests/run_sealdesk_flutter_tests.sh
```

Passing these checks does not claim complete GUI first-frame validation,
real-device camera validation, every platform package build, or cross-NAT
success. Those remain platform/integration acceptance work documented in
`docs/sealdesk-prd.md` and the relevant CI workflows.
