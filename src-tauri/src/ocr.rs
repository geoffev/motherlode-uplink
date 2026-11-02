// OCR behind a swappable trait, using the statically linked Tesseract C API.

use std::path::Path;

use image::GrayImage;
use sha2::{Digest, Sha256};
use std::io::Read;

const MODEL_SHA256: &str = "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2";
const MAX_MODEL_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
pub struct OcrText {
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct OcrWord {
    pub text: String,
    pub confidence: f32,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl OcrWord {
    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }
    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
}

/// What a crop is expected to contain; narrows the recogniser's alphabet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    Name,
    Number,
    Sizes,
}

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("OCR engine unavailable: {0}")]
    Unavailable(String),
    #[error("OCR failed: {0}")]
    Failed(String),
}

pub trait OcrEngine: Send + Sync {
    fn name(&self) -> String;
    fn available(&self) -> Result<(), String> {
        Ok(())
    }
    fn read_line(&self, img: &GrayImage, charset: Charset) -> Result<OcrText, OcrError>;
    fn read_words(&self, img: &GrayImage) -> Result<Vec<OcrWord>, OcrError>;
}

// Only model bytes cross into the native API; executable paths are never accepted.
pub struct Tesseract {
    model: Vec<u8>,
}

impl Tesseract {
    pub fn find(resources: Option<&Path>) -> Option<Tesseract> {
        // Bound allocation and authenticate the exact bytes before any native parser sees them.
        let file = std::fs::File::open(resources?.join("tesseract/tessdata/eng.traineddata")).ok()?;
        let mut model = Vec::new();
        file.take(MAX_MODEL_BYTES + 1).read_to_end(&mut model).ok()?;
        if model.len() as u64 > MAX_MODEL_BYTES || format!("{:x}", Sha256::digest(&model)) != MODEL_SHA256 {
            return None;
        }
        Some(Tesseract { model })
    }

    fn run(&self, img: &GrayImage, psm: i32, whitelist: Option<&str>) -> Result<Vec<OcrWord>, OcrError> {
        if img.width() == 0 || img.height() == 0 || img.width() > 8192 || img.height() > 8192
            || u64::from(img.width()) * u64::from(img.height()) > 32_000_000 {
            return Err(OcrError::Failed("Invalid OCR image dimensions".into()));
        }
        let whitelist = std::ffi::CString::new(whitelist.unwrap_or(""))
            .map_err(|e| OcrError::Failed(e.to_string()))?;
        // SAFETY: each call owns its API instance; nothing native is shared between threads.
        // Image/model buffers remain alive until after the handle is deleted.
        unsafe {
            let api = NativeApi(ffi::TessBaseAPICreate());
            if api.0.is_null() { return Err(OcrError::Unavailable("Could not create Tesseract".into())); }
            if ffi::TessBaseAPIInit5(api.0, self.model.as_ptr().cast(), self.model.len() as i32,
                c"eng".as_ptr(), 1, std::ptr::null_mut(), 0, std::ptr::null_mut(),
                std::ptr::null_mut(), 0, 1) != 0 {
                return Err(OcrError::Unavailable("Could not load bundled English OCR model".into()));
            }
            ffi::TessBaseAPISetPageSegMode(api.0, psm);
            if ffi::TessBaseAPISetVariable(api.0, c"tessedit_char_whitelist".as_ptr(), whitelist.as_ptr()) == 0
                || ffi::TessBaseAPISetVariable(api.0, c"preserve_interword_spaces".as_ptr(), c"1".as_ptr()) == 0 {
                return Err(OcrError::Failed("Could not configure Tesseract".into()));
            }
            ffi::TessBaseAPISetImage(api.0, img.as_raw().as_ptr(), img.width() as i32, img.height() as i32, 1, img.width() as i32);
            let monitor = NativeMonitor(ffi::TessMonitorCreate());
            if monitor.0.is_null() { return Err(OcrError::Failed("Could not create OCR progress monitor".into())); }
            // Tesseract checks this deadline during recognition; it is not an OS sandbox.
            ffi::TessMonitorSetDeadlineMSecs(monitor.0, 5_000);
            if ffi::TessBaseAPIRecognize(api.0, monitor.0) != 0 {
                return Err(OcrError::Failed("Tesseract recognition failed".into()));
            }
            let text = NativeText(ffi::TessBaseAPIGetTsvText(api.0, 0));
            if text.0.is_null() { return Err(OcrError::Failed("Tesseract returned no text".into())); }
            let tsv = std::ffi::CStr::from_ptr(text.0).to_string_lossy();
            Ok(parse_tsv(&tsv))
        }
    }
}

