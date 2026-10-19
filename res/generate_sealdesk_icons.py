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


# Adaptive layers are 108dp. Launcher masks may cover only the inner 72dp,
# and the subject has to stay inside the centered 66dp-diameter circle.
_ADAPTIVE_CANVAS_DP = 108.0
_ADAPTIVE_SAFE_DIAMETER_DP = 66.0


def _dilate(mask):
    h, w = mask.shape
    pad = np.pad(mask, 1)
    out = np.zeros_like(mask)
    for dy in (-1, 0, 1):
        for dx in (-1, 0, 1):
            out |= pad[1 + dy:1 + dy + h, 1 + dx:1 + dx + w]
    return out


def _outside(free):
    """Pixels of `free` reachable from the canvas border (4-connected)."""
    h, w = free.shape
    seen = np.zeros((h, w), bool)
    stack = []
    for x in range(w):
        for y in (0, h - 1):
            if free[y, x] and not seen[y, x]:
                seen[y, x] = True
                stack.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if free[y, x] and not seen[y, x]:
                seen[y, x] = True
                stack.append((y, x))
    while stack:
        y, x = stack.pop()
        for ny, nx in ((y + 1, x), (y - 1, x), (y, x + 1), (y, x - 1)):
            if 0 <= ny < h and 0 <= nx < w and free[ny, nx] and not seen[ny, nx]:
                seen[ny, nx] = True
                stack.append((ny, nx))
    return seen


def plate_mask(arr):
    """Pixels of the rounded-square plate, not the seal or the cat.

    The plate is the teal region that touches the outside of the artwork.
    Subject pixels (white fur, features, mint cat, lock) are a different
    color and stop the flood. A pale one-to-three pixel rim where the plate
    meets the fur is included so the cut does not leave a light outline.
    """
    alpha = arr[:, :, 3]
    rgb = arr[:, :, :3].astype(np.int16)
    red, green, blue = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    h, w = alpha.shape
    plate_color = (
        (alpha > 0) & (red < 90) & (green > 25) & (blue > 25)
        & ((green + blue) > red + 40) & ((red + green + blue) < 500)
    )
    outside = _outside(alpha <= 16)
    seeds = _dilate(outside) & plate_color & ~outside
    if int(seeds.sum()) < 100:
        raise SystemExit('adaptive plate is not connected to the canvas edge')
    plate = np.zeros((h, w), bool)
    stack = [(int(y), int(x)) for y, x in zip(*np.where(seeds))]
    plate[seeds] = True
    while stack:
        y, x = stack.pop()
        for ny, nx in ((y + 1, x), (y - 1, x), (y, x + 1), (y, x - 1)):
            if 0 <= ny < h and 0 <= nx < w and not plate[ny, nx] and plate_color[ny, nx]:
                plate[ny, nx] = True
                stack.append((ny, nx))
    if int(plate.sum()) < 100000:
        raise SystemExit('adaptive plate body is missing')
    # Antialiased plate edge, plus the pale rim painted onto the fur.
    pale = (alpha > 0) & (rgb.min(axis=2) >= 205) & (red >= 180)
    rim = np.zeros_like(plate)
    cur = plate.copy()
    for _ in range(3):
        nxt = _dilate(cur) & pale & ~cur
        if not nxt.any():
            break
        rim |= nxt
        cur |= nxt
    aa = _dilate(plate) & ~plate & (alpha > 0) & (alpha < 250) & (red < 120)
    return plate | rim | aa


def subject_master():
    """Master with the rounded-square plate made transparent.

    Seal and cat RGB stay bit-identical. Only plate pixels, including the
    pale rim where the plate meets the fur, lose alpha.
    """
    arr = np.array(master())
    drop = plate_mask(arr)
    out = arr.copy()
    out[:, :, 3] = np.where(drop, 0, arr[:, :, 3])
    kept = ~drop & (arr[:, :, 3] > 0)
    if not np.array_equal(out[:, :, :3][kept], arr[:, :, :3][kept]):
        raise SystemExit('plate knockout changed subject RGB')
    return Image.fromarray(out, 'RGBA'), arr, drop


def plate_gradient_coef(arr, drop):
    """Plane fit of the plate RGB. Rows are dx, dy, offset per channel."""
    rgb = arr[:, :, :3].astype(np.float64)
    sample = drop & (arr[:, :, 3] >= 250) & (rgb[:, :, 0] < 90)
    ys, xs = np.where(sample)
    if len(xs) < 1000:
        raise SystemExit('not enough plate pixels to fit the background gradient')
    design = np.stack([xs.astype(np.float64), ys.astype(np.float64), np.ones(len(xs))], 1)
    return np.linalg.lstsq(design, rgb[ys, xs], rcond=None)[0]


