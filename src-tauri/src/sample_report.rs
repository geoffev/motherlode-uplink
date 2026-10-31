// Runs OCR engines through the real pipeline on the screenshots in samples/
// and scores them against the values on screen. Not part of the normal test
// run (it needs the engines installed):
//
//   cargo test sample_report -- --ignored --nocapture
//
// Each engine's raw words are written to target/sample-words-<engine>-<file>.txt.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::detect;
use crate::model::{Catalog, Commodity, CommodityTerminal, Side};
use crate::ocr::OcrEngine;
use crate::vision;

/// What each sample really shows: (side, commodity, price, stock, cargo sizes). A commodity sharing its game name with others ("Construction
/// Materials") counts as found when it's among the candidates.
type Expected = (Side, &'static str, i64, Option<f64>, Option<&'static [u32]>);

const ALL_SIZES: &[u32] = &[1, 2, 4, 8, 16, 24, 32];

/// (terminal the location should match, if any; rows).
fn expected(file: &str) -> Option<(Option<&'static str>, Vec<Expected>)> {
    match file {
        "1920x1080_orison_demand-and-stock.png" => Some((
            Some("admin-orison"),
            vec![
                (Side::Sell, "savrilium", 135876, None, None),
                (Side::Sell, "tungsten", 10260, None, None),
                (Side::Buy, "organics", 10232, Some(29.0), None),
            ],
        )),
        // "Select Location": no location on screen. The last row is cut off.
        "3440x1440_no-location_in-stock.png" => Some((
            None,
            vec![
                (Side::Buy, "agricium", 8379, Some(86.0), None),
                (Side::Buy, "rmc", 8640, Some(83.0), None),
                (Side::Buy, "corundum", 2822, Some(1.0), None),
                (Side::Buy, "ship-ammunition-1", 6868, Some(12000.0), None),
                (Side::Buy, "ship-ammunition-2", 7126, Some(12000.0), None),
            ],
        )),
        // Scrolled: the IN STOCK label is off screen. The last row is cut off.
        "3440x1440_levski_in-stock-sizes.png" => Some((
            Some("admin-levski"),
            vec![
                (Side::Buy, "ship-ammunition-2", 7126, Some(12000.0), Some(ALL_SIZES)),
                (Side::Buy, "ship-ammunition-3", 7384, Some(12000.0), Some(ALL_SIZES)),
                (Side::Buy, "ship-ammunition-4", 7641, Some(12000.0), Some(ALL_SIZES)),
            ],
        )),
        _ => None,
    }
}

fn catalog() -> Catalog {
    let names = [
        ("agricium", "Agricium"), ("aluminum", "Aluminum"), ("beryl", "Beryl"), ("bexalite", "Bexalite"),
        ("borase", "Borase"), ("copper", "Copper"), ("corundum", "Corundum"), ("gold", "Gold"),
        ("hephaestanite", "Hephaestanite"), ("iron", "Iron"), ("laranite", "Laranite"), ("quantainium", "Quantainium"),
        ("quartz", "Quartz"), ("taranite", "Taranite"), ("titanium", "Titanium"), ("tungsten", "Tungsten"),
        ("savrilium", "Savrilium"), ("organics", "Organics"), ("saldynium-ore", "Saldynium (Ore)"),
        ("medical-supplies", "Medical Supplies"), ("processed-food", "Processed Food"), ("hydrogen", "Hydrogen"),
        ("stims", "Stims"), ("waste", "Waste"), ("rmc", "Construction Materials"), ("cmat", "Construction Materials"),
        ("ship-ammunition-1", "Ship Ammunition - Size 1"), ("ship-ammunition-2", "Ship Ammunition - Size 2"),
        ("ship-ammunition-3", "Ship Ammunition - Size 3"), ("ship-ammunition-4", "Ship Ammunition - Size 4"),
        ("ship-ammunition-5", "Ship Ammunition - Size 5"),
    ];
    let t = |id: &str, loc: &str| CommodityTerminal { id: id.into(), name: "Admin".into(), location: loc.into(), place: None, system: "stanton".into() };
    Catalog {
        commodities: names
            .iter()
            .map(|(id, g)| Commodity { id: (*id).into(), name: (*g).into(), display_name: None, game_name: Some((*g).into()) })
            .collect(),
        terminals: vec![t("admin-orison", "Orison"), t("admin-lorville", "Lorville"), t("admin-area18", "Area18"), t("admin-levski", "Levski")],
        fetched_at: None,
    }
}

