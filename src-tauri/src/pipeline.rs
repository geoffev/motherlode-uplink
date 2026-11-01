// Capture → detect → crop → dedupe → OCR → review queue.
//
// The loop samples the game at the configured rate (1–2 fps) and only hashes a
// small thumbnail of each frame. Once the picture holds still for a few frames,
// and differs from the last one read, the full-resolution frame goes through
// detection and OCR. Nothing heavier runs while the player is flying around.

use std::time::{Duration, Instant};

use chrono::Utc;
use image::{imageops, RgbaImage};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::{self, CaptureError};
use crate::dedupe::{Key, Values, Verdict};
use crate::detect::{self, LocatedRow, LOCATE_H};
use crate::model::{Field, NormRect, Reading, Side, TerminalSource};
use crate::ocr::{Charset, OcrEngine};
use crate::parse;
use crate::state::{AppState, CaptureState, CaptureStatus};
use crate::vision;
use crate::util::Guard;

const THUMB_H: u32 = 180;
const STEADY_FRAMES: u32 = 3;
/// Bits (of 256) two thumbnails may differ by and still be "the same picture".
const STEADY_BITS: u32 = 6;
/// How different a steady frame must be from the last one read to read it again.
const CHANGED_BITS: u32 = 10;
const FIELD_H: u32 = 48;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub terminal_found: bool,
    pub rows: usize,
    pub new_readings: usize,
    pub updated_readings: usize,
    pub duplicates: usize,
}

fn set_status(app: &AppHandle, state: CaptureState, message: Option<String>, source: &str) {
    let st = app.state::<AppState>();
    let next = CaptureStatus { state, message, source: source.to_string() };
    let mut cur = st.capture_status.guard();
    if *cur != next {
        *cur = next.clone();
        let _ = app.emit("capture-status", &next);
        crate::tray::set_status(app, &next);
    }
}

pub fn report_crash(app: &AppHandle, what: &str) {
    set_status(app, CaptureState::Error, Some(format!("Capture stopped unexpectedly and is restarting ({what})")), "");
}

const NOT_RUNNING: &str = "Star Citizen isn't running, so nothing is being recorded.";
const MINIMISED: &str = "Star Citizen is minimised, so nothing is being recorded.";

pub fn run_capture(app: AppHandle) {
    let mut backend: Option<Box<dyn capture::Backend>> = None;
    let mut last_thumb: Option<vision::Hash> = None;
    let mut steady = 0u32;
    let mut last_read: Option<vision::Hash> = None;
    let source = capture::GAME_EXE;

    loop {
        let st = app.state::<AppState>();
        let settings = st.settings.guard().clone();
        let period = Duration::from_secs_f32(1.0 / settings.capture_fps);

        let idle = if !settings.capture_enabled {
            Some((CaptureState::Paused, None))
        } else if !st.can_report() {
            Some((CaptureState::Idle, Some("Sign in as a Data Surveyors member to start reading".into())))
        } else {
            None
        };
        if let Some((state, msg)) = idle {
            backend = None;
            set_status(&app, state, msg, "");
            wait_for_wake(&st, Duration::from_secs(5));
            continue;
        }

        let b = backend.get_or_insert_with(capture::create);
        let t0 = Instant::now();

        match b.grab() {
            Err(e) => {
                last_thumb = None;
                steady = 0;
                let (state, msg) = match e {
                    CaptureError::GameNotRunning => (CaptureState::NoGame, NOT_RUNNING.to_string()),
                    CaptureError::GameMinimised => (CaptureState::NoGame, MINIMISED.to_string()),
                    CaptureError::Failed(e) => (CaptureState::Error, format!("Couldn't capture the Star Citizen window: {e}")),
                };
                set_status(&app, state, Some(msg), "");
                wait_for_wake(&st, Duration::from_secs(3));
                continue;
            }
            Ok(frame) => {
                let thumb = thumbnail_hash(&frame);
                steady = match last_thumb {
                    Some(prev) if vision::hamming(&prev, &thumb) <= STEADY_BITS => steady + 1,
                    _ => 1,
                };
                last_thumb = Some(thumb);
                let changed = last_read.is_none_or(|h| vision::hamming(&h, &thumb) > CHANGED_BITS);
                if steady >= STEADY_FRAMES && changed {
                    last_read = Some(thumb);
                    set_status(&app, CaptureState::Reading, None, source);
                    match process_frame(&app, &frame) {
                        Ok(o) if o.terminal_found => set_status(&app, CaptureState::Terminal, None, source),
                        Ok(_) => set_status(&app, CaptureState::Watching, None, source),
                        Err(e) => set_status(&app, CaptureState::Error, Some(e), source),
                    }
                } else if steady < STEADY_FRAMES {
                    // The picture moved: a terminal read earlier is no longer in view.
                    let cur = st.capture_status.guard().state.clone();
                    if cur != CaptureState::Watching {
                        set_status(&app, CaptureState::Watching, None, source);
                    }
                }
            }
        }
        if let Some(rest) = period.checked_sub(t0.elapsed()) {
            wait_for_wake(&st, rest);
        }
    }
}

