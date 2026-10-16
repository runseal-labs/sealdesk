# SealDesk

English | [简体中文](README.zh-CN.md)

SealDesk is a RustDesk fork for remote desktop sessions you start when needed.
Share this device, scan its pairing code, and connect without an account or a
RustDesk ID/relay server. Tailcat provides encrypted direct or DERP transport;
local devices can also discover and connect without Internet access.

**Alpha:** Linux x64 and Android arm64 are the current validation targets.
Other platforms retain upstream code and build recipes but are not yet validated
as complete SealDesk applications.

## Connections

| Path | Transport and authorization |
| --- | --- |
| SealDesk pairing code | Tailcat/WireGuard encryption and the existing session password or approval policy |
| SealDesk local discovery | Mandatory signed encrypted transport; first-use host approval, optional remembered controller keys and permissions |
| RustDesk direct-IP compatibility, either direction | The original direct-IP protocol and password/click authorization; **unencrypted**, with an explicit warning |

RustDesk compatibility is limited to discovery/direct-IP sessions. SealDesk does
not implement RustDesk ID registration, hbbs/hbbr, accounts, or address-book sync.
On an original RustDesk host, enable direct IP access before connecting.

Direct-IP access accepts IPv4 and IPv6 without a built-in subnet allowlist,
subject to routing, firewalls, and any explicitly configured access restrictions.
TCP **21118** is shared by SealDesk encrypted and RustDesk-compatible sessions.
Discovery uses IPv4 UDP **21119** on private LAN interfaces; adding IPv6 direct
access does not expand discovery to IPv6, public, or 100.64.0.0/10 interfaces.

## Use

1. Open SealDesk on the host and select **Share this device**.
2. On the controller, scan or import the QR image, or paste the complete
   `SEALDESK:` pairing code.
3. Without Internet access, select the host under **LAN devices** or Android's
   **Discovered** tab and accept the first request on the host.
4. Select **Trust this device** to remember the controller key and permissions.
   **Revoke access** disconnects it and requires approval again.

Pairing codes and `tc...` addresses contain access credentials. Do not post them,
screenshots of them, or unredacted logs in issues. QR import expiry is a client
check, not server-side revocation. Refresh the password or reset the address to
revoke the corresponding credential; exiting alone is not an address reset.

Linux installs as a normal desktop application, without a systemd daemon or
autostart. Normal launches start in control mode. Only sharing mode accepts
incoming sessions; stopping sharing disconnects them. Closing the main window
closes session windows and exits without leaving a background process.

With a Flutter desktop build installed:

```bash
sealdesk --host
sealdesk --control
sealdesk --control 'SEALDESK:V:1;A:<tc-address>;P:<temporary-password>;N:<name>;;'
sealdesk --control /path/to/pairing-code.png
sealdesk --tc-reset-address
```

The last command asks the running application to close host sessions, revoke the
old address, and resume sharing only if it was enabled. The Linux desktop package
rejects legacy `--service`, `--server`, and `--tray` daemon entry points.

## Build and install

See [BUILDING.md](BUILDING.md) for the Linux and Android build steps, required
native dependencies, bridge artifacts, and external Android signing setup.
A fresh checkout is not a self-contained Flutter build: it needs the matching
generated Rust/Dart bridge files and platform toolchains.

For a locally built Linux package, use
`sudo apt install ./sealdesk-<version>.deb`. Android testing uses the
`arm64-v8a` APK. See the repository's
[Releases](https://github.com/runseal-labs/sealdesk/releases) for any published
artifacts; do not use RustDesk downloads as SealDesk builds.

## Development

From a configured build environment:

```bash
cargo test --locked --lib --features linux-pkg-config tailcat::
cargo test --locked --lib --features linux-pkg-config lan::
go -C libs/tailcat-ffi test -race ./...
cd flutter
flutter test test/sealdesk_pairing_test.dart test/sealdesk_lan_panel_test.dart
```

These checks do not establish support for every desktop, mobile, or NAT setup.
See [Contributing](docs/CONTRIBUTING.md), [Security](docs/SECURITY.md),
[Fork maintenance](FORK.md), and the [current product scope](docs/sealdesk-prd.md).

## License and attribution

SealDesk is distributed under **GNU AGPL version 3**; the full, unchanged upstream
license text is in [LICENCE](LICENCE). RustDesk and other upstream contributors
retain their copyright and attribution. See [NOTICE](NOTICE) and
[third-party notices](THIRD_PARTY_NOTICES.md) for component licenses and source
references. SealDesk is an independent fork and is not endorsed by RustDesk or
Tailscale.

The standalone license scope of vendored hbb_common still needs upstream
clarification; see the [licensing follow-up](docs/license-and-firebase-follow-up.md).
