# SealDesk

English | [简体中文](README.zh-CN.md)

SealDesk is a RustDesk-derived remote-desktop application with a SealDesk-only
connection path. It is intended to connect two devices using a SealDesk
(`tc...`) address and pairing code, without a RustDesk ID server or RustDesk
relay server.

This repository is a fork, not a drop-in RustDesk client. SealDesk sessions do
not interoperate with stock RustDesk sessions, and this fork does not provide
the upstream rendezvous, hole-punching, or relay implementation. See
[FORK.md](FORK.md) for the maintained fork boundary, patch layers, and upgrade
rules.

## Current scope

- Host mode publishes a SealDesk address and pairing code.
- Control mode accepts a `SEALDESK:` pairing-code string or a QR-image path.
- Tailcat provides the encrypted direct/DERP transport for public connections.
- Native desktop LAN discovery and encrypted direct connections work without Internet access.
  Select a sharing device under **LAN devices** and approve on the host.
  Android controllers use the **Discovered** tab with the same encrypted connection and host approval.
  Original RustDesk peers are also listed: enable direct IP access on the host, then use its password or click approval. RustDesk IP sessions are unencrypted and require explicit confirmation; SealDesk sessions remain encrypted.
  **Trust this device** remembers the controller key and granted permissions;
  **Revoke access** ends its sessions and requires approval again.
  Original RustDesk controllers can also discover SealDesk and request access on standard TCP 21118. This compatibility listener is unencrypted, uses upstream password/click approval, and runs only while sharing is enabled. The host displays a plaintext warning. Both protocols share TCP 21118. SealDesk declares its encrypted protocol first and never falls back to plaintext after a handshake failure.
  LAN discovery listens on UDP 21119 and desktop connections on TCP 21118,
  only while sharing is enabled. LAN access does not need a pairing password or require enabling 2FA.
- The existing remote-desktop session, input, audio, clipboard, and file
  transfer code is retained where it is used by SealDesk sessions.
- The Flutter UI contains the SealDesk host/control and QR flows; platform
  packaging and complete native Flutter builds still require their platform
  toolchains and generated bridge artifacts.

The pairing code is a remote-access credential. Treat it like a password and
share it only with the intended operator. Reset the host address after a
`tc...` address is exposed.

## Build prerequisites

For a native desktop build, install:

- Rust with the toolchain required by `Cargo.toml` (Rust 1.75 or newer).
- Go 1.27.1, as specified by `libs/tailcat-ffi/go.mod`.
- A C compiler and the native dependencies required by the target platform.

The Cargo build script compiles `libs/tailcat-ffi` with cgo and links it into
SealDesk. Set `GO=/path/to/go` when the required Go executable is not on
`PATH`.

## Build and run on Linux

Run commands from the repository root:

```bash
git submodule update --init --recursive
GO=/path/to/go cargo build --bin sealdesk
```

The development executable is `target/debug/sealdesk`. Start a host, or start
the control UI, with:

```bash
./target/debug/sealdesk --host
./target/debug/sealdesk --control 'SEALDESK:V:1;A:<tc-address>;P:<temporary-password>;N:<name>;;'
./target/debug/sealdesk --control /path/to/pairing-code.png
```

The control image path is supported by the Flutter UI. `--control` with text
requires a complete pairing-code payload. To revoke the current host address
and invalidate old pairing codes:

```bash
./target/debug/sealdesk --tc-reset-address
```

Useful diagnostic commands are `--tc-addr`, `--tc-pubkey`, `--tc-pair`, and
`--tc-allow`. The latter changes the Tailcat allow-list and requires restarting
the host service to apply it.

## Linux desktop lifecycle

The Linux package installs a normal desktop application. It does not install,
enable, or start a systemd daemon. Launch `sealdesk` from the application menu
or terminal when needed. Package upgrades retire legacy SealDesk service units;
they do not enable an alternative background service.

Normal launches always start in control mode. Even if sharing was enabled before
exit, accepting connections requires selecting Share this device again. Closing
the main window stops sharing, closes session windows, and exits the application
without leaving a tray or background process. Use `sealdesk --host` to explicitly
start sharing. The Linux desktop package rejects the legacy `--service`,
`--server`, and `--tray` daemon entry points.

## Android release signing

Release builds use an external keystore and properties file; neither belongs in
this repository. The default properties path is `~/.sealdesk/key.properties`.
Set `SEALDESK_KEY_PROPERTIES` to use another path outside the repository.

Create and protect the directory, then generate or obtain the release keystore
through your approved key-management process. The keystore path should also be
outside the repository, for example `~/.sealdesk/release.jks`:

```bash
mkdir -p ~/.sealdesk
chmod 700 ~/.sealdesk
# Generate a keystore only when you are ready to retain and back it up safely.
keytool -genkeypair -v \\
  -keystore ~/.sealdesk/release.jks \\
  -alias sealdesk \\
  -keyalg RSA -keysize 2048 -validity 10000
chmod 600 ~/.sealdesk/release.jks
```

Write `~/.sealdesk/key.properties` with the values for that keystore. Use an
absolute `storeFile` path so builds are independent of Gradle's project
working directory:

```properties
storeFile=/home/your-user/.sealdesk/release.jks
storePassword=<keystore-password>
keyAlias=sealdesk
keyPassword=<key-password>
```

Protect the properties file (`chmod 600 ~/.sealdesk/key.properties`) and never
commit it or paste its contents into logs, issues, or CI output. CI should
provide an equivalent external file and keystore through its secret-management
system. Release Gradle tasks fail early when the properties or keystore are
missing; debug builds do not require release credentials.

## Flutter desktop build

The repository build wrapper assembles the native library and Flutter bundle.
With the matching Flutter SDK and platform dependencies installed, run from
the repository root:

```bash
python3 build.py --flutter --hwcodec
```

The wrapper invokes Cargo, builds the Flutter desktop target, and packages the
platform output. The exact package and native prerequisites vary by Linux,
macOS, and Windows. A complete Flutter build also needs the generated bridge
artifacts produced by the project CI/toolchain; they are not regenerated by
this documentation.

## Focused checks

These checks cover the SealDesk-specific code and do not replace full
platform, GUI, or cross-network acceptance:

```bash
GO=/path/to/go cargo test --lib tailcat::
GO=/path/to/go go -C libs/tailcat-ffi test -race ./...
FLUTTER=/path/to/flutter bash tests/run_sealdesk_flutter_tests.sh
```

The Flutter script creates an isolated test project and runs analysis plus the
SealDesk tests. The live security and CLI smoke tests require a successfully
built native test binary and use temporary configuration; consult the scripts
before running them on a machine with an existing SealDesk service.

## Project references

- [FORK.md](FORK.md) — fork boundary, ownership, and maintenance rules.
- [FORK.md](FORK.md) — upstream baseline, patch layers, and upgrade plan.
- [docs/sealdesk-prd.md](docs/sealdesk-prd.md) — SealDesk pairing and product
  behavior, including known validation limits.
- [libs/tailcat-ffi/README.md](libs/tailcat-ffi/README.md) — Tailcat library,
  cross-compilation, and Android library checks.

## License

SealDesk retains the licensing terms of the upstream RustDesk-derived code and
the licenses of its bundled dependencies. Consult the upstream notices and the
individual component licenses included with each dependency.
