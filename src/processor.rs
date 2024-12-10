use image::{
    codecs::{
        jpeg::{JpegDecoder, JpegEncoder},
        png::{PngDecoder, PngEncoder},
        webp::WebPDecoder,
    },
    imageops::FilterType,
    DynamicImage, ImageDecoder, ImageEncoder,
};
use libwebp_sys::*;
use std::{
    io::{self, Cursor, ErrorKind},
    mem::{self, MaybeUninit},
    ptr,
};

// TODO: Make this configurable
pub const RESIZE_FILTER: FilterType = FilterType::Triangle;
pub const DEFAULT_MAX_WIDTH: u32 = 4096;
pub const DEFAULT_MAX_HEIGHT: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageType {
    Png,
    Jpeg,
    WebP,
    Svg,
    Unknown,
}

impl From<image::ImageFormat> for ImageType {
    #[inline]
    fn from(fmt: image::ImageFormat) -> Self {
        match fmt {
            image::ImageFormat::Png => Self::Png,
            image::ImageFormat::Jpeg => Self::Jpeg,
            image::ImageFormat::WebP => Self::WebP,
            _ => Self::Unknown,
        }
    }
}

impl From<&str> for ImageType {
    #[inline]
    fn from(fmt: &str) -> Self {
        match fmt {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::WebP,
            "svg" => Self::Svg,
            _ => Self::Unknown,
        }
    }
}

impl ImageType {
    #[inline]
    pub const fn to_mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
            Self::Svg => "image/svg+xml",
            Self::Unknown => "application/octet-stream",
        }
    }

    pub fn detect_image_format(data: &[u8]) -> Option<ImageType> {
        let result = image::guess_format(data)
            .map(ImageType::from)
            .unwrap_or(ImageType::Unknown);

        if result == ImageType::Unknown {
            if is_svg(data) {
                return Some(ImageType::Svg);
            }

            return None;
        }

        Some(result)
    }
}

#[derive(Debug)]
pub struct ImageParams {
    pub quality: u8,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub format: Option<ImageType>,
}

fn is_svg(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Only scan first 100 bytes
    let scan_size = data.len().min(100);
    let data = &data[..scan_size];

    // Skip whitespace and look for SVG markers
    let mut i = 0;
    while i < data.len() {
        // Skip whitespace bytes
        while i < data.len() && data[i].is_ascii_whitespace() {
            i += 1;
        }

        // Check for SVG markers
        if i + 5 <= data.len() && &data[i..i + 5] == b"<?xml" {
            return true;
        }
        if i + 4 <= data.len() && &data[i..i + 4] == b"<svg" {
            return true;
        }

        // If we found a non-whitespace char that's not the start of SVG, it's not SVG
        break;
    }

    false
}

fn encode_webp(img: &DynamicImage, quality: u8, icc_profile: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let width = img.width() as i32;
    let height = img.height() as i32;

    let mut out_buf = ptr::null_mut();
    let output_size = unsafe {
        let result = if img.color().has_alpha() {
            let rgba = img.to_rgba8();
            let rgba_ptr = rgba.as_ptr();

            if quality >= 100 {
                WebPEncodeLosslessRGBA(rgba_ptr, width, height, width * 4, &mut out_buf)
            } else {
                WebPEncodeRGBA(
                    rgba_ptr,
                    width,
                    height,
                    width * 4,
                    quality as f32,
                    &mut out_buf,
                )
            }
        } else {
            let rgb = img.to_rgb8();
            let rgb_ptr = rgb.as_ptr();

            if quality >= 100 {
                WebPEncodeLosslessRGB(rgb_ptr, width, height, width * 3, &mut out_buf)
            } else {
                WebPEncodeRGB(
                    rgb_ptr,
                    width,
                    height,
                    width * 3,
                    quality as f32,
                    &mut out_buf,
                )
            }
        };

        if result == 0 || out_buf.is_null() {
            return Err(io::Error::new(ErrorKind::Other, "WebP encoding failed"));
        }

        result
    };

    let mux = WebPMuxNew();
    if mux.is_null() {
        unsafe {
            WebPFree(out_buf as *mut _);
        }

        return Err(io::Error::new(
            ErrorKind::Other,
            "Failed to create WebP mux",
        ));
    }

    let mut webp_data = unsafe { MaybeUninit::zeroed().assume_init() };
    WebPDataInit(&mut webp_data);

    let mut img_data = WebPData {
        bytes: out_buf,
        size: output_size as usize,
    };

    let status = unsafe { WebPMuxSetImage(mux, &mut img_data, 1) };

    if status != WebPMuxError::WEBP_MUX_OK {
        unsafe {
            WebPFree(out_buf as *mut _);
            WebPMuxDelete(mux);
            WebPDataClear(&mut webp_data); // Add this line
        }
        return Err(io::Error::new(
            ErrorKind::Other,
            "Failed to set WebP image data",
        ));
    }

    if let Some(icc) = icc_profile {
        let icc_data = WebPData {
            bytes: icc.as_ptr(),
            size: icc.len(),
        };

        let status = unsafe { WebPMuxSetChunk(mux, b"ICCP\0".as_ptr() as _, &icc_data, 1) };

        if status != WebPMuxError::WEBP_MUX_OK {
            unsafe {
                WebPFree(out_buf as *mut _);
                WebPMuxDelete(mux);
                WebPDataClear(&mut webp_data); // Add this line
            }
            return Err(io::Error::new(
                ErrorKind::Other,
                "Failed to add ICC profile",
            ));
        }
    }

    let status = unsafe { WebPMuxAssemble(mux, &mut webp_data) };

    if status != WebPMuxError::WEBP_MUX_OK {
        unsafe {
            WebPFree(out_buf as *mut _);
            WebPMuxDelete(mux);
        }
        return Err(io::Error::new(
            ErrorKind::Other,
            "Failed to assemble WebP image",
        ));
    }

    let final_data = unsafe {
        let slice = std::slice::from_raw_parts(webp_data.bytes, webp_data.size);
        let result = slice.to_vec();

        WebPDataClear(&mut webp_data);
        WebPFree(out_buf as *mut _);
        WebPMuxDelete(mux);

        result
    };

    Ok(final_data)
}

