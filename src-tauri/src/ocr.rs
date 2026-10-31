// OCR behind a swappable trait. Tesseract runs as a child process, so there are
// no native libraries to link; a custom model can replace it as another `OcrEngine`.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::GrayImage;

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

pub struct Tesseract {
    bin: PathBuf,
}

impl Tesseract {
    /// Settings path first, then PATH, then the usual Windows install location.
    pub fn find(configured: Option<&Path>) -> Option<Tesseract> {
        let candidates = configured
            .map(Path::to_path_buf)
            .into_iter()
            .chain(which::which("tesseract").ok())
            .chain(cfg!(windows).then(|| PathBuf::from(r"C:\Program Files\Tesseract-OCR\tesseract.exe")));
        candidates.into_iter().find(|p| p.is_file()).map(|bin| Tesseract { bin })
    }

    pub fn verify(path: &Path) -> Result<String, String> {
        if !path.is_file() {
            return Err(format!("{} doesn't exist", path.display()));
        }
        let mut cmd = Command::new(path);
        cmd.arg("--version");
        hide_console(&mut cmd);
        let out = cmd.output().map_err(|e| format!("Could not run {}: {e}", path.display()))?;
        // Older builds print the version on stderr.
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let first = text.lines().next().unwrap_or("").trim().to_string();
        if out.status.success() && first.to_lowercase().starts_with("tesseract") {
            Ok(first)
        } else {
            Err(format!("{} isn't Tesseract", path.display()))
        }
    }

    fn run(&self, img: &GrayImage, psm: &str, whitelist: Option<&str>) -> Result<Vec<OcrWord>, OcrError> {
        let file = tempfile::Builder::new()
            .prefix("mlr-ocr-")
            .suffix(".png")
            .tempfile()
            .map_err(|e| OcrError::Failed(e.to_string()))?;
        img.save_with_format(file.path(), image::ImageFormat::Png)
            .map_err(|e| OcrError::Failed(e.to_string()))?;

        let mut cmd = Command::new(&self.bin);
        cmd.arg(file.path()).arg("stdout").args(["--psm", psm, "-l", "eng"]);
        if let Some(w) = whitelist {
            cmd.arg("-c").arg(format!("tessedit_char_whitelist={w}"));
        }
        cmd.args(["-c", "preserve_interword_spaces=1", "tsv"]);
        hide_console(&mut cmd);
        let out = cmd.output().map_err(|e| OcrError::Unavailable(e.to_string()))?;
        if !out.status.success() {
            return Err(OcrError::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout)))
    }
}

impl OcrEngine for Tesseract {
    fn name(&self) -> String {
        format!("Tesseract ({})", self.bin.display())
    }

    fn read_line(&self, img: &GrayImage, charset: Charset) -> Result<OcrText, OcrError> {
        let whitelist = match charset {
            Charset::Name => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-'() ",
            // The unit letters too, or Tesseract forces "/SCU" into digits.
            Charset::Number => "0123456789.,KkM/SCU",
            Charset::Sizes => "0123456789 ",
        };
        let words = self.run(img, "7", Some(whitelist))?;
        Ok(join_words(&words))
    }

    fn read_words(&self, img: &GrayImage) -> Result<Vec<OcrWord>, OcrError> {
        self.run(img, "11", None)
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
        .skip(1)
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

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

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

pub fn create(tesseract_path: Option<&Path>) -> std::sync::Arc<dyn OcrEngine> {
    match Tesseract::find(tesseract_path) {
        Some(t) => std::sync::Arc::new(t),
        None => std::sync::Arc::new(Unavailable(
            "Tesseract was not found. Install it, or set its path in Settings.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_refuses_other_programs() {
        assert!(Tesseract::verify(Path::new("definitely/not/here/tesseract.exe")).is_err());
        // git prints "git version …" for --version: a real program that isn't Tesseract.
        if let Ok(git) = which::which("git") {
            assert!(Tesseract::verify(&git).is_err());
        }
        if let Some(t) = Tesseract::find(None) {
            assert!(Tesseract::verify(&t.bin).unwrap().starts_with("tesseract"));
        }
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
