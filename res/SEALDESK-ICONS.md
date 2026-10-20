# SealDesk icons

The SealDesk icon is a front-facing white seal hugging a mint cat that wears a cord collar with a gold padlock, on a rounded teal plate.

- `res/sealdesk-icon.svg`: the vector master and single source of truth. Its `plate` group is the rounded teal plate; its `subject` group is the seal, the cat and the lock. It is traced from the original approved raster artwork.
- `res/sealdesk-icon-approved.png` and `res/sealdesk-icon-approved-sheet.png`: the original approved raster artwork, kept as the reference the vector was traced from. No asset is built from them any more.
- Every other icon file is generated. Do not edit them by hand.

Regenerate all platform assets:

```sh
uv run --with resvg-py==0.5.0 --with pillow --with numpy res/generate_sealdesk_icons.py
```

Every export is rasterized from the SVG at its exact pixel size with resvg, so no slot is an upscale and every edge is clean antialiasing. The generator derives these variants from the master:

- Tile (the master as drawn, plate with a small transparent margin): Windows `res/icon.ico`, `flutter/windows/runner/resources/app_icon.ico` and `res/tray-icon.ico` (16 to 256 px), macOS `AppIcon.icns` (16 to 1024 px), `res/icon.png`, `res/mac-icon.png`, `flutter/assets/icon.png`, and the legacy Android `ic_launcher`.
- Linux theme PNGs at 32, 64, 128 and 256 px and `res/scalable.svg` crop the plate tight, square it and keep a 1px transparent margin. Linux icon theme entries use `com.runseal.sealdesk`.
- Full-bleed (plate made square and edge to edge; the OS applies the corner mask): iOS `AppIcon.appiconset` and the fastlane store icon, both opaque.
- Android adaptive icons: the background layer is the full-bleed plate; the foreground layer is the whole seal and cat, scaled so its farthest pixel is 39dp from the center of the 108dp canvas. Rounded-square and squircle launcher masks show all of it; a circle mask only trims the bottom edge of the flippers. `ic_launcher_round` is the visible 72dp of that composite under a circle mask.
- Monochrome (subject in white with the faces, the cat outline, the flipper outlines and the lock punched out): `res/sealdesk-icon-monochrome.png`, the Android themed `ic_launcher_monochrome` layer (same geometry as the foreground), the Android notification `ic_stat_logo`, and the macOS menu bar templates `res/mac-tray-dark-x2.png` (white) and `res/mac-tray-light-x2.png` (black).

`res/logo.svg`, `res/logo-header.svg`, `flutter/assets/icon.svg`, `res/scalable.svg` and `res/sealdesk-icon-small.svg` wrap a PNG render instead of copying the vector, because flutter_svg and older librsvg do not draw SVG filters, and an element whose filter is unsupported is not drawn at all. `tmp/sealdesk-final-preview.png` shows actual export sizes.

Remote desktop connection logic is unchanged. Rebuild the application to pick up updated resources.
