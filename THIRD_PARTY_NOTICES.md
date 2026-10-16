# License and third-party notices

SealDesk retains the GNU Affero General Public License version 3 text from its
RustDesk baseline, unchanged in [LICENCE](LICENCE). This is a modified fork;
the upstream history and source-level copyright notices remain intact.

## Included source and direct transport dependencies

| Component | Source / version | License reference |
| --- | --- | --- |
| RustDesk-derived application | [RustDesk](https://github.com/rustdesk/rustdesk), original baseline e910029f510fc9738424984eb3c1e7c63ede0d2a | [AGPL v3](LICENCE) |
| Vendored hbb_common | [rustdesk/hbb_common](https://github.com/rustdesk/hbb_common), imported revision 1085d5852567b057573a46816a20211fdaa8319e, with local changes | Retain the source headers and RustDesk attribution; no separate license file was included in the imported tree |
| enigo | [pythoneer/enigo](https://github.com/enigo-rs/enigo), locally modified | [MIT text](libs/enigo/LICENSE) |
| scrap | [quadrupleslap/scrap](https://github.com/quadrupleslap/scrap), locally modified | MIT, as declared in [its manifest](libs/scrap/Cargo.toml) and source notices |
| Tailcat | [tailscale/tailcat](https://github.com/tailscale/tailcat), 52fbad323e9d | [BSD 3-Clause text](third_party/licenses/tailscale-BSD-3-Clause.txt) |
| Tailscale | [tailscale/tailscale](https://github.com/tailscale/tailscale), a0e471a35b8f | [BSD 3-Clause text](third_party/licenses/tailscale-BSD-3-Clause.txt) |
| rsc.io/qr | [rsc/qr](https://github.com/rsc/qr), v0.2.0 | [BSD 3-Clause text](third_party/licenses/qr-BSD-3-Clause.txt) |

The imported hbb_common revision did not include a standalone license file.
This record preserves its provenance; it does not invent an independent license
grant. Resolve any additional licensing requirements with the upstream holders
before distributing a component under different terms.

The table is a source/provenance guide, not an exhaustive binary dependency
inventory. Cargo.lock, the Go module files, flutter/pubspec.lock, vcpkg.json, and
the selected platform features determine the actual dependency closure. Retain
each component's own notices, including those supplied by native libraries,
SDK runtimes, and Flutter plugins.

## Packaged notices

The Flutter asset `flutter/assets/LEGAL_NOTICES.txt` contains the project notice,
full AGPL text, and the direct transport/enigo license texts listed above.
It is included by the existing `assets/` bundle entry; Debian packages also
install it as `/usr/share/doc/sealdesk/copyright`. The Windows MSI license panel
uses the same text in RTF form. Refresh these copies when the source notices
change. Flutter's dependency license registry remains separately maintained.

For a binary release, retain the component notices for the dependencies actually
shipped and make the corresponding source revision and build inputs available
alongside that release. Adding this file does not replace that release check.

## Artwork

SealDesk's seal-and-cat assets and regeneration instructions are recorded in
[res/SEALDESK-ICONS.md](res/SEALDESK-ICONS.md). Original RustDesk artwork and marks
are upstream material, not SealDesk's own identity.
