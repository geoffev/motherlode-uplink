// Image helpers: brightness, Otsu thresholding, perceptual hashes, crops.
// Same ideas as lib/screen-scan/vision.ts on the website, in Rust.

use image::imageops::{self, FilterType};
use image::{GrayImage, Luma, RgbaImage};

use crate::model::NormRect;

/// Brightest channel per pixel. Terminal text is bright over a dark panel, and
/// coloured text (gold, teal) stays bright where plain luma would dim it.
pub fn brightness(img: &RgbaImage) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        Luma([p[0].max(p[1]).max(p[2])])
    })
}

pub fn resize_to_height(img: &GrayImage, h: u32) -> GrayImage {
    let w = ((img.width() as f64) * (h as f64) / (img.height().max(1) as f64)).round().max(1.0) as u32;
    let filter = if h < img.height() { FilterType::Triangle } else { FilterType::CatmullRom };
    imageops::resize(img, w, h, filter)
}

/// Otsu's threshold, never below `floor` so a dark frame can't push the cut into the background.
pub fn otsu(img: &GrayImage, floor: u8) -> u8 {
    let mut hist = [0u64; 256];
    for p in img.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let n: u64 = hist.iter().sum();
    if n == 0 {
        return floor;
    }
    let sum: f64 = hist.iter().enumerate().map(|(t, &c)| t as f64 * c as f64).sum();
    let (mut sum_b, mut w_b, mut best, mut best_t) = (0f64, 0u64, 0f64, 128u8);
    for t in 0..256 {
        w_b += hist[t];
        if w_b == 0 {
            continue;
        }
        let w_f = n - w_b;
        if w_f == 0 {
            break;
        }
        sum_b += t as f64 * hist[t] as f64;
        let m_b = sum_b / w_b as f64;
        let m_f = (sum - sum_b) / w_f as f64;
        let between = w_b as f64 * w_f as f64 * (m_b - m_f).powi(2);
        if between > best {
            best = between;
            best_t = t as u8;
        }
    }
    best_t.max(floor)
}

pub fn to_dark_text(img: &GrayImage, t: u8) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        Luma([if img.get_pixel(x, y).0[0] > t { 0 } else { 255 }])
    })
}

/// Bright text → black on white, judged against each pixel's surroundings
/// rather than one cut for the whole frame. A highlighted row (bright teal
/// card behind white text) would otherwise turn into one black block.
pub fn local_text_mask(img: &GrayImage, radius: u32, contrast: u8, floor: u8) -> GrayImage {
    let (w, h) = img.dimensions();
    // Integral image, one extra row and column of zeros.
    let iw = (w + 1) as usize;
    let mut sum = vec![0u64; iw * (h + 1) as usize];
    for y in 0..h as usize {
        let mut row = 0u64;
        for x in 0..w as usize {
            row += img.as_raw()[y * w as usize + x] as u64;
            sum[(y + 1) * iw + x + 1] = sum[y * iw + x + 1] + row;
        }
    }
    GrayImage::from_fn(w, h, |x, y| {
        let (x0, y0) = (x.saturating_sub(radius) as usize, y.saturating_sub(radius) as usize);
        let (x1, y1) = ((x + radius + 1).min(w) as usize, (y + radius + 1).min(h) as usize);
        let total = sum[y1 * iw + x1] + sum[y0 * iw + x0] - sum[y0 * iw + x1] - sum[y1 * iw + x0];
        let mean = total as f32 / ((x1 - x0) * (y1 - y0)) as f32;
        let v = img.get_pixel(x, y).0[0];
        Luma([if v >= floor && v as f32 > mean + contrast as f32 { 0 } else { 255 }])
    })
}

/// Adds a white margin; Tesseract misreads glyphs that touch the edge.
pub fn white_border(img: &GrayImage, px: u32) -> GrayImage {
    let mut out = GrayImage::from_pixel(img.width() + px * 2, img.height() + px * 2, Luma([255]));
    imageops::overlay(&mut out, img, px as i64, px as i64);
    out
}

pub fn prepare_line(img: &GrayImage, target_h: u32) -> GrayImage {
    let scaled = resize_to_height(img, target_h);
    let t = otsu(&scaled, 90);
    white_border(&to_dark_text(&scaled, t), target_h / 3)
}

pub fn to_pixels(r: NormRect, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let x = ((r.x * w as f32).round() as u32).min(w.saturating_sub(1));
    let y = ((r.y * h as f32).round() as u32).min(h.saturating_sub(1));
    let cw = ((r.w * w as f32).round() as u32).clamp(1, w - x);
    let ch = ((r.h * h as f32).round() as u32).clamp(1, h - y);
    (x, y, cw, ch)
}

pub fn crop_rgba(img: &RgbaImage, r: NormRect) -> RgbaImage {
    let (x, y, w, h) = to_pixels(r, img.width(), img.height());
    imageops::crop_imm(img, x, y, w, h).to_image()
}

pub fn crop_gray(img: &GrayImage, r: NormRect) -> GrayImage {
    let (x, y, w, h) = to_pixels(r, img.width(), img.height());
    imageops::crop_imm(img, x, y, w, h).to_image()
}

/// 256-bit difference hash (17×16 gradient signs). Robust to scale and mild
/// brightness changes; used for frame stability and duplicate crops.
pub type Hash = [u64; 4];

pub fn dhash(img: &GrayImage) -> Hash {
    let small = imageops::resize(img, 17, 16, FilterType::Triangle);
    let mut h = [0u64; 4];
    let mut bit = 0usize;
    for y in 0..16 {
        for x in 0..16 {
            if small.get_pixel(x, y).0[0] < small.get_pixel(x + 1, y).0[0] {
                h[bit / 64] |= 1 << (bit % 64);
            }
            bit += 1;
        }
    }
    h
}

/// Same idea on a 65×4 grid, for wide single-line crops (a terminal row):
/// a square grid would squash a changed digit into one or two cells.
pub fn dhash_wide(img: &GrayImage) -> Hash {
    let small = imageops::resize(img, 65, 4, FilterType::Triangle);
    let mut h = [0u64; 4];
    for y in 0..4 {
        for x in 0..64 {
            if small.get_pixel(x, y).0[0] < small.get_pixel(x + 1, y).0[0] {
                h[y as usize] |= 1 << x;
            }
        }
    }
    h
}

pub fn hamming(a: &Hash, b: &Hash) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dhash_is_stable_across_scale() {
        let img = GrayImage::from_fn(320, 180, |x, y| Luma([((x * 7 + y * 3) % 255) as u8]));
        let big = imageops::resize(&img, 1280, 720, FilterType::Triangle);
        assert!(hamming(&dhash(&img), &dhash(&big)) <= 12);
    }

    #[test]
    fn otsu_splits_bimodal() {
        let img = GrayImage::from_fn(100, 10, |x, _| Luma([if x < 70 { 20 } else { 220 }]));
        let t = otsu(&img, 0);
        assert!((20..220).contains(&t));
    }
}
