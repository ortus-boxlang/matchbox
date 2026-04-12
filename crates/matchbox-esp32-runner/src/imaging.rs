use jpeg_decoder::{Decoder, PixelFormat};
use std::io::Cursor;

#[derive(Clone, Debug)]
pub struct MonochromeBitmap {
    pub width: usize,
    pub height: usize,
    pub bytes_per_row: usize,
    pub bytes: Vec<u8>,
}

fn is_black(luminance: u8, invert: bool) -> bool {
    if invert {
        luminance > 127
    } else {
        luminance < 127
    }
}

pub fn grayscale_to_monochrome_bitmap(
    width: usize,
    height: usize,
    grayscale_bytes: &[u8],
) -> Result<MonochromeBitmap, String> {
    let expected_len = width
        .checked_mul(height)
        .ok_or_else(|| "grayscale image dimensions overflowed".to_string())?;
    if grayscale_bytes.len() < expected_len {
        return Err(format!(
            "grayscale buffer too small: expected at least {} bytes, got {}",
            expected_len,
            grayscale_bytes.len()
        ));
    }

    let invert = false;
    let bytes_per_row = width.div_ceil(8);
    let mut packed = vec![0u8; bytes_per_row * height];

    for y in 0..height {
        for x in 0..width {
            let luminance = grayscale_bytes[y * width + x];
            if is_black(luminance, invert) {
                packed[y * bytes_per_row + (x / 8)] |= 0x80 >> (x % 8);
            }
        }
    }

    Ok(MonochromeBitmap {
        width,
        height,
        bytes_per_row,
        bytes: packed,
    })
}

// This stays runner-owned for the embedded branch. If the implementation later
// shares code with another crate, keep this API surface stable and local.
pub fn jpeg_to_monochrome_bitmap(jpeg_bytes: &[u8]) -> Result<MonochromeBitmap, String> {
    let invert = false;
    let mut decoder = Decoder::new(Cursor::new(jpeg_bytes));
    let pixels = decoder
        .decode()
        .map_err(|err| format!("JPEG decode failed: {err}"))?;
    let info = decoder
        .info()
        .ok_or_else(|| "JPEG metadata was unavailable.".to_string())?;

    let width = info.width as usize;
    let height = info.height as usize;
    let bytes_per_row = width.div_ceil(8);
    let mut packed = vec![0u8; bytes_per_row * height];

    match info.pixel_format {
        PixelFormat::L8 => {
            for y in 0..height {
                for x in 0..width {
                    if is_black(pixels[y * width + x], invert) {
                        packed[y * bytes_per_row + (x / 8)] |= 0x80 >> (x % 8);
                    }
                }
            }
        }
        PixelFormat::RGB24 => {
            for y in 0..height {
                for x in 0..width {
                    let idx = (y * width + x) * 3;
                    let r = pixels[idx] as f32;
                    let g = pixels[idx + 1] as f32;
                    let b = pixels[idx + 2] as f32;
                    let luminance = (0.299 * r + 0.587 * g + 0.114 * b) as u8;

                    if is_black(luminance, invert) {
                        packed[y * bytes_per_row + (x / 8)] |= 0x80 >> (x % 8);
                    }
                }
            }
        }
        other => {
            return Err(format!("Unsupported JPEG pixel format: {other:?}"));
        }
    }

    Ok(MonochromeBitmap {
        width,
        height,
        bytes_per_row,
        bytes: packed,
    })
}
