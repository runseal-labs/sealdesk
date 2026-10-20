#!/usr/bin/env python3
"""Generate every SealDesk icon from the vector master res/sealdesk-icon.svg.

Run: uv run --with resvg-py==0.5.0 --with pillow --with numpy res/generate_sealdesk_icons.py

Each export is rasterized from the SVG at its exact pixel size, so no slot is
an upscale and edges are clean antialiasing. The master draws the rounded
plate (id="plate") and the seal hugging the cat (id="subject"); every variant
below is derived from those two groups:

  tile        the master as drawn: plate with a small transparent margin
              (desktop: Windows ICO, macOS ICNS, Linux, Flutter assets, tray)
  full        plate made full-bleed and square (iOS, fastlane)
  background  full-bleed plate only (Android adaptive background)
  foreground  subject only, scaled so it reaches 39dp from the center of the
              108dp adaptive canvas: whole under rounded-square masks, only
              the flipper bottoms touch a circle mask (Android foreground)
  mono        subject silhouette with the faces, cat outline, flippers and
              lock punched out (Android monochrome/notification, macOS tray)

The *.svg files other than the master wrap a PNG render, because flutter_svg
and older librsvg do not draw SVG filters.
"""
from pathlib import Path
import base64
import io
import json
import struct
import sys
import xml.etree.ElementTree as ET

import numpy as np
import resvg_py
from PIL import Image, ImageDraw

BASE = Path(__file__).resolve().parents[1]
MASTER = BASE / 'res/sealdesk-icon.svg'
SVG_NS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', SVG_NS)
CANVAS = 722  # master viewBox side

ADAPTIVE_DP = 108.0
SUBJECT_RADIUS_DP = 39.0
DARK_FILLS = {'#0F1E28', '#6E4A10', 'url(#gold)'}
OUTLINED_FILLS = {'url(#mint)', 'url(#mintBody)', 'url(#paw)'}


def master():
    return ET.parse(MASTER).getroot()


def by_id(root, ident):
    for el in root.iter():
        if el.get('id') == ident:
            return el
    raise SystemExit(f'master has no element id="{ident}"')


def parent_of(root, el):
    for parent in root.iter():
        if el in list(parent):
            return parent
    raise SystemExit('element not in tree')


def text(root):
    return ET.tostring(root, encoding='unicode')


def raster(svg, size):
    data = bytes(resvg_py.svg_to_bytes(svg_string=svg, width=size, height=size))
    image = Image.open(io.BytesIO(data)).convert('RGBA')
    if image.size != (size, size):
        raise SystemExit(f'resvg returned {image.size} for {size}')
    return image


def full_bleed(root):
    """Square, edge-to-edge plate; the iOS / launcher mask does the rounding."""
    square = {'x': '0', 'y': '0', 'width': str(CANVAS), 'height': str(CANVAS), 'rx': '0'}
    plate = by_id(root, 'plate')
    rects = [el for el in plate if el.tag.endswith('rect')]
    rects[0].attrib.update(square)
    for bevel in rects[1:]:
        plate.remove(bevel)
    for el in by_id(root, 'plateClip'):
        el.attrib.update(square)
    return root


def tile_svg():
    return text(master())


def full_svg():
    return text(full_bleed(master()))


def background_svg():
    root = full_bleed(master())
    root.remove(by_id(root, 'subject'))
    return text(root)


def subject_root():
    root = master()
    root.remove(by_id(root, 'plate'))
    by_id(root, 'subject').attrib.pop('clip-path', None)
    return root


def mono_root():
    """Subject in white with features, part outlines and the lock in black."""
    root = subject_root()
    for el in list(by_id(root, 'subject').iter()):
        el.attrib.pop('filter', None)
        if el.get('opacity') or el.get('stroke-opacity'):
            el.set('opacity', '0')  # highlights carry no shape
            continue
        fill, stroke = el.get('fill'), el.get('stroke')
        if fill and fill != 'none':
            el.set('fill', '#000000' if fill in DARK_FILLS else '#FFFFFF')
            if fill in OUTLINED_FILLS:
                el.set('stroke', '#000000')
                el.set('stroke-width', '12')
        if stroke and stroke != 'none':
            el.set('stroke', '#000000')
            if el.tag.endswith('g') and el.get('stroke-width') == '3':
                el.set('stroke-width', '12')  # flipper group outline
    return root


