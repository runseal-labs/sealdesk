# Security

## Reporting a vulnerability

To report a security issue, contact the Tailscale security team as
described at https://tailscale.com/.well-known/security.txt.

## Threat model

Tailscale (the company) treats the security of the Tailscale core very
seriously, and tailcat is built from those same production components:
WireGuard, magicsock, DERP, and gVisor's netstack.

The tailcat wrapper around them, however, is an early experimental
tool. It has a lot of powerful features, but it was originally
designed for use with oneself: the same person running both ends. Its
threat model hasn't historically included malicious adversaries, such
as tailcat use between two different parties, one of whom might be
trying to attack you.

We recognize that people will inevitably and increasingly use tailcat
between mutually untrusting parties, and we do want to harden it for
those use cases over time. Until then, be thoughtful about accepting
tailcat addresses from, or serving powerful things (shells, writable
directories, exit nodes) to, people you don't trust. Security reports
that help us get there are very welcome.

## Hall of Thanks

Thanks to the people who've reported security issues in tailcat:

* [Will Frame](https://wpf.nz/) reported two rounds of issues with
  write-only (`:wo`) file shares (drop boxes), as used by
  `tailcat recv`:
  * In 0.4.0, senders could write over existing files, and could test
    whether a guessed filename existed by how opening it behaved.
    Fixed in
    [d796f883e](https://github.com/tailscale/tailcat/commit/d796f883e5ec8b17f6c4196276fad65646d101f5).
  * That fix left narrower ways to test guessed names: exclusive
    creation failed on a collision, and directories could be stat'd.
    Fixed by storing each upload under a server-chosen name and
    making directory support a separate opt-in
    (`tailcat recv --accept-dirs`).
* [Matt Andreko](https://www.mattandreko.com/) reported two issues
  with how untrusted tailcat addresses are handled. Both are mostly
  attacking-yourself issues today, but they matter for automation, or
  any time you get a tailcat address from an untrusted party:
  * Invalid tailcat addresses were passed unvalidated to ssh/scp
    child processes, fixed in
    [aba9d9ba2](https://github.com/tailscale/tailcat/commit/aba9d9ba255380ab58b12abf8dbcb17cf1f5a649).
  * Mistyped tailcat addresses leaked to DNS as hostname lookups,
    fixed in
    [5cb1ec356](https://github.com/tailscale/tailcat/commit/5cb1ec3566617f5667764456fd1f2ee7cb46a366).
* [Dinnerb0ne](https://github.com/Dinnerb0ne) reported a panic
  reachable via a meow packet with a zero disco key. A crash (denial
  of service) only, but one an anonymous stranger could trigger via
  DERP. Fixed in
  [79da910c4](https://github.com/tailscale/tailcat/commit/79da910c40a65125c318f41f22a8ac7f5c5c5efd).
* [Heyang Zhou](https://x.com/heyang_zhou) reported that reusing the
  node key as the disco key exposed the unlisted node public key on
  direct UDP paths, fixed in
  [cb1e0d753](https://github.com/tailscale/tailcat/commit/cb1e0d753e9ace2ebc5bff147ccf1eee6ccdd463).
