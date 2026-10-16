# Draft: hbb_common license clarification request

**Status: prepared locally; not posted or sent.**

Suggested destination:
[existing RustDesk discussion #15801](https://github.com/rustdesk/rustdesk/discussions/15801).

The following is the proposed comment:

---

Hello RustDesk maintainers,

We are preparing an AGPLv3 RustDesk-derived source distribution and want to
preserve the correct license and attribution for hbb_common.

Our RustDesk baseline is
`e910029f510fc9738424984eb3c1e7c63ede0d2a`, which pins hbb_common to
`5d52530b636fb296a324260b074780cf559e0093`. The current public hbb_common main
also points to that commit at the time of this check.

We found the parent RustDesk repository's AGPL license and recursive submodule
build, but at the pinned hbb_common revision we could not find a repository
license file, source-level licensing declaration, or Cargo license metadata.
We have retained the source provenance and existing attribution and have not
invented a license declaration for the vendored crate.

Could an authorized rights holder clarify:

1. Which license applies to hbb_common at that exact revision?
2. Does it cover that complete snapshot and the historical/contributed code,
   or are any files, schemas, generated material, or contributions excluded?
3. If the RustDesk root AGPL grant is intended to cover the separate hbb_common
   repository too, could that scope be recorded explicitly?
4. Could the declaration be added to the upstream repository with the matching
   Cargo metadata, together with a commit/PR link?

We are not asking for a proprietary license exception. We need a verifiable
statement of the existing permission and its historical scope before marking
the dependency's licensing review complete.

Thank you.