def mono_glyph(svg, size):
    """White glyph: alpha keeps the white parts of the mono render only."""
    arr = np.array(raster(svg, size)).astype(np.float32)
    lum = arr[:, :, :3].mean(axis=2) / 255.0
    alpha = arr[:, :, 3] / 255.0 * np.clip((lum - 0.25) / 0.5, 0, 1)
    out = np.zeros(arr.shape, dtype=np.uint8)
    out[:, :, :3] = 255
    out[:, :, 3] = np.clip(alpha * 255 + 0.5, 0, 255).astype(np.uint8)
    return Image.fromarray(out, 'RGBA')


def subject_fit():
    """Scale and center that put the subject within SUBJECT_RADIUS_DP."""
    alpha = np.array(raster(text(subject_root()), CANVAS).getchannel('A'))
    ys, xs = np.where(alpha > 40)
    cx = (xs.min() + xs.max() + 1) / 2.0
    cy = (ys.min() + ys.max() + 1) / 2.0
    radius = float(np.sqrt((xs + 0.5 - cx) ** 2 + (ys + 0.5 - cy) ** 2).max())
    return SUBJECT_RADIUS_DP / ADAPTIVE_DP * CANVAS / radius, cx, cy


def fitted(root, fit):
    scale, cx, cy = fit
    mid = CANVAS / 2.0
    subject = by_id(root, 'subject')
    parent = parent_of(root, subject)
    index = list(parent).index(subject)
    parent.remove(subject)
    group = ET.Element(f'{{{SVG_NS}}}g', {
        'transform': f'translate({mid:.3f} {mid:.3f}) scale({scale:.5f}) translate({-cx:.3f} {-cy:.3f})'})
    group.append(subject)
    parent.insert(index, group)
    return root