pub struct ImageProcessResult {
    pub data: Vec<u8>,
    pub format: ImageType,
}

pub fn process_image(
    img_data: Vec<u8>,
    src_format: ImageType,
    params: &ImageParams,
) -> io::Result<ImageProcessResult> {
    let mut decoder: Box<dyn ImageDecoder> = match src_format {
        ImageType::Png => Box::new(PngDecoder::new(Cursor::new(&img_data)).map_err(|_| {
            io::Error::new(ErrorKind::InvalidData, "Failed to decode image as PNG")
        })?),
        ImageType::Jpeg => Box::new(JpegDecoder::new(Cursor::new(&img_data)).map_err(|_| {
            io::Error::new(ErrorKind::InvalidData, "Failed to decode image as JPEG")
        })?),
        ImageType::WebP => Box::new(WebPDecoder::new(Cursor::new(&img_data)).map_err(|_| {
            io::Error::new(ErrorKind::InvalidData, "Failed to decode image as WebP")
        })?),
        _ => return Err(io::Error::new(ErrorKind::InvalidData, "Unsupported format")),
    };

    let color_profile = decoder.icc_profile().unwrap_or(None);

    let (width, height) = decoder.dimensions();

    let max_width = params.max_width.unwrap_or(DEFAULT_MAX_WIDTH);
    let max_height = params.max_height.unwrap_or(DEFAULT_MAX_HEIGHT);

    let (mut new_width, mut new_height) = (width, height);

    // Only resize if image exceeds the maximum dimensions
    if width > max_width || height > max_height {
        let ratio = f64::min(
            max_width as f64 / width as f64,
            max_height as f64 / height as f64,
        );

        new_width = (width as f64 * ratio).round() as u32;
        new_height = (height as f64 * ratio).round() as u32;
    }
    let dst_format = if let Some(ref fmt) = params.format {
        match fmt {
            ImageType::Png => ImageType::Png,
            ImageType::Jpeg => ImageType::Jpeg,
            ImageType::WebP => ImageType::WebP,
            _ => return Err(io::Error::new(ErrorKind::Other, "Unsupported format")),
        }
    } else {
        src_format
    };

    // If no processing is needed, return the original image to avoid redundant work.
    // no work is needed if the image has the same format and dimensions
    if new_width == width && new_height == height && src_format == dst_format {
        mem::drop(decoder);

        return Ok(ImageProcessResult {
            data: img_data,
            format: src_format,
        });
    }

    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|_| io::Error::new(ErrorKind::Other, "Failed to decode image"))?;

    img = img.resize(new_width, new_height, RESIZE_FILTER);

    let mut buffer = Cursor::new(Vec::new());

    let output_bytes = match dst_format {
        ImageType::Png => {
            let mut encoder = PngEncoder::new(&mut buffer);
            if let Some(profile) = color_profile {
                encoder.set_icc_profile(profile).unwrap();
            }
            encoder
                .write_image(
                    img.as_bytes(),
                    img.width(),
                    img.height(),
                    img.color().into(),
                )
                .map_err(|_| io::Error::new(ErrorKind::Other, "Failed to encode image"))?;
            buffer.into_inner()
        }
        ImageType::WebP => encode_webp(&img, params.quality, color_profile.as_deref())?,
        ImageType::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut buffer, params.quality);
            if let Some(profile) = color_profile {
                encoder.set_icc_profile(profile).unwrap();
            }
            encoder
                .encode(
                    img.as_bytes(),
                    img.width(),
                    img.height(),
                    img.color().into(),
                )
                .map_err(|_| io::Error::new(ErrorKind::Other, "Failed to encode image"))?;
            buffer.into_inner()
        }
        _ => return Err(io::Error::new(ErrorKind::Other, "Unsupported format")),
    };

    Ok(ImageProcessResult {
        data: output_bytes,
        format: dst_format,
    })
}
