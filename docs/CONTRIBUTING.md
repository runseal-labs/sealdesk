# Contributing to SealDesk

File issues and pull requests in
[runseal-labs/sealdesk](https://github.com/runseal-labs/sealdesk), not the upstream
RustDesk tracker. English and Chinese reports are welcome.
For vulnerabilities, use [the security policy](SECURITY.md) instead of a public
issue containing credentials or exploit details.

## Changes

- Target `main` and keep changes focused, with Conventional Commit messages.
- Read [FORK.md](../FORK.md) and [BUILDING.md](../BUILDING.md) before changing
  retained upstream code or platform integration.
- Preserve upstream copyright, authorship, and component licenses.
- Do not commit signing keys, pairing codes, local configuration, generated
  packages, or files containing your SDK/toolchain paths.
- Keep README.md and README.zh-CN.md consistent when changing user-facing behavior.
- For a bug fix, add or run the relevant behavior check; do not infer runtime
  support from a successful library build alone.

## Checks

From a configured Linux build environment, run the relevant subset:

```bash
cargo test --locked --lib --features linux-pkg-config tailcat::
cargo test --locked --lib --features linux-pkg-config lan::
go -C libs/tailcat-ffi test -race ./...
cd flutter
flutter test test/sealdesk_pairing_test.dart test/sealdesk_lan_panel_test.dart
```

GUI, mobile camera/permission, reconnect, and lifecycle changes also need actual
application verification for the affected platform. Use isolated configurations
and networks when a test could otherwise disturb existing sessions.

Bridge generation requires the pinned toolchain recorded in
[bridge.yml](../.github/workflows/bridge.yml); do not regenerate all bridge files
as a side effect of an unrelated change. Check the repository's AGENTS.md for
additional implementation guidance.

Contributions are distributed under the existing [AGPL license](../LICENCE),
with separately licensed components retaining their notices.