fn wait_for_wake(st: &AppState, timeout: Duration) {
    let (lock, cv) = &st.capture_wake;
    let g = lock.guard();
    let start = *g;
    let _ = cv.wait_timeout_while(g, timeout, |n| *n == start);
}

fn thumbnail_hash(frame: &RgbaImage) -> vision::Hash {
    let w = (frame.width() as u64 * THUMB_H as u64 / frame.height().max(1) as u64).max(1) as u32;
    let small = imageops::thumbnail(frame, w, THUMB_H);
    vision::dhash(&vision::brightness(&small))
}

fn read_field(ocr: &dyn OcrEngine, gray: &image::GrayImage, rect: NormRect, charset: Charset) -> (String, f32) {
    let crop = vision::crop_gray(gray, rect);
    match ocr.read_line(&vision::prepare_line(&crop, FIELD_H), charset) {
        Ok(t) => (t.text.trim().to_string(), t.confidence),
        Err(e) => {
            log::warn!("field OCR failed: {e}");
            (String::new(), 0.0)
        }
    }
}

/// Positions between Otsu's cut and pure white. Low cuts keep thin strokes,
/// high cuts open up the font's zero so it stops reading as an 8 or a 3.
const PASS_CUTS: [f32; 3] = [0.0, 0.4, 0.7];

/// A number read at each cut; the value most passes agree on wins, and its
/// confidence drops with every pass that disagreed.
/// `wide` is the frame-wide pass's read of the same number: one more vote.
fn read_number(
    ocr: &dyn OcrEngine,
    gray: &image::GrayImage,
    rect: NormRect,
    parse: &dyn Fn(&str) -> Option<(f64, bool)>,
    wide: Option<(&str, f32)>,
) -> (String, Option<(f64, bool)>, f32) {
    let crop = vision::resize_to_height(&vision::crop_gray(gray, rect), FIELD_H);
    let t0 = vision::otsu(&crop, 90);
    let mut reads: Vec<(String, Option<(f64, bool)>, f32)> = Vec::new();
    for k in PASS_CUTS {
        let t = (t0 as f32 + k * (255.0 - t0 as f32)).round().min(254.0) as u8;
        let img = vision::white_border(&vision::to_dark_text(&crop, t), FIELD_H / 3);
        match ocr.read_line(&img, Charset::Number) {
            Ok(r) => {
                let text = r.text.trim().to_string();
                reads.push((text.clone(), parse(&text), r.confidence));
            }
            Err(e) => log::warn!("field OCR failed: {e}"),
        }
    }
    let total = reads.len().max(1) + usize::from(wide.is_some());
    if let Some((text, conf)) = wide {
        reads.push((text.to_string(), parse(text), conf));
    }
    let votes = |v: f64| reads.iter().filter(|r| r.1.is_some_and(|p| p.0 == v)).count();
    let best = reads
        .iter()
        .filter(|r| r.1.is_some())
        .max_by(|a, b| votes(a.1.unwrap().0).cmp(&votes(b.1.unwrap().0)).then(a.2.partial_cmp(&b.2).unwrap()));
    match best {
        None => (reads.first().map(|r| r.0.clone()).unwrap_or_default(), None, 0.0),
        Some(b) => {
            let agree = votes(b.1.unwrap().0) as f32 / total as f32;
            (b.0.clone(), b.1, b.2 * agree)
        }
    }
}

pub fn locate_frame(ocr: &dyn OcrEngine, gray: &image::GrayImage, catalog: &crate::model::Catalog) -> Result<Option<detect::Located>, String> {
    let (locate_img, bin) = locate_input(gray);
    let words = ocr.read_words(&bin).map_err(|e| e.to_string())?;
    Ok(detect::locate(&words, &locate_img, catalog))
}