def content_radius(image, thresh=128):
    """Max distance from the canvas center to a pixel at or above `thresh`."""
    alpha = np.array(image.getchannel('A'))
    ys, xs = np.where(alpha >= thresh)
    if len(xs) == 0:
        return 0.0
    cx = (image.width - 1) / 2.0
    cy = (image.height - 1) / 2.0
    return float(np.hypot(xs - cx, ys - cy).max())


# Nominal scale of the plate-free subject on the 108dp canvas. Opaque pixels
# measure a few dp under this, which lands the visible subject near 72dp.
# The lower body runs off the bottom edge and may be cut by the launcher mask.
_ADAPTIVE_SUBJECT_WIDTH_DP = 75.0


def _subject_layout(subject):
    """Opaque bounds and the body-bottom edge on the plate-free master."""
    alpha = np.array(subject.getchannel('A'))
    ys, xs = np.where(alpha >= 128)
    if len(xs) == 0:
        raise SystemExit('adaptive foreground subject is empty')
    # The belly narrows to a point at the artwork edge. Seat the last row
    # that is still a wide body, so the canvas bottom cuts the body the way
    # the original plate did. The point below that row is cropped, not redrawn.
    widths = np.zeros(alpha.shape[0], np.int32)
    for y in np.unique(ys):
        widths[y] = int((alpha[y] >= 128).sum())
    wide = np.where(widths >= int(widths.max() * 0.40))[0]
    seat = int(wide.max()) if len(wide) else int(ys.max())
    return {
        'x0': int(xs.min()),
        'x1': int(xs.max()),
        'y_edge': float(seat) + 1.0,
        'cx': (float(xs.min()) + float(xs.max())) / 2.0,
    }


def adaptive_foreground(canvas, subject):
    """Scale the plate-free subject to about 72dp and seat the wide belly on the bottom.

    Uniform Lanczos of the original pixels only. The tapered point below the
    wide belly is cropped by the canvas edge, so no pixels are invented.
    """
    lay = _subject_layout(subject)
    width = lay['x1'] - lay['x0'] + 1
    scale = (_ADAPTIVE_SUBJECT_WIDTH_DP / _ADAPTIVE_CANVAS_DP) * canvas / width
    sw = max(1, int(round(subject.width * scale)))
    sh = max(1, int(round(subject.height * scale)))
    scaled = subject.resize((sw, sh), Image.Resampling.LANCZOS)
    left = int(round(canvas / 2.0 - lay['cx'] * (sw / subject.width)))
    top = int(round(canvas - lay['y_edge'] * (sh / subject.height)))
    out = Image.new('RGBA', (canvas, canvas), (0, 0, 0, 0))
    if top >= 0 and left >= 0 and top + sh <= canvas and left + sw <= canvas:
        out.alpha_composite(scaled, (left, top))
    else:
        # The scaled subject is taller than the canvas. Crop before compositing;
        # alpha_composite drops a negative destination instead of clipping it.
        src_top = max(0, -top)
        src_left = max(0, -left)
        cropped = scaled.crop((
            src_left, src_top,
            min(sw, src_left + canvas - max(left, 0)),
            min(sh, src_top + canvas - max(top, 0))))
        out.alpha_composite(cropped, (max(left, 0), max(top, 0)))
    alpha = np.array(out.getchannel('A'))
    if not np.any(alpha >= 128):
        raise SystemExit(f'foreground {canvas} is empty')
    return out, 0


def _component_sizes(mask):
    """4-connected component id and size for True pixels. 0 is background."""
    h, w = mask.shape
    ids = np.zeros((h, w), np.int32)
    sizes = [0]
    cid = 0
    ys, xs = np.where(mask)
    for y, x in zip(ys, xs):
        if ids[y, x]:
            continue
        cid += 1
        stack = [(int(y), int(x))]
        ids[y, x] = cid
        n = 0
        while stack:
            cy, cx = stack.pop()
            n += 1
            for ny, nx in ((cy + 1, cx), (cy - 1, cx), (cy, cx + 1), (cy, cx - 1)):
                if 0 <= ny < h and 0 <= nx < w and mask[ny, nx] and ids[ny, nx] == 0:
                    ids[ny, nx] = cid
                    stack.append((ny, nx))
        sizes.append(n)
    return ids, sizes