struct NativeApi(*mut std::ffi::c_void);
impl Drop for NativeApi {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this handle was allocated by TessBaseAPICreate and has one owner.
            unsafe { ffi::TessBaseAPIDelete(self.0) }
        }
    }
}
struct NativeText(*mut std::ffi::c_char);
impl Drop for NativeText {
    fn drop(&mut self) {
        // SAFETY: this string was allocated by TessBaseAPIGetTsvText and has one owner.
        unsafe { ffi::TessDeleteText(self.0) }
    }
}

struct NativeMonitor(*mut std::ffi::c_void);
impl Drop for NativeMonitor {
    fn drop(&mut self) {
        // SAFETY: the monitor is owned by this call and recognition has returned.
        unsafe { ffi::TessMonitorDelete(self.0) }
    }
}

// Signatures from the pinned Tesseract 5.5.4 include/tesseract/capi.h.
mod ffi {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        pub fn TessVersion() -> *const c_char;
        pub fn TessBaseAPICreate() -> *mut c_void;
        pub fn TessBaseAPIDelete(handle: *mut c_void);
        pub fn TessBaseAPIInit5(handle: *mut c_void, data: *const c_char, size: c_int,
            language: *const c_char, mode: c_int, configs: *mut *mut c_char,
            configs_size: c_int, vars: *mut *mut c_char, values: *mut *mut c_char,
            vars_size: usize, non_debug_only: c_int) -> c_int;
        pub fn TessBaseAPISetPageSegMode(handle: *mut c_void, mode: c_int);
        pub fn TessBaseAPISetVariable(handle: *mut c_void, name: *const c_char, value: *const c_char) -> c_int;
        pub fn TessBaseAPISetImage(handle: *mut c_void, data: *const u8, width: c_int,
            height: c_int, bytes_per_pixel: c_int, bytes_per_line: c_int);
        pub fn TessBaseAPIRecognize(handle: *mut c_void, monitor: *mut c_void) -> c_int;
        pub fn TessBaseAPIGetTsvText(handle: *mut c_void, page: c_int) -> *mut c_char;
        pub fn TessDeleteText(text: *const c_char);
        pub fn TessMonitorCreate() -> *mut c_void;
        pub fn TessMonitorDelete(monitor: *mut c_void);
        pub fn TessMonitorSetDeadlineMSecs(monitor: *mut c_void, deadline: c_int);
    }
}

impl OcrEngine for Tesseract {
    fn name(&self) -> String {
        // SAFETY: TessVersion returns a non-null, static NUL-terminated string.
        let version = unsafe { std::ffi::CStr::from_ptr(ffi::TessVersion()) }.to_string_lossy();
        format!("Tesseract {version} (native, bundled English model)")
    }

    fn read_line(&self, img: &GrayImage, charset: Charset) -> Result<OcrText, OcrError> {
        let whitelist = match charset {
            Charset::Name => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-'() ",
            // The unit letters too, or Tesseract forces "/SCU" into digits.
            Charset::Number => "0123456789.,KkM/SCU",
            Charset::Sizes => "0123456789 ",
        };
        let words = self.run(img, 7, Some(whitelist))?;
        Ok(join_words(&words))
    }

    fn read_words(&self, img: &GrayImage) -> Result<Vec<OcrWord>, OcrError> {
        self.run(img, 11, None)
    }
}

pub fn join_words(words: &[OcrWord]) -> OcrText {
    let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    let confidence = words.iter().map(|w| w.confidence).fold(f32::INFINITY, f32::min);
    OcrText { text, confidence: if confidence.is_finite() { confidence } else { 0.0 } }
}

/// Tesseract TSV: level page block par line word left top width height conf text.
fn parse_tsv(tsv: &str) -> Vec<OcrWord> {
    tsv.lines()
        .filter_map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() < 12 || c[0] != "5" {
                return None;
            }
            let text = c[11].trim();
            let conf: f32 = c[10].parse().ok()?;
            if text.is_empty() || conf < 0.0 {
                return None;
            }
            let n = |i: usize| c[i].parse::<f32>().ok();
            let (x, y, w, h) = (n(6)?, n(7)?, n(8)?, n(9)?);
            Some(OcrWord { text: text.to_string(), confidence: conf / 100.0, x0: x, y0: y, x1: x + w, y1: y + h })
        })
        .collect()
}