fn samples() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png" || x == "jpg")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn report(name: &str, engine: &dyn OcrEngine) {
    let cat = catalog();
    for path in samples() {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        println!("\n══ {name} · {file}");
        let frame = image::open(&path).unwrap().to_rgba8();
        let gray = vision::brightness(&frame);

        let t0 = Instant::now();
        let (locate_img, bin) = crate::pipeline::locate_input(&gray);
        bin.save(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("target/sample-bin-{file}"))).unwrap();
        let words = engine.read_words(&bin).expect("read_words");
        let locate_ms = t0.elapsed().as_millis();
        let dump: String = words.iter().map(|w| format!("{:>5.0} {:>5.0}  {:>3.0}%  {}\n", w.x0, w.y0, w.confidence * 100.0, w.text)).collect();
        let out = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("target/sample-words-{name}-{file}.txt"));
        std::fs::write(&out, dump).unwrap();
        println!("locate pass: {} words in {locate_ms} ms (dump: {})", words.len(), out.display());

        let Some(located) = detect::locate(&words, &locate_img, &cat) else {
            println!("✗ terminal NOT found");
            continue;
        };
        println!("location read: {:?} → terminals {:?}", located.location_text, located.terminal_ids);

        let t1 = Instant::now();
        let readings: Vec<_> = located.rows.iter().map(|r| crate::pipeline::read_row_for_test(engine, &gray, r, &cat)).collect();
        let fields_ms = t1.elapsed().as_millis();
        println!("field reads: {} rows in {fields_ms} ms", readings.len());
        println!("  {:<5} {:<22} {:<18} {:>9} {:>5}  {:>8} {:>5}  sizes", "side", "name read", "commodity", "price", "conf", "stock", "conf");
        for r in &readings {
            println!(
                "  {:<5} {:<22} {:<18} {:>9} {:>4.0}%  {:>8} {:>4.0}%  {:?}",
                format!("{:?}", r.side).to_lowercase(),
                r.commodity.text.chars().take(22).collect::<String>(),
                r.commodity.value.clone().unwrap_or_else(|| format!("? {:?}", r.commodity_candidates)),
                r.price.value.map(|v| v.to_string()).unwrap_or("—".into()),
                r.price.confidence * 100.0,
                r.quantity.value.map(|v| v.to_string()).unwrap_or("—".into()),
                r.quantity.confidence * 100.0,
                r.sizes.value.clone().unwrap_or_default(),
            );
        }

        if let Some((terminal, want)) = expected(&file) {
            let mut ok = 0;
            let total = want.len() * 2 + 1;
            if located.terminal_ids.first().map(String::as_str) == terminal {
                ok += 1;
            } else {
                println!("  ✗ terminal: expected {terminal:?}, got {:?}", located.terminal_ids.first());
            }
            for (side, id, price, stock, sizes) in &want {
                let exact = |r: &&crate::model::Reading| r.side == *side && r.commodity.value.as_deref() == Some(*id);
                let candidate = |r: &&crate::model::Reading| r.side == *side && r.commodity_candidates.iter().any(|c| c == id);
                let Some(r) = readings.iter().find(exact).or_else(|| readings.iter().find(candidate)) else {
                    println!("  ✗ missing {side:?} {id}");
                    continue;
                };
                ok += 1;
                let got_sizes = r.sizes.value.clone().unwrap_or_default();
                let right = r.price.value == Some(*price)
                    && (stock.is_none() || r.quantity.value == *stock)
                    && sizes.is_none_or(|s| got_sizes == s);
                if right {
                    ok += 1;
                } else {
                    println!(
                        "  ✗ wrong values for {side:?} {id}: expected {price} / {stock:?} / {sizes:?}, got {:?} / {:?} / {got_sizes:?}",
                        r.price.value, r.quantity.value
                    );
                }
            }
            let extra = readings.len().saturating_sub(want.len());
            println!("score: {ok}/{total} (terminal, each commodity found, its values right){}", if extra > 0 { format!(", {extra} extra rows") } else { String::new() });
        }
        println!("total time: {} ms", t0.elapsed().as_millis());
    }
}

