use std::time::Duration;

use hbb_common::{anyhow::anyhow, bail, ResultType};
use image::ImageEncoder;
use scrap::{TraitCapturer, TraitPixelBuffer};

const CAPTURE_ERROR: &str = "Unable to read the QR image. Please choose another one";

#[derive(Clone, Copy)]
pub(crate) struct DisplayRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub(crate) fn display_for_region(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    displays: &[DisplayRect],
) -> Option<usize> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let center_x = x.saturating_add(width / 2);
    let center_y = y.saturating_add(height / 2);
    displays
        .iter()
        .position(|display| contains(display, center_x, center_y))
}

fn contains(display: &DisplayRect, x: i32, y: i32) -> bool {
    display.width > 0
        && display.height > 0
        && x >= display.x
        && y >= display.y
        && x < display.x.saturating_add(display.width)
        && y < display.y.saturating_add(display.height)
}

pub(crate) fn crop_to_png(
    pixels: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    bgra: bool,
    x: i32,
    y: i32,
    crop_w: i32,
    crop_h: i32,
) -> ResultType<Vec<u8>> {
    if width == 0 || height == 0 || crop_w <= 0 || crop_h <= 0 || x < 0 || y < 0 {
        bail!(CAPTURE_ERROR);
    }
    let (x, y) = (x as usize, y as usize);
    let (crop_w, crop_h) = (crop_w as usize, crop_h as usize);
    let row_bytes = crop_w
        .checked_mul(4)
        .ok_or_else(|| anyhow!(CAPTURE_ERROR))?;
    let Some(end_x) = x.checked_add(crop_w) else {
        bail!(CAPTURE_ERROR);
    };
    let Some(end_y) = y.checked_add(crop_h) else {
        bail!(CAPTURE_ERROR);
    };
    if end_x > width || end_y > height || stride < width.saturating_mul(4) {
        bail!(CAPTURE_ERROR);
    }
    let Some(needed) = stride.checked_mul(height) else {
        bail!(CAPTURE_ERROR);
    };
    if pixels.len() < needed {
        bail!(CAPTURE_ERROR);
    }
    let mut rgba = vec![0u8; row_bytes * crop_h];
    for row in 0..crop_h {
        let src = (y + row) * stride + x * 4;
        let dst = row * row_bytes;
        if bgra {
            for col in 0..crop_w {
                let s = src + col * 4;
                let d = dst + col * 4;
                rgba[d] = pixels[s + 2];
                rgba[d + 1] = pixels[s + 1];
                rgba[d + 2] = pixels[s];
                rgba[d + 3] = pixels[s + 3];
            }
        } else {
            rgba[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
        }
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&rgba, crop_w as u32, crop_h as u32, image::ColorType::Rgba8)
        .map_err(|_| anyhow!(CAPTURE_ERROR))?;
    Ok(bytes)
}

#[cfg_attr(not(feature = "flutter"), allow(dead_code))]
pub(crate) fn capture_png(x: i32, y: i32, width: i32, height: i32) -> ResultType<Vec<u8>> {
    #[cfg(target_os = "linux")]
    if !crate::platform::linux::is_x11() {
        bail!(CAPTURE_ERROR);
    }
    let displays = scrap::Display::all().map_err(|_| anyhow!(CAPTURE_ERROR))?;
    let rects = displays
        .iter()
        .map(|display| {
            let (origin_x, origin_y) = display.origin();
            DisplayRect {
                x: origin_x,
                y: origin_y,
                width: i32::try_from(display.width()).unwrap_or(0),
                height: i32::try_from(display.height()).unwrap_or(0),
            }
        })
        .collect::<Vec<_>>();
    let index =
        display_for_region(x, y, width, height, &rects).ok_or_else(|| anyhow!(CAPTURE_ERROR))?;
    let origin = (rects[index].x, rects[index].y);
    let display = displays
        .into_iter()
        .nth(index)
        .ok_or_else(|| anyhow!(CAPTURE_ERROR))?;
    let mut capturer = scrap::Capturer::new(display).map_err(|_| anyhow!(CAPTURE_ERROR))?;
    let frame = capturer
        .frame(Duration::from_millis(300))
        .map_err(|_| anyhow!(CAPTURE_ERROR))?;
    png_from_frame(frame, origin, x, y, width, height)
}

#[cfg_attr(not(feature = "flutter"), allow(dead_code))]
fn png_from_frame(
    frame: scrap::Frame<'_>,
    origin: (i32, i32),
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> ResultType<Vec<u8>> {
    let scrap::Frame::PixelBuffer(buffer) = frame else {
        bail!(CAPTURE_ERROR);
    };
    let bgra = match buffer.pixfmt() {
        scrap::Pixfmt::BGRA => true,
        scrap::Pixfmt::RGBA => false,
        _ => bail!(CAPTURE_ERROR),
    };
    let stride = buffer.stride().first().copied().unwrap_or(0);
    crop_to_png(
        buffer.data(),
        buffer.width(),
        buffer.height(),
        stride,
        bgra,
        x.saturating_sub(origin.0),
        y.saturating_sub(origin.1),
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_the_selected_bgra_pixel_to_png() {
        let pixels = [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        let png = crop_to_png(&pixels, 2, 2, 8, true, 1, 0, 1, 1).unwrap();
        let image = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.as_raw(), &[6, 5, 4, 255]);
        assert!(crop_to_png(&pixels, 2, 2, 8, true, 1, 0, 2, 1).is_err());
        assert!(crop_to_png(&pixels, 2, 2, 8, false, -1, 0, 1, 1).is_err());
    }

    #[test]
    fn picks_the_display_that_contains_the_region_center() {
        let displays = [
            DisplayRect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            DisplayRect {
                x: 100,
                y: 0,
                width: 80,
                height: 60,
            },
        ];
        assert_eq!(display_for_region(120, 10, 20, 20, &displays), Some(1));
        assert_eq!(display_for_region(10, 10, 30, 30, &displays), Some(0));
        assert_eq!(display_for_region(200, 10, 10, 10, &displays), None);
        assert_eq!(display_for_region(0, 0, 0, 10, &displays), None);
    }
}
