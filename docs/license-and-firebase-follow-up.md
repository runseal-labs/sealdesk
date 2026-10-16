# hbb_common licensing and Firebase follow-up

Checked on 2026-10-16 against SealDesk
`ab76ed4133038c49281f6478b0d41613bbf557ed` and live upstream metadata.
No API-key values are reproduced in this record.

## hbb_common: what upstream actually does

RustDesk's original fork baseline pins `libs/hbb_common` as a Git submodule to
`5d52530b636fb296a324260b074780cf559e0093`. The live RustDesk master and
hbb_common main still reference that revision at this check.

Sources:

- [RustDesk baseline submodule](https://github.com/rustdesk/rustdesk/tree/e910029f510fc9738424984eb3c1e7c63ede0d2a/libs/hbb_common)
- [Upstream hbb_common tree](https://github.com/rustdesk/hbb_common/tree/5d52530b636fb296a324260b074780cf559e0093)
- [Upstream crate manifest](https://github.com/rustdesk/hbb_common/blob/5d52530b636fb296a324260b074780cf559e0093/Cargo.toml)
- [RustDesk baseline AGPL text](https://github.com/rustdesk/rustdesk/blob/e910029f510fc9738424984eb3c1e7c63ede0d2a/LICENCE)
- [RustDesk submodule configuration](https://github.com/rustdesk/rustdesk/blob/e910029f510fc9738424984eb3c1e7c63ede0d2a/.gitmodules)

The complete upstream hbb_common tree has no LICENSE, LICENCE, COPYING, or NOTICE
file; its manifest has neither `license` nor `license-file`. GitHub's repository
license metadata is null. The API history queries for LICENSE, LICENCE, and
COPYING also returned no commits. RustDesk itself keeps an AGPL license at the
main-repository root and its application build checks out submodules recursively.
That explains the original application's build/distribution structure. It is
not an explicit standalone grant or historical-coverage statement for this
separate repository.

[Upstream discussion #15801](https://github.com/rustdesk/rustdesk/discussions/15801)
asks the same question for an older pinned version. The live discussion has no
accepted answer; both comments are from non-maintainers, with no rights-holder
clarification as of this check. It is evidence of an unanswered request, not
evidence of a particular license grant.

### Corrected SealDesk provenance

The prior notice identified intermediate snapshot
`1085d5852567b057573a46816a20211fdaa8319e` as if it were a public upstream
revision. The hash occurs in SealDesk's development history but is not available
from the public hbb_common repository. The verifiable upstream base is
`5d52530b636fb296a324260b074780cf559e0093`.

Comparing Git blob IDs at the review baseline: upstream has 33 files and the
vendored tree has 22; 16 files are identical, 6 are modified, 11 are removed,
and none are added. The modified files are Cargo.toml, src/config.rs, src/lib.rs,
src/socket_client.rs, src/stream.rs, and src/tcp.rs. This comparison establishes
source provenance, not licensing permission.

The notices now distinguish the public upstream base from SealDesk's local
modifications. No license file or SPDX declaration has been invented for
hbb_common. The standalone permission/coverage question remains unresolved;
publishing a license label ourselves would not settle it.

A [draft clarification request](upstream-license-request.md) is ready for an
authorized maintainer to post in the existing upstream discussion. It has not
been sent. Closing this item needs a rights-holder statement that identifies
the license and historical coverage, preferably recorded in the upstream
repository. This review cannot supply that statement on the holder's behalf.

## Firebase: provenance and current status

The three scanner findings correspond to two distinct client API keys. They
belong to the historical upstream project named `rustdesk`, not to a SealDesk
Firebase project.

| Upstream commit | Historical file | Role |
| --- | --- | --- |
| [873439452](https://github.com/rustdesk/rustdesk/blob/87343945237b5bbc21733dcbdf5409926f7f71d8/flutter_hbb/android/app/google-services.json) | flutter_hbb/android/app/google-services.json | Android client configuration, introduced for Google analytics |
| [3fd0982af](https://github.com/rustdesk/rustdesk/blob/3fd0982af57fb62e8f994bedfc546310189df0c3/web/index.html) | web/index.html | Web Firebase app / analytics initialization |
| [4887cdb75](https://github.com/rustdesk/rustdesk/blob/4887cdb7566c6231a3d1879e94d49c6d6a76b4ba/flutter/web/index.html) | flutter/web/index.html | The same web client configuration after directory refactoring |

For all three, the raw bytes from RustDesk's public repository matched the
corresponding local historical Git blob exactly. These are client configuration
files/objects, not Google service-account private keys.

[Firebase's API-key documentation](https://firebase.google.com/docs/projects/api-keys)
distinguishes project identification from authorization: Firebase client keys
are designed to be public, with access governed by service-side rules and
restrictions. That does not establish the restrictions or current validity of
RustDesk's keys. This review did not use them to call a service, inspect data,
or test an account.

### Cleanup and verification

- None of the three historical files exists in the current tracked source.
- Current Dart/Gradle manifests and the Flutter lockfile do not enable Firebase
  dependencies or its Google-services plugin.
- Dead Firebase initialization/observer/dependency comments and obsolete F-Droid
  stripping steps were removed. No active dependency or runtime code was changed.
- Firebase client configuration filenames are ignored to prevent accidental
  restoration in this application.
- The existing Android test artifact
  `sealdesk-0.1.0-alpha.1-main-eafd77f3c-arm64-v8a.apk` was checked across every
  ZIP member: neither key, nor the historical Firebase project domains or
  analytics measurement ID, is present. This is a check of that artifact, not
  a claim that a new APK was built for this documentation change.
- Upstream Git history and authorship are retained. Rewriting them merely to hide
  public upstream client configuration would not revoke or secure those keys.

Disposition: the Firebase findings are classified and the current application
residuals removed. They are not SealDesk-owned secret credentials requiring
rotation. Any restrictions or rotation of the original project's keys belongs
to its owner.