def glyph_fit(glyph, size, margin):
    """Crop a glyph to its ink and center it in `size` with `margin` px clear."""
    ink = glyph.crop(glyph.getchannel('A').point(lambda a: 255 if a > 8 else 0).getbbox())
    scale = (size - 2 * margin) / max(ink.size)
    ink = ink.resize((max(1, round(ink.width * scale)), max(1, round(ink.height * scale))), Image.Resampling.LANCZOS)
    out = Image.new('RGBA', (size, size))
    out.alpha_composite(ink, ((size - ink.width) // 2, (size - ink.height) // 2))
    return out


def tinted(glyph, color):
    out = Image.new('RGBA', glyph.size, color)
    out.putalpha(glyph.getchannel('A'))
    return out


def linux_icon(n, margin=1):
    """Plate cropped tight and squared, scaled to n - 2*margin, then padded."""
    big = raster(tile_svg(), CANVAS * 2)
    big = big.crop(big.getchannel('A').getbbox())
    side = max(big.size)
    square = Image.new('RGBA', (side, side))
    square.alpha_composite(big, ((side - big.width) // 2, (side - big.height) // 2))
    out = Image.new('RGBA', (n, n))
    out.alpha_composite(square.resize((n - 2 * margin,) * 2, Image.Resampling.LANCZOS), (margin, margin))
    return out


def transparent_edges(image):
    a = np.array(image.getchannel('A'))
    return not (a[0].any() or a[-1].any() or a[:, 0].any() or a[:, -1].any())


def svg_wrap(image):
    data = io.BytesIO()
    image.save(data, format='PNG', optimize=True)
    encoded = base64.b64encode(data.getvalue()).decode()
    w, h = image.size
    return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
            f'<image width="{w}" height="{h}" xlink:href="data:image/png;base64,{encoded}"/>'
            '</svg>\n').encode()


def put(name, data):
    p = BASE / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(data)


def png_bytes(im):
    b = io.BytesIO()
    im.save(b, format='PNG', optimize=True)
    return b.getvalue()


def png(name, im):
    put(name, png_bytes(im))


def ico(name, svg, sizes):
    frames = [png_bytes(raster(svg, n)) for n in sizes]
    offset = 6 + 16 * len(sizes)
    data = struct.pack('<HHH', 0, 1, len(sizes))
    for n, frame in zip(sizes, frames):
        data += struct.pack('<BBBBHHII', n % 256, n % 256, 0, 0, 1, 32, len(frame), offset)
        offset += len(frame)
    put(name, data + b''.join(frames))


def round_mask(image):
    n = image.width
    mask = Image.new('L', (n * 4, n * 4))
    ImageDraw.Draw(mask).ellipse((0, 0, n * 4 - 1, n * 4 - 1), fill=255)
    out = image.copy()
    out.putalpha(mask.resize((n, n), Image.Resampling.LANCZOS))
    return out


def main():
    tile, full, background = tile_svg(), full_svg(), background_svg()
    fit = subject_fit()
    foreground = text(fitted(subject_root(), fit))
    mono_foreground = text(fitted(mono_root(), fit))

    master_png = raster(tile, CANVAS)
    for name in ['res/logo.svg', 'res/logo-header.svg', 'flutter/assets/icon.svg']:
        put(name, svg_wrap(master_png))
    put('res/scalable.svg', svg_wrap(linux_icon(CANVAS)))
    put('res/sealdesk-icon-small.svg', svg_wrap(raster(tile, 32)))
    for name in ['res/icon.png', 'res/mac-icon.png', 'flutter/assets/icon.png']:
        png(name, master_png)

    for label, n in [('32x32', 32), ('64x64', 64), ('128x128', 128), ('128x128@2x', 256)]:
        icon = linux_icon(n)
        if not transparent_edges(icon):
            raise SystemExit(f'{label} is missing a transparent edge')
        png('res/' + label + '.png', icon)

    for name in ['res/icon.ico', 'flutter/windows/runner/resources/app_icon.ico', 'res/tray-icon.ico']:
        ico(name, tile, [16, 24, 32, 48, 64, 128, 256])

    chunks = []
    for tag, n in [('icp4', 16), ('icp5', 32), ('icp6', 64), ('ic07', 128),
                   ('ic08', 256), ('ic09', 512), ('ic10', 1024),
                   ('ic11', 32), ('ic12', 64), ('ic13', 256), ('ic14', 512)]:
        data = png_bytes(raster(tile, n))
        chunks.append(tag.encode() + struct.pack('>I', len(data) + 8) + data)
    body = b''.join(chunks)
    put('flutter/macos/Runner/AppIcon.icns', b'icns' + struct.pack('>I', len(body) + 8) + body)

    glyph = mono_glyph(text(mono_root()), CANVAS * 2)
    png('res/sealdesk-icon-monochrome.png', glyph_fit(glyph, CANVAS, 24))
    png('res/mac-tray-dark-x2.png', tinted(glyph_fit(glyph, 60, 2), 'white'))
    png('res/mac-tray-light-x2.png', tinted(glyph_fit(glyph, 48, 2), 'black'))

    ios = 'flutter/ios/Runner/Assets.xcassets/AppIcon.appiconset/'
    for item in json.loads((BASE / ios / 'Contents.json').read_text())['images']:
        n = round(float(item['size'].split('x')[0]) * float(item['scale'][:-1]))
        png(ios + item['filename'], raster(full, n).convert('RGB'))

    android = 'flutter/android/app/src/main/res/'
    for density, n, f, stat in [('mdpi', 48, 108, 24), ('hdpi', 72, 162, 36),
                               ('xhdpi', 96, 216, 48), ('xxhdpi', 144, 324, 72),
                               ('xxxhdpi', 192, 432, 96)]:
        out = android + 'mipmap-' + density + '/'
        fg, bg = raster(foreground, f), raster(background, f)
        png(out + 'ic_launcher_foreground.png', fg)
        png(out + 'ic_launcher_background.png', bg)
        png(out + 'ic_launcher_monochrome.png', mono_glyph(mono_foreground, f))
        png(out + 'ic_launcher.png', raster(tile, n))
        composite = bg.copy()
        composite.alpha_composite(fg)
        inset = f * 18 // 108  # adaptive layers show only their centered 72dp
        visible = composite.crop((inset, inset, f - inset, f - inset))
        png(out + 'ic_launcher_round.png', round_mask(visible.resize((n, n), Image.Resampling.LANCZOS)))
        png(out + 'ic_stat_logo.png', glyph_fit(glyph, stat, max(1, stat // 12)))

    png('fastlane/metadata/android/en-US/images/icon.png', raster(full, 512).convert('RGB'))

    preview = Image.new('RGB', (1120, 650), '#e9eae7')
    im = raster(tile, 500)
    preview.paste(im, (20, 35), im)
    draw = ImageDraw.Draw(preview)
    for i, n in enumerate([128, 64, 48, 32, 24, 16]):
        x, y = 580 + i % 3 * 170, 80 + i // 3 * 220
        im = raster(tile, n)
        preview.paste(im, (x, y), im)
        draw.text((x, y + n + 14), str(n) + ' px', fill='#143A49')
    png('tmp/sealdesk-final-preview.png', preview)

    print(json.dumps({'adaptive_subject_scale': round(fit[0], 4),
                      'adaptive_subject_radius_dp': SUBJECT_RADIUS_DP}, indent=2))
    print('SealDesk icons generated from res/sealdesk-icon.svg')


if __name__ == '__main__':
    sys.exit(main() or 0)
