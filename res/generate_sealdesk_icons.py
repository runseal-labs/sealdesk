#!/usr/bin/env python3
"""Generate icons from the approved raster master.

Run: uv run --with pillow --with numpy res/generate_sealdesk_icons.py

The SVG compatibility files embed this PNG; they are not vector redraws.
Edge RGB on the master is decontaminated in place. Interior pixels stay
bit-identical and alpha is not modified. Nothing here redraws the seal.

Exports are Lanczos scales of that cleaned 722 master (or a translated
copy of the monochrome glyph). Slots are not upscaled past 722 unless the
platform requires that exact size:
  * iOS App Store marketing icon, 1024
  * macOS ICNS ic10 (512 pt @2x), 1024
Those two are upscales and add no detail. Tray files are not written:
res/tray-icon.ico, res/mac-tray-dark-x2.png, res/mac-tray-light-x2.png.
"""
from pathlib import Path
import base64
import io
import json
import struct
import sys

import numpy as np
from PIL import Image, ImageDraw

BASE = Path(__file__).resolve().parents[1]
SOURCE = BASE / 'res/sealdesk-icon-approved.png'
MONO_SOURCE = BASE / 'res/sealdesk-icon-monochrome.png'
BRAND = '#07516B'

# Filled by prepare().
_MASTER = None
_MONO = None


def neighbors8(mask):
    pad = np.pad(mask, 1, constant_values=False)
    out = np.zeros_like(mask)
    h, w = mask.shape
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            if dx == 0 and dy == 0:
                continue
            out |= pad[1 + dy:1 + dy + h, 1 + dx:1 + dx + w]
    return out


def fringe_flags(rgb):
    near_white = rgb.min(axis=2) >= 220
    near_black = rgb.max(axis=2) <= 28
    return near_white, near_black


def decontaminate(im):
    """Edge-only RGB cleanup from the icon-fix reference.

    Edge pixels are 0 < alpha < 255 plus the 1px opaque ring next to
    transparency. Contaminated edge RGB is replaced by the nearest fully
    opaque interior color (iterative 8-neighbor dilation). Alpha is unchanged.
    """
    rgb = im[:, :, :3].astype(np.float64)
    alpha = im[:, :, 3].astype(np.uint8)
    opaque = alpha == 255
    semi = (alpha > 0) & (alpha < 255)
    transparent = alpha == 0
    edge_opaque = opaque & neighbors8(transparent | semi)
    interior = opaque & ~neighbors8(~opaque)
    edge = semi | edge_opaque

    nw, nb = fringe_flags(im[:, :, :3])
    stats = {
        'semi_near_white_before': int((semi & nw).sum()),
        'semi_near_black_before': int((semi & nb).sum()),
        'edge_pixel_count': int(edge.sum()),
        'semi_count': int(semi.sum()),
        'edge_opaque_count': int(edge_opaque.sum()),
        'interior_count': int(interior.sum()),
    }

    known = interior.copy()
    color = np.zeros_like(rgb)
    color[known] = rgb[known]
    h, w = alpha.shape
    for _ in range(12):
        if known.all():
            break
        pad_k = np.pad(known, 1, constant_values=False)
        acc = np.zeros((h, w, 3), dtype=np.float64)
        cnt = np.zeros((h, w), dtype=np.float64)
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if dx == 0 and dy == 0:
                    continue
                k = pad_k[1 + dy:1 + dy + h, 1 + dx:1 + dx + w]
                c = np.pad(color, ((1, 1), (1, 1), (0, 0)), constant_values=0)[
                    1 + dy:1 + dy + h, 1 + dx:1 + dx + w
                ]
                acc += c * k[:, :, None]
                cnt += k
        newly = (~known) & edge & (cnt > 0)
        if not newly.any():
            break
        color[newly] = acc[newly] / cnt[newly, None]
        known[newly] = True

    out = im.copy()
    filled = edge & known
    prop = np.clip(np.rint(color), 0, 255).astype(np.uint8)
    src_rgb = im[:, :, :3]
    delta = np.abs(src_rgb.astype(np.int16) - prop.astype(np.int16)).max(axis=2)
    contaminated = nw | nb | (delta >= 18)
    replace = filled & contaminated
    out[:, :, :3][replace] = prop[replace]

    unique_a = np.unique(alpha)
    stair = set(unique_a.tolist()) <= {0, 255}
    stats['binary_stair_step'] = bool(stair)
    stats['alpha_modified'] = False
    stats['stair_step_smoothed'] = False

    nw2, nb2 = fringe_flags(out[:, :, :3])
    semi_after = (out[:, :, 3] > 0) & (out[:, :, 3] < 255)
    stats['semi_near_white_after'] = int((semi_after & nw2).sum())
    stats['semi_near_black_after'] = int((semi_after & nb2).sum())
    changed = np.any(out != im, axis=2)
    stats['edge_pixels_changed'] = int((changed & edge).sum())
    stats['non_edge_pixels_changed'] = int((changed & ~edge).sum())
    stats['pixels_changed'] = int(changed.sum())
    return out, stats