def clear_stray_alpha(image):
    """Drop isolated knockout leftovers. Subject RGB is not rewritten.

    Lanczos of a hard plate cut leaves one-pixel alpha islands beside the
    arms, around one third of the way down. Anything that is not the main
    body, and any faint pixel with no opaque neighbor, is cleared.
    """
    arr = np.array(image)
    alpha = arr[:, :, 3]
    opaque = alpha >= 128
    ids, sizes = _component_sizes(opaque)
    if len(sizes) <= 1:
        return image
    keep = int(np.argmax(sizes))
    drop = opaque & (ids != keep)
    # Faint islands and fringe specks that do not touch an opaque pixel.
    faint = (alpha > 0) & ~opaque
    faint_ids, faint_sizes = _component_sizes(faint)
    touches = neighbors8(opaque) & faint
    touch_ids = set(int(i) for i in np.unique(faint_ids[touches]) if i)
    # The real antialiased rim is the large faint component on the fur.
    # Smaller faint blobs beside the arms are knockout leftovers.
    rim = 0
    best = 0
    for cid in touch_ids:
        if faint_sizes[cid] > best:
            best = faint_sizes[cid]
            rim = cid
    for cid, n in enumerate(faint_sizes):
        if cid and cid != rim:
            drop |= faint_ids == cid
    if not drop.any():
        return image
    arr[:, :, 3] = np.where(drop, 0, alpha)
    return Image.fromarray(arr, 'RGBA')


