//! Image I/O — convert the composed grayscale canvas into the wire
//! formats served over HTTP.
//!
//! - `to_mono` thresholds the 8-bit canvas at 128 to pure black/white.
//! - `to_png_bytes` writes 8-bit grayscale PNG (browser preview).
//! - `to_bmp_bytes` writes a 1-bit BMP for ESPHome `online_image`
//!   `type: BINARY`, with bits inverted relative to the on-screen
//!   polarity so bit=1 means "draw pixel" (black on ePaper).
//!
//! ## Wire-format parity with Python
//!
//! The 1-bit BMP is hand-rolled rather than going through the `image`
//! crate's BMP encoder because the latter has no clean 1bpp path. The
//! handful of header bytes is straightforward, and rolling our own
//! gives bit-for-bit parity with Pillow's `mode="1"` BMP output —
//! which is what ESPHome's `online_image` parser expects.

use std::io::Cursor;

use anyhow::{Context, Result};
use image::{codecs::png::PngEncoder, ExtendedColorType, GrayImage, ImageEncoder, Luma};

/// Threshold for 8-bit → 1-bit collapse. Pixels below this become black,
/// at or above become white. Matches the `< 128` check in the Python
/// addon's `to_mono`.
pub const THRESHOLD: u8 = 128;

/// Hard-threshold the canvas to pure black/white.
///
/// Returns a new `GrayImage` where each pixel is exactly 0 or 255 — no
/// in-between values. This deliberately uses no dithering: ePaper has
/// no greyscale, and dithering thin strokes (descenders, the dot on
/// `i`, etc.) produces noisy speckles. Anti-aliased edges from
/// `draw_text` get cleanly thresholded at this point.
pub fn to_mono(img: &GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut out = GrayImage::new(w, h);
    for (src, dst) in img.pixels().zip(out.pixels_mut()) {
        dst[0] = if src[0] < THRESHOLD { 0 } else { 255 };
    }
    out
}

/// Encode the canvas as a grayscale PNG — for browser preview.
///
/// Output is 8-bit L8 with values strictly in {0, 255}. A true 1-bit
/// PNG would be ~2x smaller but the `image` crate's encoder doesn't
/// expose 1bpp without dragging in `png` as a direct dep; the size
/// difference is irrelevant for the dashboard preview path.
pub fn to_png_bytes(img: &GrayImage) -> Result<Vec<u8>> {
    let mono = to_mono(img);
    let mut buf = Cursor::new(Vec::new());
    PngEncoder::new(&mut buf)
        .write_image(
            mono.as_raw(),
            mono.width(),
            mono.height(),
            ExtendedColorType::L8,
        )
        .context("PNG encode failed")?;
    Ok(buf.into_inner())
}

