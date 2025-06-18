use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq)]
pub struct Image<'a> {
    pub width: usize,
    pub height: usize,
    pub bytes: Cow<'a, [u8]>,
}

impl<'a> From<Image<'a>> for arboard::ImageData<'a> {
    fn from(value: Image<'a>) -> Self {
        Self {
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

impl<'a> From<arboard::ImageData<'a>> for Image<'a> {
    fn from(value: arboard::ImageData<'a>) -> Self {
        Self {
            width: value.width,
            height: value.height,
            bytes: value.bytes,
        }
    }
}

pub trait Clipboard {
    fn get_text(&mut self) -> anyhow::Result<String>;
    fn get_image(&mut self) -> anyhow::Result<Image<'static>>;
    fn set_text(&mut self, data: String) -> anyhow::Result<()>;
    fn set_image(&mut self, data: Image<'static>) -> anyhow::Result<()>;
}

impl Clipboard for arboard::Clipboard {
    fn get_text(&mut self) -> anyhow::Result<String> {
        arboard::Clipboard::get_text(self).map_err(Into::into)
    }

    fn get_image(&mut self) -> anyhow::Result<Image<'static>> {
        arboard::Clipboard::get_image(self)
            .map_err(Into::into)
            .map(Image::from)
    }

    fn set_text(&mut self, data: String) -> anyhow::Result<()> {
        arboard::Clipboard::set_text(self, data).map_err(Into::into)
    }

    fn set_image(&mut self, data: Image<'static>) -> anyhow::Result<()> {
        arboard::Clipboard::set_image(self, data.into()).map_err(Into::into)
    }
}

pub struct WaylandClipboard;

impl Clipboard for WaylandClipboard {
    fn get_text(&mut self) -> anyhow::Result<String> {
        use std::io::Read;
        use wl_clipboard_rs::paste::*;
        let (mut pipe, _) =
            get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text)?;
        let mut contents = vec![];
        pipe.read_to_end(&mut contents)?;
        Ok(String::from_utf8_lossy(&contents).into_owned())
    }

    fn get_image(&mut self) -> anyhow::Result<Image<'static>> {
        use std::io::Read;
        use wl_clipboard_rs::paste::*;
        let (mut pipe, mime) =
            get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Any)?;
        let format = match mime.as_str() {
            "image/png" => image::ImageFormat::Png,
            "image/gif" => image::ImageFormat::Gif,
            "image/webp" => image::ImageFormat::WebP,
            "image/jpg" | "image/jpeg" => image::ImageFormat::Jpeg,
            _ => anyhow::bail!("Unsuported mime {mime}"),
        };
        let mut contents = vec![];
        pipe.read_to_end(&mut contents)?;
        let mut cursor = std::io::Cursor::new(contents);
        let mut reader = image::ImageReader::new(&mut cursor);
        reader.set_format(format);
        let img = reader.decode()?.into_rgba8();
        let (w, h) = img.dimensions();
        Ok(Image {
            width: w as usize,
            height: h as usize,
            bytes: img.into_raw().into(),
        })
    }

    fn set_text(&mut self, data: String) -> anyhow::Result<()> {
        use wl_clipboard_rs::copy::*;
        let mut opts = Options::new();
        opts.clipboard(ClipboardType::Regular);
        opts.copy(Source::Bytes(data.into_bytes().into()), MimeType::Text)?;
        Ok(())
    }

    fn set_image(&mut self, data: Image<'static>) -> anyhow::Result<()> {
        use image::ImageEncoder as _;
        use wl_clipboard_rs::copy::*;

        if data.bytes.is_empty() || data.width == 0 || data.height == 0 {
            anyhow::bail!("Empty image");
        }

        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        encoder
            .write_image(
                data.bytes.as_ref(),
                data.width as u32,
                data.height as u32,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| anyhow::anyhow!("Encoding error: {e}"))?;

        let mut opts = Options::new();
        opts.clipboard(ClipboardType::Regular);
        opts.copy(
            Source::Bytes(png_bytes.into()),
            MimeType::Specific("image/png".to_string()),
        )?;
        Ok(())
    }
}
