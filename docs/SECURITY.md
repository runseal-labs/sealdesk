# SealDesk security policy

This repository is maintained independently of RustDesk. Do not send SealDesk
credentials or fork-specific vulnerability reports to RustDesk's support inbox.

## Private reports

Use [GitHub private vulnerability reporting](https://github.com/runseal-labs/sealdesk/security/advisories/new)
when it is enabled for this repository. If that form is unavailable, contact a
repository maintainer through the contact information on their GitHub profile
to arrange a private channel before sharing exploit details or private data.
There is no separate SealDesk security email address or guaranteed response SLA
published at present.

Include the affected source revision/build, operating systems, connection path
(Tailcat, encrypted LAN, or RustDesk-compatible direct IP), impact, and minimal
reproduction steps. Replace addresses, passwords, keys, and identifiable
screenshots with synthetic values.

## Scope and credentials

- SealDesk's own transports require encryption. RustDesk-compatible direct-IP
  sessions are unencrypted and explicitly identified.
- Pairing codes and `tc...` addresses are credentials. Never attach them to a
  public issue. QR expiry is an import check, not server-side revocation.
- Android signing material must remain outside the checkout.
- Reports for unmodified upstream behavior may also need coordination with
  RustDesk, but fork-specific triage starts here.

Only the latest source revision is actively maintained during the alpha phase.
No bug-bounty program or support for old alpha builds is promised.