def monochrome_from_foreground(fg):
    """White silhouette with eyes, nose, cat and arm gaps punched from dark pixels.

    Only clearly dark subject pixels become holes, and only blobs large enough
    to survive a 24dp preview are kept. Whisker-scale noise and bottom chips
    are dropped so the seal holding the cat stays readable.
    """
    arr = np.array(fg)
    rgb = arr[:, :, :3].astype(np.int16)
    alpha = arr[:, :, 3]
    luma = (rgb[:, :, 0] * 30 + rgb[:, :, 1] * 59 + rgb[:, :, 2] * 11) // 100
    body = alpha >= 128
    dark = body & (luma < 110)
    ids, sizes = _component_sizes(dark)
    area = max(1, int(body.sum()))
    # Eyes are the reference: drop specks, keep features that still show at 24dp.
    min_feat = max(8, area // 900)
    keep_dark = np.zeros_like(dark)
    for cid, n in enumerate(sizes):
        if cid and n >= min_feat:
            keep_dark |= ids == cid
    # One-pixel close so an eye stays one hole, without bridging to the fur.
    keep_dark = _dilate(keep_dark) & body & (luma < 160)
    sil = body & ~keep_dark
    sil_ids, sil_sizes = _component_sizes(sil)
    main = int(np.argmax(sil_sizes)) if len(sil_sizes) > 1 else 0
    min_frag = max(12, area // 400)
    sil_keep = sil_ids == main
    for cid, n in enumerate(sil_sizes):
        if cid and cid != main and n >= min_frag:
            sil_keep |= sil_ids == cid
    out = np.zeros_like(arr)
    out[:, :, :3] = 255
    out[:, :, 3] = np.where(sil_keep, 255, 0).astype(np.uint8)
    return Image.fromarray(out, 'RGBA')


def adaptive_background(canvas, coef):
    """Full-bleed linear gradient fitted on the original plate pixels.

    The plane is evaluated in master-pixel space and stretched so the plate's
    own bounding box covers this 108dp layer.
    """
    # Plate bounds on the 722 master (alpha silhouette of the artwork).
    x0, y0, x1, y1 = 16.0, 30.0, 706.0, 692.0
    yy, xx = np.mgrid[0:canvas, 0:canvas]
    mx = x0 + (xx + 0.5) / canvas * (x1 - x0)
    my = y0 + (yy + 0.5) / canvas * (y1 - y0)
    rgb = mx[..., None] * coef[0] + my[..., None] * coef[1] + coef[2]
    rgb = np.clip(np.rint(rgb), 0, 255).astype(np.uint8)
    out = np.zeros((canvas, canvas, 4), np.uint8)
    out[:, :, :3] = rgb
    out[:, :, 3] = 255
    return Image.fromarray(out, 'RGBA')


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
        if density == 'mdpi':
            subject, plate_src, plate_drop = subject_master()
            plate_coef = plate_gradient_coef(plate_src, plate_drop)
        fg, added = adaptive_foreground(f, subject)
        fg = clear_stray_alpha(fg)
        bg = adaptive_background(f, plate_coef)
        # Legacy launcher icons show the same subject and gradient as the
        # adaptive layers, scaled from 108dp down to the 48dp slot.
        legacy = bg.resize((n, n), Image.Resampling.LANCZOS)
        legacy.alpha_composite(fg.resize((n, n), Image.Resampling.LANCZOS))
        png(root + 'ic_launcher.png', legacy.convert('RGB'))
        im = legacy.copy()
        mask = Image.new('L', (n * 4, n * 4))
        ImageDraw.Draw(mask).ellipse((0, 0, n * 4 - 1, n * 4 - 1), fill=255)
        im.putalpha(mask.resize((n, n), Image.Resampling.LANCZOS))
        png(root + 'ic_launcher_round.png', im)
        alpha = np.array(fg.getchannel('A'))
        if int(np.where(alpha >= 128)[0].max()) < f - 2:
            raise SystemExit(f'{density} foreground body does not reach the canvas bottom')
        png(root + 'ic_launcher_foreground.png', fg)
        png(root + 'ic_launcher_background.png', adaptive_background(f, plate_coef))
        png(root + 'ic_stat_logo.png', mono(stat))
        png(root + 'ic_launcher_monochrome.png', monochrome_from_foreground(fg))
    put(android + 'values/ic_launcher_background.xml', b'<?xml version="1.0" encoding="utf-8"?>\n<resources>\n    <color name="ic_launcher_background">#07516B</color>\n</resources>\n')
    for name in ['ic_launcher.xml', 'ic_launcher_round.xml']:
        put(android + 'mipmap-anydpi-v26/' + name, (
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
            '    <background android:drawable="@mipmap/ic_launcher_background"/>\n'
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


def write_adaptive_layers():
    """Foreground subject plus the plate gradient background. Other exports stay."""
    prepare()
    subject, plate_src, plate_drop = subject_master()
    plate_coef = plate_gradient_coef(plate_src, plate_drop)
    android = 'flutter/android/app/src/main/res/'
    diameters = {}
    for density, f in [('mdpi', 108), ('hdpi', 162), ('xhdpi', 216), ('xxhdpi', 324), ('xxxhdpi', 432)]:
        root = android + 'mipmap-' + density + '/'
        fg, added = adaptive_foreground(f, subject)
        fg = clear_stray_alpha(fg)
        alpha = np.array(fg.getchannel('A'))
        ys, xs = np.where(alpha >= 128)
        if int(ys.max()) < f - 2:
            raise SystemExit(f'{density} foreground body does not reach the canvas bottom')
        png(root + 'ic_launcher_foreground.png', fg)
        bg = adaptive_background(f, plate_coef)
        png(root + 'ic_launcher_background.png', bg)
        png(root + 'ic_launcher_monochrome.png', monochrome_from_foreground(fg))
        # 48dp legacy slot is the adaptive composite at this density's scale.
        n = f * 48 // 108
        legacy = bg.resize((n, n), Image.Resampling.LANCZOS)
        legacy.alpha_composite(fg.resize((n, n), Image.Resampling.LANCZOS))
        png(root + 'ic_launcher.png', legacy.convert('RGB'))
        im = legacy.copy()
        mask = Image.new('L', (n * 4, n * 4))
        ImageDraw.Draw(mask).ellipse((0, 0, n * 4 - 1, n * 4 - 1), fill=255)
        im.putalpha(mask.resize((n, n), Image.Resampling.LANCZOS))
        png(root + 'ic_launcher_round.png', im)
        diameters[density] = {
            'width_dp': round((int(xs.max()) - int(xs.min()) + 1) / f * _ADAPTIVE_CANVAS_DP, 2),
            'height_dp': round((int(ys.max()) - int(ys.min()) + 1) / f * _ADAPTIVE_CANVAS_DP, 2),
            'body_pixels_added': int(added),
        }
    for name in ['ic_launcher.xml', 'ic_launcher_round.xml']:
        put(android + 'mipmap-anydpi-v26/' + name, (
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
            '    <background android:drawable="@mipmap/ic_launcher_background"/>\n'
            '    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>\n'
            '    <monochrome android:drawable="@mipmap/ic_launcher_monochrome"/>\n'
            '</adaptive-icon>\n').encode())
    print(json.dumps({'subject_diameter_dp': diameters}, indent=2))


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--adaptive-only':
        write_adaptive_layers()
    else:
        sys.exit(main() or 0)
