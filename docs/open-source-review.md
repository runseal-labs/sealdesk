# Pre-publication review: 2026-10-16

Review baseline: `40212abc752b4021d7315f8e9c5d04f530977f33`.
This record covers the source repository and the fixes in this changeset.
It does not change repository visibility or publish an application release.

## Sensitive information and history

Gitleaks 8.30.1 was downloaded from its official release and its archive checksum
verified. Scans used full redaction; no suspected credential values are recorded
here.

- The tracked baseline snapshot produced 82 matches, all in translation strings
  describing API keys/direct-IP syntax. Review found no credential in those strings.
- The `main` history scan produced 144 matches. All originating commits precede
  the RustDesk fork baseline. They consist of translations, test/sample strings,
  minified upstream client code, and three Firebase client-configuration API-key
  entries. Their public upstream provenance and current application absence are
  now confirmed in the [follow-up](license-and-firebase-follow-up.md). They were
  not used to access services or test authorization.
- No match was introduced by SealDesk's feature commits.
- The public remote refs at review time were `main` and `upstream/1.5.1`;
  the tag is an ancestor of main. Old private backup bundles and local candidate
  refs were not pushed.
- No keystore, signing-properties file, private-key file, environment file, APK,
  or Debian package is tracked in the current tree.
- The final source review found no development machine names, private download
  links, or developer home paths. Generic OS home-directory fallback code,
  container accounts, test paths, and example placeholders remain.
- Test fixture machine names and a development-device test IP were replaced
  with synthetic values. Ignore rules now cover signing material, private
  environment files, local test directories, and generated packages.
- Host startup no longer logs the tc access address or its QR code.

Inherited public client configuration is not a claim that all historical
external credentials are safe or inactive. It is kept separate from credentials
created or owned by this fork; no upstream history or author attribution was
silently removed.

Reproduce the history scan with an installed Gitleaks:

```bash
gitleaks git . --log-opts="--full-history HEAD" --redact=100 --report-format json --report-path /tmp/sealdesk-history-secrets.json
```

The command reports inherited matches as well as new ones. Review the redacted
report rather than suppressing entire translation/test directories. Keep reports
outside the repository and do not attach unredacted scanner output to issues.

## Documentation

- English/Chinese README commands and feature boundaries are aligned.
- Direct-IP interoperability is distinguished from RustDesk ID-server support.
- IPv4/IPv6 direct access, unchanged private-IPv4 discovery, encryption differences,
  click approval, remembered controllers, reconnect, and exit behavior are documented.
- BUILDING.md replaces obsolete Sciter/native-binary and multi-ABI instructions.
  Matching bridge artifacts and platform dependencies remain explicit prerequisites.
- The PRD now describes current implementation rather than accumulating conflicting
  prototype logs. It does not promise per-launch identity rotation or server-side
  QR expiration.
- Inherited multilingual README/contribution documents are marked as upstream
  references. Current support and contribution instructions point to SealDesk.
- The inherited RustDesk sponsorship configuration was removed; vulnerability
  reporting no longer directs SealDesk credentials to an upstream support inbox.

## Licensing and packages

The original AGPL text remains byte-identical to the upstream baseline. Cargo
metadata, README, NOTICE, third-party provenance, and direct transport license
texts identify the licensing boundary without replacing component copyrights.

The vendored hbb_common tree contains no standalone license file. The
[follow-up](license-and-firebase-follow-up.md) now identifies its public upstream
base, documents RustDesk's actual submodule handling, and records the unanswered
rights-holder clarification. This licensing item is not closed. The third-party
table is not an exhaustive binary license inventory.

The checked Linux package contains the complete project/direct-dependency notice
asset both in the Flutter bundle and as Debian copyright documentation. The MSI
RTF is generated from the same notice text and checked for text equivalence;
this review did not build or run a Windows installer or a fresh Android package.

## Verification

- Rust Tailcat checks: 36 passed, 5 environment-dependent tests ignored.
- Rust LAN checks: 8 passed.
- Flutter pairing/LAN checks: 22 passed.
- Linux release native library and Debian package built successfully.
- Package notice payloads and native-library hashes checked; no systemd units
  or autostart files.
- Maintained documentation links, README command parity, issue YAML, Python
  syntax, Cargo metadata, and ignore rules checked.

Platform runtime acceptance is not inferred from documentation or package-content
checks. Binary releases still need their actual dependency closure/notices and
corresponding source revision.

## Follow-up disposition

Firebase client configuration: public upstream provenance verified, inactive
source remnants removed, and the existing Android test artifact checked.
hbb_common licensing: exact source and original RustDesk handling verified;
explicit standalone permission/coverage is still awaiting upstream confirmation.
See [the evidence](license-and-firebase-follow-up.md) and the
[unposted request draft](upstream-license-request.md).
