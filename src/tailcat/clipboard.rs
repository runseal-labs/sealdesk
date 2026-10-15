use arboard::ImageData;
use hbb_common::{anyhow::anyhow, bail, ResultType};
use image::ImageEncoder;
use std::io::Cursor;

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

pub(crate) fn read_image() -> ResultType<Vec<u8>> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|_| anyhow!("无法打开系统剪贴板，请确认桌面会话可用"))?;
    let image = clipboard
        .get_image()
        .map_err(|_| anyhow!("剪贴板中没有可读取的图片，请复制二维码截图后重试"))?;
    into_png(image)
}

fn check_dimensions(width: usize, height: usize) -> ResultType<()> {
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width * height > 16 * 1024 * 1024
    {
        bail!("图片尺寸过大或无效，请裁剪到二维码区域后重试");
    }
    Ok(())
}

fn into_png(image: ImageData<'_>) -> ResultType<Vec<u8>> {
    let bytes = match image {
        ImageData::Png(bytes) => {
            if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
                bail!("图片为空或超过 20 MB，请选择较小的二维码截图");
            }
            let (width, height) = image::io::Reader::with_format(
                Cursor::new(bytes.as_ref()),
                image::ImageFormat::Png,
            )
            .into_dimensions()
            .map_err(|_| anyhow!("无法读取剪贴板中的 PNG 图片，请重新截图"))?;
            check_dimensions(width as usize, height as usize)?;
            bytes.into_owned()
        }
        ImageData::Rgba(rgba) => {
            check_dimensions(rgba.width, rgba.height)?;
            if rgba.bytes.len() != rgba.width * rgba.height * 4 {
                bail!("剪贴板图片数据不完整，请重新截图");
            }
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes).write_image(
                &rgba.bytes,
                rgba.width as u32,
                rgba.height as u32,
                image::ColorType::Rgba8,
            )?;
            bytes
        }
        ImageData::Svg(_) => bail!("不支持剪贴板中的矢量图片，请复制二维码截图"),
    };
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!("图片超过 20 MB，请裁剪到二维码区域后重试");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn preserves_png_and_rgba_pixels() {
        let pixels = [255, 0, 0, 128, 0, 255, 0, 255];
        let png = into_png(ImageData::rgba(2, 1, Cow::Borrowed(&pixels))).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.as_raw(), &pixels);
        assert_eq!(into_png(ImageData::png(Cow::Borrowed(&png))).unwrap(), png);
    }

    #[test]
    fn rejects_invalid_dimensions_and_incomplete_buffers() {
        for (width, height, pixels) in [
            (0, 1, Vec::new()),
            (8193, 1, Vec::new()),
            (8192, 8192, Vec::new()),
            (usize::MAX, 2, Vec::new()),
            (2, 1, vec![0; 4]),
        ] {
            assert!(into_png(ImageData::rgba(width, height, pixels.into())).is_err());
        }
    }

    #[test]
    fn rejects_invalid_or_unsupported_content() {
        assert!(into_png(ImageData::png(Cow::Borrowed(b"not a png"))).is_err());
        assert!(into_png(ImageData::png(vec![0; MAX_IMAGE_BYTES + 1].into())).is_err());
        assert!(into_png(ImageData::svg("<svg/>")).is_err());
    }

    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn live_clipboard_image() {
        assert!(std::env::var("TAILDESK_CLIPBOARD_PROBE").is_ok());
        let mut owner = arboard::Clipboard::new().unwrap();
        let pixels = [255, 255, 255, 255, 0, 0, 0, 255];
        owner
            .set_image(ImageData::rgba(2, 1, Cow::Borrowed(&pixels)))
            .unwrap();
        let read = read_image().unwrap();
        let decoded = image::load_from_memory(&read).unwrap().into_rgba8();
        assert_eq!(decoded.as_raw(), &pixels);
        owner.set_text("plain text without image").unwrap();
        assert!(read_image()
            .unwrap_err()
            .to_string()
            .contains("没有可读取的图片"));
        println!("LIVE_CLIPBOARD_IMAGE_OK real X11 clipboard preserves image pixels");
    }
}
