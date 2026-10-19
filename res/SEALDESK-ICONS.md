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

All color sizes use Lanczos resizing of the same master, preserving the original face, whiskers, lock and shading. 1024 px exports are upscaled from the original raster and do not add image detail. Windows ICO and macOS ICNS include multiple native frame sizes. iOS and Android store icons use an opaque matching teal matte. Android adaptive foregrounds keep only the seal and cat: the rounded-square plate is cleared to transparency (including its pale rim against the fur) and the seal and cat RGB are not redrawn. The plate's teal gradient is a full-bleed background image. The subject is a uniform scale about 72dp wide. Its wide lower body is seated on the bottom edge of the 108dp canvas, so the seal reads as emerging from the mask; the tapered point below that row is cropped and no pixels are invented. The lower body may also be clipped by the launcher mask. Legacy `ic_launcher` / `ic_launcher_round` are the same composite scaled into the 48dp slot. After scaling, isolated alpha leftovers from the plate cut (specks beside the arms) are cleared without rewriting subject RGB. The monochrome layer is that silhouette with dark features — eyes, nose, cat and arm edges — punched out, and fragments smaller than the eyes removed. Monochrome/notification and macOS template images derive from the approved artwork. `tmp/sealdesk-final-preview.png` shows actual export sizes.

Remote desktop connection logic is unchanged. The Linux window icon lookup and title use SealDesk branding; rebuild the application to pick up updated resources.

Linux icon theme entries use `com.runseal.sealdesk` consistently in desktop launchers, the GTK runner and packaging recipes, avoiding the previously cached generic icon name.

Linux theme PNGs at 32, 64, 128 and 256 px each keep a 1px transparent margin, and the scalable compatibility image does the same. Other platforms retain their existing export layout.