#[test]
#[ignore]
fn sample_report_tesseract() {
    let engine = crate::ocr::Tesseract::find(None).expect("Tesseract not installed");
    report("tesseract", &engine);
}

/// Writes review-queue fixtures for the wiki screenshots (docs-screens/): the
/// readings the real pipeline makes from two samples, with their crops and a
/// downscaled frame, plus the catalog they refer to.
///
///   cargo test docs_fixtures -- --ignored
#[test]
#[ignore]
fn docs_fixtures() {
    let engine = crate::ocr::Tesseract::find(None).expect("Tesseract not installed");
    let cat = catalog();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs-screens/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let mut readings = Vec::new();
    let now = chrono::Utc::now();
    for (n, (file, terminal)) in
        [("3440x1440_levski_in-stock-sizes.png", "admin-levski"), ("1920x1080_orison_demand-and-stock.png", "admin-orison")].iter().enumerate()
    {
        let frame = image::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples").join(file)).unwrap().to_rgba8();
        let gray = vision::brightness(&frame);
        let located = crate::pipeline::locate_frame(&engine, &gray, &cat).unwrap().expect("terminal");
        let stem = file.trim_end_matches(".png");
        let small = image::imageops::thumbnail(&frame, 1600, 1600 * frame.height() / frame.width());
        image::DynamicImage::ImageRgba8(small).to_rgb8().save(out.join(format!("{stem}-frame.jpg"))).unwrap();
        for (i, row) in located.rows.iter().enumerate() {
            let mut r = crate::pipeline::read_row_for_test(&engine, &gray, row, &cat);
            let id = format!("{stem}-{i}");
            r.id = id.clone();
            r.screen_id = stem.to_string();
            r.terminal_id = Some(terminal.to_string());
            r.terminal_source = Some(crate::model::TerminalSource::Screen);
            r.location_text = located.location_text.clone();
            r.created_at = now - chrono::Duration::minutes(3 + n as i64 * 20);
            r.updated_at = r.created_at;
            r.frame = Some(format!("/docs-screens/fixtures/{stem}-frame.jpg"));
            r.frame_width = frame.width();
            r.frame_height = frame.height();
            let save = |name: &str, rect: Option<crate::model::NormRect>| {
                rect.map(|rect| {
                    vision::crop_rgba(&frame, rect).save(out.join(format!("{id}-{name}.png"))).unwrap();
                    format!("/docs-screens/fixtures/{id}-{name}.png")
                })
            };
            r.commodity.crop = save("commodity", r.commodity.rect);
            r.price.crop = save("price", r.price.rect);
            r.quantity.crop = save("quantity", r.quantity.rect);
            r.sizes.crop = save("sizes", r.sizes.rect);
            readings.push(r);
        }
    }
    std::fs::write(out.join("readings.json"), serde_json::to_string_pretty(&readings).unwrap()).unwrap();
    std::fs::write(out.join("catalog.json"), serde_json::to_string_pretty(&cat).unwrap()).unwrap();
    println!("{} readings written to {}", readings.len(), out.display());
}
