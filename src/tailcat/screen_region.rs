use std::{
    io::ErrorKind,
    thread,
    time::{Duration, Instant},
};

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
    /// Maps caller coordinates onto the framebuffer. `1` when the caller
    /// already passed pixels; on macOS the caller passes points.
    pub scale: f64,
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
    let x = scale_px(x, display.scale);
    let y = scale_px(y, display.scale);
    let left = scale_px(display.x, display.scale);
    let top = scale_px(display.y, display.scale);
    display.width > 0
        && display.height > 0
        && x >= left
        && y >= top
        && x < left.saturating_add(display.width)
        && y < top.saturating_add(display.height)
}

pub(crate) fn scale_px(value: i32, scale: f64) -> i32 {
    if !scale.is_finite() || scale <= 0.0 || (scale - 1.0).abs() < f64::EPSILON {
        return value;
    }
    let scaled = (value as f64) * scale;
    if scaled >= i32::MAX as f64 {
        i32::MAX
    } else if scaled <= i32::MIN as f64 {
        i32::MIN
    } else {
        scaled.round() as i32
    }
}

fn region_pixels(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    origin: (i32, i32),
    scale: f64,
) -> (i32, i32, i32, i32) {
    (
        scale_px(x.saturating_sub(origin.0), scale),
        scale_px(y.saturating_sub(origin.1), scale),
        scale_px(width, scale),
        scale_px(height, scale),
    )
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
                rgba[d + 3] = 255;
            }
        } else {
            rgba[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
            for col in 0..crop_w {
                rgba[dst + col * 4 + 3] = 255;
            }
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
                // macOS `origin()` is in points while `width()` is the
                // framebuffer. `scale()` is the factor that joins them, and
                // it follows the retina flag the capturer itself uses.
                #[cfg(target_os = "macos")]
                scale: {
                    let scale = display.scale();
                    if scale.is_finite() && scale > 0.0 {
                        scale
                    } else {
                        1.0
                    }
                },
                #[cfg(not(target_os = "macos"))]
                scale: 1.0,
            }
        })
        .collect::<Vec<_>>();
    let index =
        display_for_region(x, y, width, height, &rects).ok_or_else(|| anyhow!(CAPTURE_ERROR))?;
    let geometry = rects[index];
    let (crop_x, crop_y, crop_w, crop_h) = region_pixels(
        x,
        y,
        width,
        height,
        (geometry.x, geometry.y),
        geometry.scale,
    );
    let display = displays
        .into_iter()
        .nth(index)
        .ok_or_else(|| anyhow!(CAPTURE_ERROR))?;
    let mut capturer = scrap::Capturer::new(display).map_err(|_| anyhow!(CAPTURE_ERROR))?;
    read_png(&mut capturer, crop_x, crop_y, crop_w, crop_h)
}

#[cfg_attr(not(feature = "flutter"), allow(dead_code))]
fn read_png(
    capturer: &mut scrap::Capturer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> ResultType<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!(CAPTURE_ERROR);
        }
        let wait = (deadline - now).min(Duration::from_millis(100));
        match capturer.frame(wait) {
            Ok(frame) => return png_from_frame(frame, x, y, width, height),
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(Instant::now());
                let pause = Duration::from_millis(20).min(left);
                if pause.is_zero() {
                    bail!(CAPTURE_ERROR);
                }
                thread::sleep(pause);
            }
            Err(_) => bail!(CAPTURE_ERROR),
        }
    }
}

#[cfg_attr(not(feature = "flutter"), allow(dead_code))]
fn png_from_frame(
    frame: scrap::Frame<'_>,
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
        x,
        y,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_the_selected_bgra_pixel_to_png() {
        let pixels = [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 10, 11, 12, 0];
        let png = crop_to_png(&pixels, 2, 2, 8, true, 1, 0, 1, 1).unwrap();
        let image = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.as_raw(), &[6, 5, 4, 255]);
        let straight = crop_to_png(&pixels, 2, 2, 8, false, 0, 0, 1, 1).unwrap();
        let straight = image::load_from_memory(&straight).unwrap().into_rgba8();
        assert_eq!(straight.as_raw(), &[1, 2, 3, 255]);
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
                scale: 1.0,
            },
            DisplayRect {
                x: 100,
                y: 0,
                width: 80,
                height: 60,
                scale: 1.0,
            },
        ];
        assert_eq!(display_for_region(120, 10, 20, 20, &displays), Some(1));
        assert_eq!(display_for_region(10, 10, 30, 30, &displays), Some(0));
        assert_eq!(display_for_region(200, 10, 10, 10, &displays), None);
        assert_eq!(display_for_region(0, 0, 0, 10, &displays), None);
    }

    #[test]
    fn mac_secondary_display_is_chosen_in_its_own_pixels() {
        let displays = [
            DisplayRect {
                x: 0,
                y: 0,
                width: 2880,
                height: 1800,
                scale: 2.0,
            },
            DisplayRect {
                x: 1440,
                y: 0,
                width: 1920,
                height: 1080,
                scale: 1.0,
            },
        ];
        assert_eq!(display_for_region(2395, 535, 10, 10, &displays), Some(1));
        assert_eq!(display_for_region(715, 445, 10, 10, &displays), Some(0));
        assert_eq!(
            region_pixels(1500, 10, 20, 20, (1440, 0), 2.0),
            (120, 20, 40, 40)
        );
    }
}