def interior_proof(src, dst):
    alpha = src[:, :, 3]
    opaque = alpha == 255
    interior = opaque & ~neighbors8(~opaque)
    diff = np.abs(src.astype(np.int16) - dst.astype(np.int16))
    interior_diff = diff[interior]
    changed = np.any(diff > 0, axis=2) & interior
    return {
        'interior_pixels': int(interior.sum()),
        'interior_pixels_changed': int(changed.sum()),
        'interior_max_abs_diff': int(interior_diff.max()) if interior.any() else 0,
        'alpha_pixels_changed': int(np.any(src[:, :, 3] != dst[:, :, 3])),
    }


def translate(arr, dx, dy):
    """Integer shift. Vacated pixels become transparent. No resample."""
    out = np.zeros_like(arr)
    h, w = arr.shape[:2]
    if abs(dx) >= w or abs(dy) >= h:
        raise SystemExit(f'mono shift {(dx, dy)} moves the glyph off the canvas')
    src_x0 = max(0, -dx)
    src_y0 = max(0, -dy)
    dst_x0 = max(0, dx)
    dst_y0 = max(0, dy)
    width = w - abs(dx)
    height = h - abs(dy)
    out[dst_y0:dst_y0 + height, dst_x0:dst_x0 + width] = arr[src_y0:src_y0 + height, src_x0:src_x0 + width]
    return out


def recenter_shift(alpha):
    ys, xs = np.where(alpha > 0)
    if len(xs) == 0:
        return 0, 0
    cx = (xs.min() + xs.max()) / 2.0
    cy = (ys.min() + ys.max()) / 2.0
    target_x = (alpha.shape[1] - 1) / 2.0
    target_y = (alpha.shape[0] - 1) / 2.0
    return int(round(target_x - cx)), int(round(target_y - cy))


def prepare():
    """Clean the color master and recenter the mono glyph. Idempotent."""
    global _MASTER, _MONO
    raw = np.array(Image.open(SOURCE).convert('RGBA'))
    cleaned, stats = decontaminate(raw)
    proof = interior_proof(raw, cleaned)
    if proof['interior_pixels_changed'] or proof['alpha_pixels_changed'] or stats['non_edge_pixels_changed']:
        raise SystemExit(f'master cleanup touched interior or alpha: {proof} {stats}')
    before = stats['semi_near_white_before'] + stats['semi_near_black_before']
    after = stats['semi_near_white_after'] + stats['semi_near_black_after']
    if before and after > before * 0.1:
        raise SystemExit(f'fringe reduction {before} -> {after} is under 90%')
    if not np.array_equal(cleaned, raw):
        Image.fromarray(cleaned, 'RGBA').save(SOURCE)
    _MASTER = Image.fromarray(cleaned, 'RGBA')

    mono = np.array(Image.open(MONO_SOURCE).convert('RGBA'))
    dx, dy = recenter_shift(mono[:, :, 3])
    moved = translate(mono, dx, dy)
    # Visible glyph only: transparent padding may fall off the canvas.
    ys, xs = np.where(mono[:, :, 3] > 0)
    h, w = mono.shape[:2]
    if ys.size and (ys.min() + dy < 0 or xs.min() + dx < 0 or ys.max() + dy >= h or xs.max() + dx >= w):
        raise SystemExit(f'mono recenter {(dx, dy)} clips the glyph')
    if ys.size and not np.array_equal(moved[ys + dy, xs + dx], mono[ys, xs]):
        raise SystemExit('mono recenter changed glyph pixels')
    if (dx, dy) != (0, 0):
        Image.fromarray(moved, 'RGBA').save(MONO_SOURCE)
    _MONO = Image.fromarray(moved, 'RGBA')
    stats.update(proof)
    stats['mono_shift'] = [dx, dy]
    return stats