pub struct Unavailable(pub String);

impl OcrEngine for Unavailable {
    fn name(&self) -> String {
        "None".into()
    }
    fn available(&self) -> Result<(), String> {
        Err(self.0.clone())
    }
    fn read_line(&self, _: &GrayImage, _: Charset) -> Result<OcrText, OcrError> {
        Err(OcrError::Unavailable(self.0.clone()))
    }
    fn read_words(&self, _: &GrayImage) -> Result<Vec<OcrWord>, OcrError> {
        Err(OcrError::Unavailable(self.0.clone()))
    }
}

pub fn create(resources: Option<&Path>) -> std::sync::Arc<dyn OcrEngine> {
    match Tesseract::find(resources) {
        Some(t) => std::sync::Arc::new(t),
        None => std::sync::Arc::new(Unavailable(
            "Bundled English OCR model is missing or failed its integrity check. Reinstall the app to restore OCR.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_bundled_model() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Tesseract::find(None).is_none());
        assert!(Tesseract::find(Some(dir.path())).is_none());
        let data = dir.path().join("tesseract/tessdata");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("eng.traineddata"), b"").unwrap();
        assert!(Tesseract::find(Some(dir.path())).is_none());
        std::fs::write(data.join("eng.traineddata"), b"tampered model").unwrap();
        assert!(Tesseract::find(Some(dir.path())).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn native_ocr_loads_model_from_verbatim_resource_path() {
        // Exercise the linked library and bundled model, as tauri dev resolves them.
        // prepare:ocr supplies these resources locally and in CI.
        let resources = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .canonicalize()
            .expect("Run npm run prepare:ocr before Rust tests");
        assert!(resources.as_os_str().to_string_lossy().starts_with(r"\\?\"));
        let ocr = Tesseract::find(Some(&resources)).expect("Bundled Tesseract missing");
        assert!(!ocr.model.is_empty());
        assert!(ocr.name().contains("5.5.4"));
        let blank = GrayImage::from_pixel(200, 60, image::Luma([255]));
        assert!(ocr.read_words(&blank).expect("Bundled English model must load").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn native_ocr_recognizes_english_from_memory() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let ocr = Tesseract::find(Some(&resources)).expect("Run npm run prepare:ocr");
        // A portable raster fixture; no system fonts, executable or temporary image.
        let glyphs = [
            [17u8, 17, 17, 31, 17, 17, 17], // H
            [31, 16, 16, 30, 16, 16, 31], // E
            [16, 16, 16, 16, 16, 16, 31], // L
            [16, 16, 16, 16, 16, 16, 31], // L
            [14, 17, 17, 17, 17, 17, 14], // O
        ];
        let mut image = GrayImage::from_pixel(220, 90, image::Luma([255]));
        for (letter, rows) in glyphs.iter().enumerate() {
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        for dy in 0..6 {
                            for dx in 0..6 {
                                image.put_pixel(20 + letter as u32 * 36 + col * 6 + dx,
                                    20 + row as u32 * 6 + dy, image::Luma([0]));
                            }
                        }
                    }
                }
            }
        }
        let words = ocr.run(&image, 7, None).unwrap();
        assert_eq!(join_words(&words).text, "HELLO");
        assert!(words.iter().all(|w| w.confidence > 0.0 && w.x1 > w.x0 && w.y1 > w.y0));
        assert_eq!(ocr.read_line(&image, Charset::Name).unwrap().text, "HELLO");
        assert!(ocr.run(&GrayImage::new(0, 0), 7, None).is_err());
    }

    #[test]
    fn parses_tesseract_tsv_words() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   4\t1\t1\t1\t1\t0\t10\t10\t100\t20\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t10\t10\t40\t20\t91.5\tAGRICIUM\n\
                   5\t1\t1\t1\t1\t2\t60\t10\t50\t20\t42\t10,138\n";
        let w = parse_tsv(tsv);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].text, "AGRICIUM");
        assert!((w[1].confidence - 0.42).abs() < 1e-6);
        assert_eq!(w[1].x1, 110.0);
        let joined = join_words(&w);
        assert_eq!(joined.text, "AGRICIUM 10,138");
        assert!((joined.confidence - 0.42).abs() < 1e-6);
    }
}
