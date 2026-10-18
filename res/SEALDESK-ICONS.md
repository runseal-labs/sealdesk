# SealDesk approved raster icons

The approved artwork is the original front-facing seal hugging a mint cat with whiskers and a gold lock. No vector redraw or generative reinterpretation is used.

- `res/sealdesk-icon-approved-sheet.png`: original approved presentation sheet.
- `res/sealdesk-icon-approved.png`: transparent square master, with original artwork proportions retained. The artwork crop is `(158, 80, 848, 742)`, 690 x 662 pixels, centered on a 722 x 722 transparent canvas. Only background alpha edges are cleaned; interior RGB pixels are unchanged.
- `res/sealdesk-icon-monochrome.png`: luminance-derived platform template.
- `res/sealdesk-icon.svg` and other existing SVG names: compatibility wrappers embedding the approved PNG, not vector artwork.

Regenerate all platform assets with Pillow and NumPy:

```sh
uv run --with pillow --with numpy res/generate_sealdesk_icons.py
```

To reproduce the master from the approved sheet first:

```sh
uv run --with pillow res/extract_sealdesk_icon.py
```

All color sizes use Lanczos resizing of the same master, preserving the original face, whiskers, lock and shading. 1024 px exports are upscaled from the original raster and do not add image detail. Windows ICO and macOS ICNS include multiple native frame sizes. iOS and Android store icons use an opaque matching teal matte. Android adaptive foregrounds fit the original tile in the safe zone; monochrome/notification and macOS template images derive from the approved artwork. `tmp/sealdesk-final-preview.png` shows actual export sizes.

Remote desktop connection logic is unchanged. The Linux window icon lookup and title use SealDesk branding; rebuild the application to pick up updated resources.

Linux icon theme entries use `com.runseal.sealdesk` consistently in desktop launchers, the GTK runner and packaging recipes, avoiding the previously cached generic icon name.

Linux theme PNGs and the scalable compatibility image omit the extra outer transparent padding, retaining the artwork proportions while increasing its Dock footprint by about 4 percent. Other platforms retain their existing export layout.