pub fn locate_input(gray: &image::GrayImage) -> (image::GrayImage, image::GrayImage) {
    let locate_img = vision::resize_to_height(gray, LOCATE_H);
    // A window about two text lines across, at LOCATE_H.
    let bin = vision::local_text_mask(&locate_img, 18, 35, 90);
    (locate_img, bin)
}

pub fn process_frame(app: &AppHandle, frame: &RgbaImage) -> Result<ScanOutcome, String> {
    let st = app.state::<AppState>();
    let ocr = st.ocr();
    ocr.available()?;
    let catalog = st.catalog();

    let gray = vision::brightness(frame);
    let mut out = ScanOutcome { terminal_found: false, rows: 0, new_readings: 0, updated_readings: 0, duplicates: 0 };
    let Some(located) = locate_frame(ocr.as_ref(), &gray, &catalog)? else { return Ok(out) };
    out.terminal_found = true;
    out.rows = located.rows.len();

    // The location on screen first, then the terminal last confirmed.
    let last_terminal = st.settings.guard().last_terminal_id.clone();
    let (terminal_id, terminal_source) = match (located.terminal_ids.first(), &last_terminal) {
        (Some(t), _) => (Some(t.clone()), Some(TerminalSource::Screen)),
        (None, Some(t)) => (Some(t.clone()), Some(TerminalSource::Last)),
        _ => (None, None),
    };

    let crops = st.crops();
    let screen_id = uuid::Uuid::new_v4().to_string();
    let dir = crops.screen_dir(&screen_id);
    let mut frame_path: Option<Option<String>> = None;
    let now = Utc::now();

    for row in &located.rows {
        let row_hash = vision::dhash_wide(&vision::crop_gray(&gray, row.row));
        if st.recent.guard().seen_crop(&row_hash) {
            out.duplicates += 1;
            continue;
        }
        let mut reading = read_row(ocr.as_ref(), &gray, row, &catalog);
        reading.screen_id = screen_id.clone();
        reading.terminal_id = terminal_id.clone();
        reading.terminal_source = terminal_source;
        reading.location_text = located.location_text.clone();
        reading.created_at = now;
        reading.updated_at = now;
        reading.frame_width = frame.width();
        reading.frame_height = frame.height();

        let key = Key {
            terminal: terminal_id.clone(),
            commodity: reading.commodity.value.clone().unwrap_or_else(|| crate::util::normalise(&reading.commodity.text)),
            side: row.side,
        };
        let values =
            Values { price: reading.price.value, quantity: reading.quantity.value, sizes: reading.sizes.value.clone().unwrap_or_default() };
        let verdict = st.recent.guard().check(&key, &values);
        let existing = match &verdict {
            Verdict::Duplicate(id) => {
                // Remember this crop too, so the next pass skips it before OCR.
                st.recent.guard().record(key, values, row_hash, id.clone());
                // Still on screen: the prices were seen now.
                touch(app, id, now);
                out.duplicates += 1;
                continue;
            }
            Verdict::Update(id) => st.readings.guard().get(id).cloned(),
            Verdict::New => None,
        };

        if let Some(prev) = &existing {
            // Keep the queue entry (its id and age); the UI keeps any field the
            // player already edited.
            crops.discard(prev);
            reading.id = prev.id.clone();
            reading.created_at = prev.created_at;
            reading.revision = prev.revision + 1;
            reading.terminal_id = prev.terminal_id.clone().or(reading.terminal_id);
            out.updated_readings += 1;
        } else {
            out.new_readings += 1;
        }

        // Crops of the colour frame, for the review UI and for retraining.
        reading.frame = frame_path.get_or_insert_with(|| crops.save_frame(&dir, frame)).clone();
        let save = |name: &str, rect: Option<NormRect>| {
            rect.and_then(|r| crops.save_crop(&dir, &format!("{}-{name}", reading.id), &vision::crop_rgba(frame, r)))
        };
        reading.commodity.crop = save("commodity", reading.commodity.rect);
        reading.price.crop = save("price", reading.price.rect);
        reading.quantity.crop = save("quantity", reading.quantity.rect);
        reading.sizes.crop = save("sizes", reading.sizes.rect);

        st.recent.guard().record(key, values, row_hash, reading.id.clone());
        st.readings.guard().upsert(reading.clone());
        let _ = app.emit("reading", &reading);
    }
    st.readings.guard().save();
    Ok(out)
}

