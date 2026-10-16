#!/usr/bin/env python3
"""Generate icons from the approved raster master. Requires Pillow only.
Run: uv run --with pillow res/generate_sealdesk_icons.py
The SVG compatibility files embed this PNG; they are not vector redraws.
"""
from pathlib import Path
import base64
import io
import json
import struct
from PIL import Image, ImageDraw

BASE = Path(__file__).resolve().parents[1]
SOURCE = BASE / 'res/sealdesk-icon-approved.png'
MONO_SOURCE = BASE / 'res/sealdesk-icon-monochrome.png'


def render(size, kind='tile'):
    image = Image.open(SOURCE).convert('RGBA').resize((size, size), Image.Resampling.LANCZOS)
    if kind == 'full':
        background = Image.new('RGBA', image.size, '#07516B')
        background.alpha_composite(image)
        return background
    return image


def linux_master():
    image = Image.open(SOURCE).convert('RGBA')
    image = image.crop(image.getchannel('A').getbbox())
    side = max(image.size)
    out = Image.new('RGBA', (side, side))
    out.alpha_composite(image, ((side-image.width)//2, (side-image.height)//2))
    return out


def svg(size=1024, tight=False):
    image = linux_master() if tight else Image.open(SOURCE)
    image = image.resize((size, size), Image.Resampling.LANCZOS) if size < image.width else image
    data = io.BytesIO()
    image.save(data, format='PNG')
    encoded = base64.b64encode(data.getvalue()).decode()
    return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="1024" height="1024" viewBox="0 0 1024 1024">'
            f'<image width="1024" height="1024" xlink:href="data:image/png;base64,{encoded}"/>'
            '</svg>\n').encode()


def mono(size, color='white'):
    source = Image.open(MONO_SOURCE).convert('RGBA').resize((size,size), Image.Resampling.LANCZOS)
    out = Image.new('RGBA', source.size, color)
    out.putalpha(source.getchannel('A'))
    return out


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
    # Export every required frame size from the approved raster master.
    frames = [png_bytes(render(n)) for n in sizes]
    offset = 6 + 16 * len(sizes)
    data = struct.pack('<HHH', 0, 1, len(sizes))
    for n, frame in zip(sizes, frames):
        data += struct.pack('<BBBBHHII', n % 256, n % 256, 0, 0, 1, 32, len(frame), offset)
        offset += len(frame)
    put(name, data + b''.join(frames))


def main():
    for name in ['res/scalable.svg', 'res/logo.svg', 'res/logo-header.svg', 'flutter/assets/icon.svg']:
        put(name, svg(tight=name == 'res/scalable.svg'))
    put('res/sealdesk-icon.svg', svg())
    put('res/sealdesk-icon-small.svg', svg(32))
    for name in ['res/icon.png', 'res/mac-icon.png', 'flutter/assets/icon.png']:
        png(name, render(1024))
    for label, n in [('32x32', 32), ('64x64', 64), ('128x128', 128), ('128x128@2x', 256)]:
        png('res/' + label + '.png', linux_master().resize((n,n), Image.Resampling.LANCZOS))
    for name in ['res/icon.ico', 'flutter/windows/runner/resources/app_icon.ico', 'res/tray-icon.ico']:
        ico(name, [16, 24, 32, 48, 64, 128, 256])
    # Explicit ICNS chunks include the required standard and Retina slots.
    chunks = []
    for tag, n in [('icp4', 16), ('icp5', 32), ('icp6', 64), ('ic07', 128),
                   ('ic08', 256), ('ic09', 512), ('ic10', 1024),
                   ('ic11', 32), ('ic12', 64), ('ic13', 256), ('ic14', 512)]:
        data = png_bytes(render(n))
        chunks.append(tag.encode() + struct.pack('>I', len(data) + 8) + data)
    body = b''.join(chunks)
    put('flutter/macos/Runner/AppIcon.icns', b'icns' + struct.pack('>I', len(body) + 8) + body)
    for name, n, color in [('mac-tray-dark-x2', 60, 'white'), ('mac-tray-light-x2', 48, 'black')]:
        png('res/' + name + '.png', mono(n, color))
    root = 'flutter/ios/Runner/Assets.xcassets/AppIcon.appiconset/'
    for item in json.loads((BASE / root / 'Contents.json').read_text())['images']:
        n = round(float(item['size'].split('x')[0]) * float(item['scale'][:-1]))
        png(root + item['filename'], render(n, 'full').convert('RGB'))
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
        # Enlarge the approved artwork to reduce the inset tile effect.
        fg = Image.new('RGBA', (f, f))
        k = round(f * 0.76)
        art = render(k, 'foreground')
        fg.alpha_composite(art, ((f - k) // 2, (f - k) // 2))
        png(root + 'ic_launcher_foreground.png', fg)
        png(root + 'ic_stat_logo.png', mono(stat))
        k = round(f * 0.58)
        mf = Image.new('RGBA', (f, f))
        mi = mono(k)
        mf.alpha_composite(mi, ((f-k)//2, (f-k)//2))
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
    preview = Image.new('RGB', (1120,650), '#e9eae7')
    im = render(500)
    preview.paste(im, (20, 35), im)
    draw = ImageDraw.Draw(preview)
    for i, n in enumerate([128, 64, 48, 32, 24, 16]):
        x, y = 580 + i % 3 * 170, 80 + i // 3 * 220
        im = render(n)
        preview.paste(im, (x,y), im)
        draw.text((x,y+n+14), str(n)+' px', fill='#143A49')
    for i, color in enumerate(['white','black']):
        x = 610+i*150
        draw.rectangle((x-8,540,x+72,620), fill='#183E50' if i==0 else '#ffffff')
        im=mono(64,color)
        preview.paste(im,(x,548),im)
    png('tmp/sealdesk-final-preview.png', preview)
    print('SealDesk icons generated from approved raster master')


if __name__ == '__main__':
    main()