def master():
    if _MASTER is None:
        raise SystemExit('prepare() was not called')
    return _MASTER


def mono_image():
    if _MONO is None:
        raise SystemExit('prepare() was not called')
    return _MONO


def render(size, kind='tile', allow_upscale=False):
    image = master()
    if size > image.width and not allow_upscale:
        raise SystemExit(f'refusing to upscale the {image.width}px master to {size}')
    if size != image.width:
        image = image.resize((size, size), Image.Resampling.LANCZOS)
    if kind == 'full':
        background = Image.new('RGBA', image.size, BRAND)
        background.alpha_composite(image)
        return background
    return image


def linux_square():
    """Tight square of the tile. Left and right of this square are flush."""
    image = master()
    image = image.crop(image.getchannel('A').getbbox())
    side = max(image.size)
    out = Image.new('RGBA', (side, side))
    out.alpha_composite(image, ((side - image.width) // 2, (side - image.height) // 2))
    return out


def with_margin(image, margin):
    out = Image.new('RGBA', (image.width + 2 * margin, image.height + 2 * margin))
    out.alpha_composite(image, (margin, margin))
    return out


def linux_icon(n, margin=1):
    """Downscale the squared tile, then pad so every edge has `margin` clear px."""
    if n <= 2 * margin:
        raise SystemExit(f'linux size {n} cannot keep a {margin}px margin')
    square = linux_square()
    inner = n - 2 * margin
    if inner > square.width:
        raise SystemExit(f'refusing to upscale the linux tile to {inner}')
    resized = square if inner == square.width else square.resize((inner, inner), Image.Resampling.LANCZOS)
    return with_margin(resized, margin)


def full_bleed(image):
    alpha = np.array(image.getchannel('A'))
    ys, xs = np.where(alpha > 0)
    if len(xs) == 0:
        return False
    h, w = alpha.shape
    return xs.min() == 0 and ys.min() == 0 and xs.max() == w - 1 and ys.max() == h - 1


def adaptive_foreground(canvas):
    """b1: scale the whole cleaned icon so its rounded tile covers the canvas.

    Uniform Lanczos only, then a center crop. The tile is wider than it is
    tall, so a few percent of the plate's left and right is what the crop
    drops; the seal stays inside. Launcher masks do the only further crop.
    """
    image = master()
    bbox = image.getchannel('A').getbbox()
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    for extra in (2, 4, 6, 8, 12):
        scale = max((canvas + extra) / tw, (canvas + extra) / th)
        sw = max(canvas, round(image.width * scale))
        sh = max(canvas, round(image.height * scale))
        if sw > image.width or sh > image.height:
            raise SystemExit(f'foreground {canvas} would upscale the master')
        scaled = image.resize((sw, sh), Image.Resampling.LANCZOS)
        left = bbox[0] * sw / image.width
        top = bbox[1] * sh / image.height
        tile_w = tw * sw / image.width
        tile_h = th * sh / image.height
        crop_x = int(round(left + (tile_w - canvas) / 2))
        crop_y = int(round(top + (tile_h - canvas) / 2))
        crop_x = max(0, min(crop_x, sw - canvas))
        crop_y = max(0, min(crop_y, sh - canvas))
        out = scaled.crop((crop_x, crop_y, crop_x + canvas, crop_y + canvas))
        if full_bleed(out):
            return out
    raise SystemExit(f'b1 foreground {canvas} still has a transparent margin')


def svg_wrap(image):
    data = io.BytesIO()
    image.save(data, format='PNG')
    encoded = base64.b64encode(data.getvalue()).decode()
    w, h = image.size
    # ViewBox matches the embedded pixels so the wrapper does not upscale.
    return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
            f'<image width="{w}" height="{h}" xlink:href="data:image/png;base64,{encoded}"/>'
            '</svg>\n').encode()


def mono(size, color='white'):
    source = mono_image()
    if size > source.width:
        raise SystemExit(f'refusing to upscale the mono glyph to {size}')
    source = source if size == source.width else source.resize((size, size), Image.Resampling.LANCZOS)
    out = Image.new('RGBA', source.size, color)
    out.putalpha(source.getchannel('A'))
    return out


def transparent_edges(image):
    alpha = np.array(image.getchannel('A'))
    return (int(alpha[0].max()) == 0 and int(alpha[-1].max()) == 0
            and int(alpha[:, 0].max()) == 0 and int(alpha[:, -1].max()) == 0)


def put(name, data):
    p = BASE / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(data)


def png_bytes(im):
    b = io.BytesIO()
    im.save(b, format='PNG')
    return b.getvalue()


def png(name, im):
    put(name, png_bytes(im))


def ico(name, sizes):
    frames = [png_bytes(render(n)) for n in sizes]
    offset = 6 + 16 * len(sizes)
    data = struct.pack('<HHH', 0, 1, len(sizes))
    for n, frame in zip(sizes, frames):
        data += struct.pack('<BBBBHHII', n % 256, n % 256, 0, 0, 1, 32, len(frame), offset)
        offset += len(frame)
    put(name, data + b''.join(frames))


def main():
    stats = prepare()
    native = master().width
    for name in ['res/logo.svg', 'res/logo-header.svg', 'flutter/assets/icon.svg', 'res/sealdesk-icon.svg']:
        put(name, svg_wrap(master()))
    # Native squared tile plus one clear pixel. Not an upscale.
    put('res/scalable.svg', svg_wrap(with_margin(linux_square(), 1)))
    put('res/sealdesk-icon-small.svg', svg_wrap(render(32)))
    for name in ['res/icon.png', 'res/mac-icon.png', 'flutter/assets/icon.png']:
        png(name, render(native))
    for label, n in [('32x32', 32), ('64x64', 64), ('128x128', 128), ('128x128@2x', 256)]:
        icon = linux_icon(n)
        if not transparent_edges(icon):
            raise SystemExit(f'{label} is missing a transparent edge')
        png('res/' + label + '.png', icon)
    # Tray assets stay as they are (Windows ICO and macOS menu-bar templates).
    for name in ['res/icon.ico', 'flutter/windows/runner/resources/app_icon.ico']:
        ico(name, [16, 24, 32, 48, 64, 128, 256])
    chunks = []
    upscaled = []
    for tag, n in [('icp4', 16), ('icp5', 32), ('icp6', 64), ('ic07', 128),
                   ('ic08', 256), ('ic09', 512), ('ic10', 1024),
                   ('ic11', 32), ('ic12', 64), ('ic13', 256), ('ic14', 512)]:
        # ic10 is the required 512@2x macOS slot. It is an upscale of 722.
        allow = n > native
        if allow:
            upscaled.append(f'flutter/macos/Runner/AppIcon.icns:{tag}:{n}')
        data = png_bytes(render(n, allow_upscale=allow))
        chunks.append(tag.encode() + struct.pack('>I', len(data) + 8) + data)
    body = b''.join(chunks)
    put('flutter/macos/Runner/AppIcon.icns', b'icns' + struct.pack('>I', len(body) + 8) + body)
    root = 'flutter/ios/Runner/Assets.xcassets/AppIcon.appiconset/'
    for item in json.loads((BASE / root / 'Contents.json').read_text())['images']:
        n = round(float(item['size'].split('x')[0]) * float(item['scale'][:-1]))
        allow = n > native
        if allow:
            upscaled.append(root + item['filename'])
        png(root + item['filename'], render(n, 'full', allow_upscale=allow).convert('RGB'))
    android = 'flutter/android/app/src/main/res/'
    for density, n, f, stat in [('mdpi', 48, 108, 24), ('hdpi', 72, 162, 36),
                               ('xhdpi', 96, 216, 48), ('xxhdpi', 144, 324, 72),
                               ('xxxhdpi', 192, 432, 96)]:
        root = android + 'mipmap-' + density + '/'
        png(root + 'ic_launcher.png', render(n, 'full'))
        im = render(n, 'full')
        mask = Image.new('L', (n * 4, n * 4))
        ImageDraw.Draw(mask).ellipse((0, 0, n * 4 - 1, n * 4 - 1), fill=255)
        im.putalpha(mask.resize((n, n), Image.Resampling.LANCZOS))
        png(root + 'ic_launcher_round.png', im)
        fg = adaptive_foreground(f)
        if not full_bleed(fg):
            raise SystemExit(f'{density} foreground is not full-bleed')
        png(root + 'ic_launcher_foreground.png', fg)
        png(root + 'ic_stat_logo.png', mono(stat))
        # Same inset as before. Only the glyph's position inside the master changed.
        k = round(f * 0.58)
        mf = Image.new('RGBA', (f, f))
        mi = mono(k)
        mf.alpha_composite(mi, ((f - k) // 2, (f - k) // 2))
        png(root + 'ic_launcher_monochrome.png', mf)
    put(android + 'values/ic_launcher_background.xml', b'<?xml version="1.0" encoding="utf-8"?>\n<resources>\n    <color name="ic_launcher_background">#07516B</color>\n</resources>\n')
    for name in ['ic_launcher.xml', 'ic_launcher_round.xml']:
        put(android + 'mipmap-anydpi-v26/' + name, (
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
            '    <background android:drawable="@color/ic_launcher_background"/>\n'
            '    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>\n'
            '    <monochrome android:drawable="@mipmap/ic_launcher_monochrome"/>\n'
            '</adaptive-icon>\n').encode())
    png('fastlane/metadata/android/en-US/images/icon.png', render(512, 'full').convert('RGB'))
    preview = Image.new('RGB', (1120, 650), '#e9eae7')
    im = render(500)
    preview.paste(im, (20, 35), im)
    draw = ImageDraw.Draw(preview)
    for i, n in enumerate([128, 64, 48, 32, 24, 16]):
        x, y = 580 + i % 3 * 170, 80 + i // 3 * 220
        im = render(n)
        preview.paste(im, (x, y), im)
        draw.text((x, y + n + 14), str(n) + ' px', fill='#143A49')
    for i, color in enumerate(['white', 'black']):
        x = 610 + i * 150
        draw.rectangle((x - 8, 540, x + 72, 620), fill='#183E50' if i == 0 else '#ffffff')
        im = mono(64, color)
        preview.paste(im, (x, 548), im)
    png('tmp/sealdesk-final-preview.png', preview)
    if not transparent_edges(linux_icon(32)) or not transparent_edges(with_margin(linux_square(), 1)):
        raise SystemExit('linux margin check failed')
    print(json.dumps({
        'master': {k: stats[k] for k in stats if k != 'mono_shift'},
        'mono_shift': stats['mono_shift'],
        'upscaled': upscaled,
        'master_px': native,
    }, indent=2))
    print('SealDesk icons generated from cleaned raster master')


if __name__ == '__main__':
    sys.exit(main() or 0)