fn touch(app: &AppHandle, id: &str, at: chrono::DateTime<Utc>) {
    let st = app.state::<AppState>();
    let updated = {
        let mut readings = st.readings.guard();
        let Some(mut r) = readings.get(id).cloned() else { return };
        r.updated_at = at;
        readings.upsert(r.clone());
        r
    };
    let _ = app.emit("reading", &updated);
}

/// OCR's the row's fields at full resolution; falls back to the locate pass's
/// text (at reduced confidence) where the close read fails.
fn read_row(ocr: &dyn OcrEngine, gray: &image::GrayImage, row: &LocatedRow, catalog: &crate::model::Catalog) -> Reading {
    let (name_text, name_conf) = read_field(ocr, gray, row.name, Charset::Name);
    let located_score = row.commodity.as_ref().map(|m| m.score).unwrap_or(0.0);
    let (best, text, conf) = match detect::match_commodity(&name_text, catalog) {
        Some(m) if m.score >= located_score => {
            let c = name_conf.min(m.score);
            (Some(m), name_text, c)
        }
        _ => (row.commodity.clone(), row.name_text.clone(), row.confidence.min(located_score) * 0.9),
    };
    // A shared game name: no value until the player picks one of the candidates.
    let (value, candidates) = match best {
        Some(m) if m.ids.len() == 1 => (m.ids.into_iter().next(), vec![]),
        Some(m) => (None, m.ids),
        None => (None, vec![]),
    };
    let commodity = Field { value, text, confidence: if candidates.is_empty() { conf } else { 0.0 }, crop: None, rect: Some(row.name) };

    let number = |rect: Option<NormRect>, fallback: &Option<String>, parse: &dyn Fn(&str) -> Option<(f64, bool)>| {
        let Some(rect) = rect else { return Field::empty() };
        let (text, parsed, conf) = read_number(ocr, gray, rect, parse, fallback.as_deref().map(|t| (t, row.confidence)));
        let (value, conf) = match parsed {
            Some((v, exact)) => (Some(v), if exact { conf } else { conf * 0.5 }),
            None => match fallback.as_deref().and_then(parse) {
                Some((v, exact)) => (Some(v), row.confidence * if exact { 0.5 } else { 0.25 }),
                None => (None, 0.0),
            },
        };
        Field { value, text, confidence: conf, crop: None, rect: Some(rect) }
    };
    let mut price_f = number(row.price, &row.price_text, &|t| parse::parse_price(t).map(|(v, e)| (v as f64, e)));
    // The close read sometimes picks up the ¤ as an extra leading digit
    // ("37,384" for "7,384"): when the frame-wide read is that minus its first
    // digit, it wins, with less confidence.
    if let (Some(v), Some((loc, _))) = (price_f.value, row.price_text.as_deref().and_then(parse::parse_price)) {
        let (close, wide) = ((v as i64).to_string(), loc.to_string());
        if close.len() == wide.len() + 1 && close.ends_with(&wide) {
            price_f.value = Some(loc as f64);
            price_f.confidence = price_f.confidence.min(0.4);
        }
    }
    let price = Field {
        value: price_f.value.map(|v| v as i64),
        text: price_f.text,
        confidence: price_f.confidence,
        crop: None,
        rect: price_f.rect,
    };

    let quantity = number(row.quantity, &row.quantity_text, &parse::parse_quantity);
    let read_sizes = |rect, button_band| {
        if button_band {
            let crop = vision::resize_to_height(&vision::crop_gray(gray, rect), 96);
            let mask = vision::to_dark_text(&crop, vision::otsu(&crop, 90));
            let clean = vision::white_border(&vision::remove_edge_ink(&mask), 32);
            match ocr.read_line(&clean, Charset::Sizes) {
                Ok(r) => (r.text, r.confidence),
                Err(e) => { log::warn!("size OCR failed: {e}"); (String::new(), 0.0) }
            }
        } else {
            read_field(ocr, gray, rect, Charset::Sizes)
        }
    };
    let mut sizes = match row.sizes {
        None => Field { value: Some(row.sizes_found.clone()), text: String::new(), confidence: row.confidence * 0.5, crop: None, rect: None },
        Some(rect) => {
            let (text, conf) = read_sizes(rect, row.sizes_are_button_band);
            let parsed = parse::parse_sizes(&text);
            let (mut value, mut conf) = if parsed.is_empty() { (row.sizes_found.clone(), row.confidence * 0.5) } else { (parsed, conf) };
            if row.side == Side::Sell && row.sizes_found.iter().any(|s| !value.contains(s)) {
                value.extend(&row.sizes_found);
                value.sort_unstable();
                value.dedup();
                conf = conf.min(row.confidence * 0.5);
            }
            Field { value: Some(value), text, confidence: conf, crop: None, rect: Some(rect) }
        }
    };

    for rect in &row.additional_size_bands {
        let (text, conf) = read_sizes(*rect, true);
        let extra = parse::parse_sizes(&text);
        if !extra.is_empty() {
            let values = sizes.value.get_or_insert_with(Vec::new);
            values.extend(extra);
            values.sort_unstable();
            values.dedup();
            sizes.confidence = sizes.confidence.min(conf);
            sizes.text.push_str(&format!("; {text}"));
            // Keep one card-local review crop; never join bands across cards.
            sizes.rect.get_or_insert(*rect);
        }
    }

    Reading {
        id: uuid::Uuid::new_v4().to_string(),
        screen_id: String::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        revision: 0,
        side: row.side,
        terminal_id: None,
        terminal_source: None,
        location_text: None,
        commodity,
        commodity_candidates: candidates,
        price,
        quantity,
        sizes,
        frame: None,
        frame_width: 0,
        frame_height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{OcrError, OcrText, OcrWord};

    struct FirstVariantOcr;

    #[test]
    #[ignore = "requires the local Orison screenshot in samples and prepared OCR"]
    fn real_sell_sample() {
        let resources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let ocr = crate::ocr::Tesseract::find(None, Some(&resources)).unwrap();
        let frame = image::open(resources.join("../../samples/1920x1080_orison_demand-and-stock.png")).unwrap().to_rgba8();
        let gray = vision::brightness(&frame);
        let catalog = crate::model::Catalog {
            commodities: ["Savrilium", "Tungsten", "Organics"].into_iter().map(|name| crate::model::Commodity {
                id: name.to_lowercase(), name: name.into(), game_name: Some(name.into()), display_name: None,
            }).collect(),
            ..Default::default()
        };
        let located = locate_frame(&ocr, &gray, &catalog).unwrap().unwrap();
        assert_eq!(located.rows[0].additional_size_bands.len(), 1, "keep unreadable quality-variant bands for close OCR");
        let readings: Vec<_> = located.rows.iter().map(|r| read_row(&ocr, &gray, r, &catalog)).collect();
        let sell: Vec<_> = readings.iter().filter(|r| r.side == Side::Sell).collect();
        assert_eq!(sell.len(), 2);
        assert_eq!(sell[0].sizes.value, Some(vec![1, 4]));
        assert_eq!(sell[1].sizes.value, Some(vec![1, 2]));
        assert!(sell.iter().all(|r| r.quantity.value.is_none()));
        assert!(readings.iter().filter(|r| r.side == Side::Buy).all(|r| r.sizes.value == Some(vec![])));
    }

    impl OcrEngine for FirstVariantOcr {
        fn name(&self) -> String { "fixture".into() }
        fn read_line(&self, _: &image::GrayImage, charset: Charset) -> Result<OcrText, OcrError> {
            let text = match charset {
                Charset::Name => "SAVRILIUM",
                Charset::Number => "135876",
                Charset::Sizes => "1x1 4x2",
            };
            Ok(OcrText { text: text.into(), confidence: 1.0 })
        }
        fn read_words(&self, _: &image::GrayImage) -> Result<Vec<OcrWord>, OcrError> { unreachable!() }
    }

    #[test]
    fn sell_size_close_read_retains_other_quality_variants() {
        let rect = NormRect { x: 0.0, y: 0.0, w: 1.0, h: 0.2 };
        let other_card = NormRect { y: 0.7, ..rect };
        let row = LocatedRow {
            side: Side::Sell, row: rect, name: rect, name_text: "SAVRILIUM".into(), commodity: None,
            price: Some(rect), price_text: Some("135876".into()), quantity: None, quantity_text: None,
            sizes: Some(rect), sizes_are_button_band: false, additional_size_bands: vec![other_card], sizes_found: vec![1, 4, 16], confidence: 1.0,
        };
        let reading = read_row(&FirstVariantOcr, &image::GrayImage::new(100, 100), &row, &crate::model::Catalog::default());
        assert_eq!(reading.sizes.value, Some(vec![1, 4, 16]));
        assert!(reading.quantity.value.is_none());
        assert_eq!(reading.sizes.rect, Some(rect), "review crop must not span quality-variant cards");
        assert!(reading.sizes.confidence <= 0.5, "sizes without a close-read vote need review");
    }
}