/// Encode the canvas as a 1-bit BMP for ESPHome `online_image` with
/// `type: BINARY`.
///
/// ESPHome reads raw bits and treats bit=1 as "draw pixel" regardless
/// of the BMP palette. To stay compatible we set bit=1 wherever the
/// on-screen pixel is black. The colour palette is set to
/// `[black, white]` to match Pillow's `mode="1"` BMP defaults so the
/// output bytes line up with the Python addon byte-for-byte.
pub fn to_bmp_bytes(img: &GrayImage) -> Vec<u8> {
    let mono = to_mono(img);
    let w = mono.width();
    let h = mono.height();
    // 1bpp packs 8 pixels per byte. BMP rows must be padded to a
    // 4-byte boundary.
    let row_bytes = w.div_ceil(8);
    let row_padded = row_bytes.div_ceil(4) * 4;
    let pixel_bytes = (row_padded * h) as usize;
    // 14 (file header) + 40 (BITMAPINFOHEADER) + 8 (2-entry palette).
    let header_size: u32 = 14 + 40 + 8;
    let file_size = header_size + pixel_bytes as u32;

    let mut out = Vec::with_capacity(file_size as usize);

    // BITMAPFILEHEADER (14 bytes)
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // reserved
    out.extend_from_slice(&header_size.to_le_bytes()); // pixel data offset

    // BITMAPINFOHEADER (40 bytes)
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(w as i32).to_le_bytes()); // width
    out.extend_from_slice(&(h as i32).to_le_bytes()); // height (positive ⇒ bottom-up)
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&1u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // compression: BI_RGB
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes()); // raw image size
    out.extend_from_slice(&0i32.to_le_bytes()); // x pels per metre
    out.extend_from_slice(&0i32.to_le_bytes()); // y pels per metre
    out.extend_from_slice(&0u32.to_le_bytes()); // colours used (0 ⇒ 2^bpp)
    out.extend_from_slice(&0u32.to_le_bytes()); // important colours

    // Palette: index 0 = black, index 1 = white. Matches Pillow's
    // `mode="1"` BMP default. ESPHome ignores the palette so the only
    // observable effect is byte-level parity with the Python addon's
    // BMP output, which simplifies cross-implementation diffing.
    out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // black (BGR + reserved)
    out.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x00]); // white

    // Pixel data: bottom-up, MSB-first within each byte. Bit=1 where
    // the source pixel is black so ESPHome draws it.
    for y in (0..h).rev() {
        let mut row = vec![0u8; row_padded as usize];
        for x in 0..w {
            let p: Luma<u8> = *mono.get_pixel(x, y);
            if p[0] < THRESHOLD {
                let bit = 7 - (x % 8);
                row[(x / 8) as usize] |= 1 << bit;
            }
        }
        out.extend_from_slice(&row);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, value: u8) -> GrayImage {
        GrayImage::from_pixel(w, h, Luma([value]))
    }

    #[test]
    fn to_mono_thresholds_at_midpoint() {
        let mut img = GrayImage::new(4, 1);
        img.put_pixel(0, 0, Luma([0])); // black stays black
        img.put_pixel(1, 0, Luma([127])); // just below threshold ⇒ black
        img.put_pixel(2, 0, Luma([128])); // exactly at threshold ⇒ white
        img.put_pixel(3, 0, Luma([255])); // white stays white
        let mono = to_mono(&img);
        assert_eq!(mono.get_pixel(0, 0)[0], 0);
        assert_eq!(mono.get_pixel(1, 0)[0], 0);
        assert_eq!(mono.get_pixel(2, 0)[0], 255);
        assert_eq!(mono.get_pixel(3, 0)[0], 255);
    }

    #[test]
    fn to_png_bytes_starts_with_png_signature() {
        let img = solid(8, 8, 255);
        let png = to_png_bytes(&img).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn to_bmp_bytes_starts_with_bm_magic() {
        let img = solid(8, 8, 255);
        let bmp = to_bmp_bytes(&img);
        assert_eq!(&bmp[..2], b"BM");
    }

    #[test]
    fn to_bmp_bytes_size_matches_header() {
        // 800x480 1-bit BMP: each row is 100 bytes (already 4-aligned).
        let img = solid(800, 480, 255);
        let bmp = to_bmp_bytes(&img);
        let expected = 14 + 40 + 8 + 100 * 480;
        assert_eq!(bmp.len(), expected);
        // File-size field at byte offset 2 must match.
        let recorded = u32::from_le_bytes([bmp[2], bmp[3], bmp[4], bmp[5]]) as usize;
        assert_eq!(recorded, expected);
    }

    #[test]
    fn to_bmp_bytes_padding_for_non_aligned_widths() {
        // Width 13 → 2 raw bytes per row → padded to 4. Height 1.
        let mut img = GrayImage::from_pixel(13, 1, Luma([255]));
        img.put_pixel(0, 0, Luma([0])); // black bit at MSB of first byte
        let bmp = to_bmp_bytes(&img);
        // header (62) + 4-byte padded row * 1 row = 66
        assert_eq!(bmp.len(), 14 + 40 + 8 + 4);
        // Inspect the row bytes (last 4 in the buffer): MSB-first means
        // the leftmost pixel sets bit 7 of byte 0.
        let row_start = bmp.len() - 4;
        assert_eq!(bmp[row_start], 0b1000_0000);
        assert_eq!(bmp[row_start + 1], 0);
        // Padding bytes must be zero.
        assert_eq!(bmp[row_start + 2], 0);
        assert_eq!(bmp[row_start + 3], 0);
    }

    #[test]
    fn to_bmp_bytes_polarity_inversion_vs_canvas() {
        // White canvas ⇒ all bits 0 (ESPHome draws nothing).
        let white = solid(16, 1, 255);
        let bmp_white = to_bmp_bytes(&white);
        let row_start = bmp_white.len() - 4; // 16px row = 2 bytes + 2 pad
        assert_eq!(bmp_white[row_start], 0);
        assert_eq!(bmp_white[row_start + 1], 0);

        // Black canvas ⇒ all bits 1.
        let black = solid(16, 1, 0);
        let bmp_black = to_bmp_bytes(&black);
        let row_start = bmp_black.len() - 4;
        assert_eq!(bmp_black[row_start], 0xFF);
        assert_eq!(bmp_black[row_start + 1], 0xFF);
    }

    #[test]
    fn to_bmp_bytes_palette_is_black_then_white() {
        let img = solid(8, 1, 255);
        let bmp = to_bmp_bytes(&img);
        // Palette starts at offset 14 + 40 = 54.
        assert_eq!(&bmp[54..58], &[0x00, 0x00, 0x00, 0x00]); // black
        assert_eq!(&bmp[58..62], &[0xFF, 0xFF, 0xFF, 0x00]); // white
    }

    #[test]
    fn to_bmp_bytes_is_bottom_up() {
        // Two rows: top row black, bottom row white. Bottom-up storage
        // means the white row appears first in the pixel data.
        let mut img = GrayImage::from_pixel(8, 2, Luma([255]));
        for x in 0..8 {
            img.put_pixel(x, 0, Luma([0])); // top row black
        }
        let bmp = to_bmp_bytes(&img);
        let pixel_offset = 14 + 40 + 8;
        // First stored row = bottom of canvas = white = bits 0.
        assert_eq!(bmp[pixel_offset], 0);
        // Second stored row = top of canvas = black = bits 1.
        assert_eq!(bmp[pixel_offset + 4], 0xFF);
    }
}
