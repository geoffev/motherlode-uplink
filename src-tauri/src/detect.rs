// Finds the commodity terminal on a frame and splits it into per-commodity
// field regions. Works from the words OCR finds on the whole frame, never from
// fixed pixel positions, so any resolution, aspect ratio or UI scale reads the
// same way (lib/screen-scan/ocr-engine.ts `locatePanel` uses the same idea).
//
// The terminal shows both sides at once:
//
//   COMMODITIES                                       CURRENT BALANCE
//   YOUR INVENTORIES                     SHOP INVENTORY
//   [ ORISON          ▸]                 [BUY] [LOCAL MARKET VALUE]
//   IN DEMAND                            IN STOCK
//   ▲ SAVRILIUM        9 SCU             ORGANICS          SHOP QUANTITY
//     QUALITY 322  ¤135,876/SCU          VERY LOW INVENTORY       29 SCU
//     AVAILABLE CARGO SIZE (SCU)                            ¤10,232/SCU
//     [1 x1] [4 x2]  MAX                 AVAILABLE CARGO SIZE (SCU)
//   …                                    OUT OF STOCK  +
//
// IN DEMAND rows give sell prices and positive evidence of accepted cargo sizes
// from the player's inventory. Their SCU quantity is not terminal demand.
// IN STOCK rows are the buy side: price,
// shop quantity, cargo sizes. YOUR INVENTORIES names the location.
// The middle of the screen is the game world, so each list is bounded by its
// own labels and its right-aligned prices.

use image::GrayImage;

use crate::model::{Catalog, NormRect, Side, CARGO_SIZES};
use crate::ocr::OcrWord;
use crate::parse;
use crate::util::normalise;

/// Height the whole frame is resampled to before looking for the terminal.
/// SC's UI scales with vertical resolution, so this evens out glyph sizes.
pub const LOCATE_H: u32 = 1440;

/// Words that are labels, never part of a commodity name.
const LABELS: &[&str] = &[
    "AUEC", "AVAILABLE", "BALANCE", "BUY", "CARGO", "CATEGORY", "COMMODITIES", "CURRENT", "DEMAND", "EST", "HIGH",
    "IN", "INVENTORIES", "INVENTORY", "LOCAL", "LOW", "MARKET", "MAX", "MEDIUM", "OF", "OUT", "QUALITY", "QUANTITY",
    "SCU", "SELECT", "SELL", "SHOP", "SIZE", "STOCK", "SUB", "SUBCATEGORY", "TIME", "TOTAL", "UNLOADING", "VALUE",
    "VERY", "YOUR", "NO", "CANNOT", "LOCATION", "PLEASE", "VALID", "TRANSACTION",
];

/// A label word, allowing one OCR slip in longer ones ("CARGD", "AVAILAGLE").
fn is_label(n: &str) -> bool {
    LABELS.contains(&n) || is_inventory_word(n) || (n.len() >= 5 && LABELS.iter().any(|l| l.len() >= 5 && strsim::levenshtein(n, l) <= 1))
}

/// "INVENTORY" as OCR reads it in the small terminal font ("INVENGRY", "INVENIURY").
fn is_inventory_word(text: &str) -> bool {
    let n = normalise(text);
    n.len() >= 7 && strsim::levenshtein(&n, "INVENTORY") <= 2
}

/// Cargo-size boxes come back with their borders: "|16", "[24", "32'", "1,2", "4/8", "|B".
fn box_sizes(text: &str) -> Vec<u32> {
    // Counts below/inside a button (x2) are not additional container sizes.
    let text = text.split(['x', 'X', '×']).next().unwrap_or("");
    let cleaned: String = text
        .chars()
        .filter(|c| !"|[](){}'\"`".contains(*c))
        .map(|c| match c {
            'B' => '8',
            'O' | 'o' | 'D' => '0',
            'I' | 'l' => '1',
            'S' | 's' => '5',
            'Z' => '2',
            _ => c,
        })
        .collect();
    cleaned
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|t| t.parse::<u32>().ok())
        .filter(|n| CARGO_SIZES.contains(n))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// "¤135,876/SCU": a price per SCU (the number with the unit glued on).
    /// Holds the number part only.
    PerScu(String),
    Number(String),
    /// "85SCU", "12,000SCU": a quantity with the unit glued on (no slash).
    /// Holds the number part only.
    NumScu(String),
    /// "SCU" or "(SCU)" on its own: the unit of the number before it.
    Scu,
    Alpha(String),
    Other,
}

/// The number part of a per-SCU price, if `text` looks like one. OCR often
/// garbles the unit ("/8CU", "/SC0"), so anything after a slash that looks
/// roughly like SCU counts.
fn per_scu_number(text: &str) -> Option<&str> {
    let has_digit = |s: &str| s.chars().any(|c| c.is_ascii_digit());
    if let Some(i) = text.rfind('/') {
        let unit = normalise(&text[i + 1..]);
        let unit_like = (2..=5).contains(&unit.len())
            && (unit.contains("CU") || unit.contains("SC") || unit.ends_with("C0") || unit.starts_with(['S', '5', '8']));
        if unit_like && has_digit(&text[..i]) {
            return Some(&text[..i]);
        }
    }
    None
}

/// "85SCU" → "85"; also "LSCu" (a 1 read as L). No slash, or it's a price.
fn glued_quantity(text: &str) -> Option<String> {
    if text.contains('/') {
        return None;
    }
    let n = normalise(text);
    let body = n.strip_suffix("SCU").or_else(|| n.strip_suffix("SCO"))?;
    let digit_like = |c: char| c.is_ascii_digit() || "OILSBZ".contains(c);
    if body.is_empty() || body.len() > 8 || !body.chars().all(digit_like) {
        return None;
    }
    // Keep the separators from the original text ("12,000SCU" → "12,000").
    let up = text.to_uppercase();
    let cut = up.rfind("SC").unwrap_or(up.len());
    Some(text[..cut].trim().to_string())
}

fn tokenise(text: &str) -> Tok {
    let n = normalise(text);
    if n.is_empty() {
        return Tok::Other;
    }
    if n == "SCU" && !text.starts_with('/') {
        return Tok::Scu;
    }
    let has_digit = n.chars().any(|c| c.is_ascii_digit());
    if let Some(num) = per_scu_number(text) {
        return match parse::parse_price(num) {
            Some(_) => Tok::PerScu(num.to_string()),
            None => Tok::Other,
        };
    }
    if let Some(num) = glued_quantity(text) {
        if parse::parse_quantity(&num).is_some() {
            return Tok::NumScu(num);
        }
    }
    if has_digit {
        return match parse::parse_number(text) {
            Some(_) => Tok::Number(text.to_string()),
            None => Tok::Other,
        };
    }
    if n.chars().all(|c| c.is_ascii_alphabetic()) {
        return Tok::Alpha(n);
    }
    Tok::Other
}

#[derive(Debug, Clone, PartialEq)]
pub struct NameMatch {
    pub ids: Vec<String>,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct LocatedRow {
    pub side: Side,
    pub row: NormRect,
    pub name: NormRect,
    pub name_text: String,
    pub commodity: Option<NameMatch>,
    pub price: Option<NormRect>,
    pub price_text: Option<String>,
    pub quantity: Option<NormRect>,
    pub quantity_text: Option<String>,
    pub sizes: Option<NormRect>,
    pub sizes_are_button_band: bool,
    pub additional_size_bands: Vec<NormRect>,
    pub sizes_found: Vec<u32>,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct Located {
    pub rows: Vec<LocatedRow>,
    pub location_text: Option<String>,
    pub terminal_ids: Vec<String>,
}

struct Ctx {
    w: f32,
    h: f32,
    lh: f32,
}

impl Ctx {
    fn rect(&self, w: &OcrWord) -> NormRect {
        NormRect { x: w.x0 / self.w, y: w.y0 / self.h, w: (w.x1 - w.x0) / self.w, h: (w.y1 - w.y0) / self.h }
    }
    fn rect_of(&self, ws: &[&OcrWord]) -> Option<NormRect> {
        ws.iter().map(|w| self.rect(w)).reduce(NormRect::union)
    }
    fn pad(&self, r: NormRect) -> NormRect {
        r.pad(self.lh * 0.35 / self.w, self.lh * 0.3 / self.h)
    }
}

#[derive(Debug, Clone, Copy)]
struct Bbox {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

pub fn match_commodity(name: &str, catalog: &Catalog) -> Option<NameMatch> {
    let n = normalise(name);
    if n.len() < 3 {
        return None;
    }
    let mut best: Option<NameMatch> = None;
    for c in &catalog.commodities {
        let labels: Vec<&str> = match c.game_name.as_deref().filter(|g| !g.is_empty()) {
            Some(g) => vec![g],
            None => [Some(c.name.as_str()), c.display_name.as_deref()].into_iter().flatten().collect(),
        };
        let score = labels
            .iter()
            .map(|label| {
                let l = normalise(label);
                1.0 - strsim::levenshtein(&n, &l) as f32 / l.len().max(n.len()).max(1) as f32
            })
            .fold(0.0, f32::max);
        if score < 0.75 {
            continue;
        }
        match &mut best {
            Some(b) if (score - b.score).abs() < 1e-6 => b.ids.push(c.id.clone()),
            Some(b) if score < b.score => {}
            _ => best = Some(NameMatch { ids: vec![c.id.clone()], score }),
        }
    }
    best
}

pub fn match_location(text: &str, catalog: &Catalog) -> Vec<String> {
    let t = normalise(text);
    if t.len() < 3 {
        return vec![];
    }
    let close = |loc: &str| {
        let l = normalise(loc);
        l == t || (l.len() >= 5 && strsim::levenshtein(&l, &t) <= 1)
    };
    let mut ids: Vec<&crate::model::CommodityTerminal> = catalog.terminals.iter().filter(|x| close(&x.location)).collect();
    if ids.is_empty() {
        ids = catalog.terminals.iter().filter(|x| normalise(&x.location).contains(&t)).collect();
    }
    // The main exchange first.
    ids.sort_by_key(|x| !x.name.eq_ignore_ascii_case("admin"));
    ids.into_iter().map(|x| x.id.clone()).collect()
}

fn group_lines<'a>(words: &[&'a OcrWord], tol: f32) -> Vec<Vec<&'a OcrWord>> {
    let mut sorted: Vec<&OcrWord> = words.to_vec();
    sorted.sort_by(|a, b| a.cy().partial_cmp(&b.cy()).unwrap());
    let mut lines: Vec<Vec<&OcrWord>> = Vec::new();
    for w in sorted {
        match lines.last_mut() {
            Some(line) if (w.cy() - line.iter().map(|x| x.cy()).sum::<f32>() / line.len() as f32).abs() <= tol => {
                line.push(w)
            }
            _ => lines.push(vec![w]),
        }
    }
    for l in &mut lines {
        l.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap());
    }
    lines
}

/// Every place `phrase` appears as consecutive, closely spaced words on a line.
/// Matches however OCR split or glued the words ("IN DEMAND", "INDEMAND").
fn find_phrase(lines: &[Vec<&OcrWord>], phrase: &str, lh: f32) -> Vec<Bbox> {
    let target = normalise(phrase);
    // OCR misreads a letter or two in the terminal's font ("IW DERANO").
    let tol = match target.len() { 0..=5 => 0, 6..=7 => 1, _ => 2 };
    let mut out = Vec::new();
    for line in lines {
        for i in 0..line.len() {
            let mut acc = String::new();
            for j in i..line.len() {
                if j > i && line[j].x0 - line[j - 1].x1 > lh * 2.0 {
                    break;
                }
                acc.push_str(&normalise(&line[j].text));
                if strsim::levenshtein(&acc, &target) <= tol {
                    out.push(Bbox { x0: line[i].x0, y0: line[i].y0, x1: line[j].x1, y1: line[j].y1 });
                    break;
                }
                if acc.len() > target.len() + 1 {
                    break;
                }
            }
        }
    }
    out
}

fn first_phrase(lines: &[Vec<&OcrWord>], phrase: &str, lh: f32) -> Option<Bbox> {
    find_phrase(lines, phrase, lh).into_iter().min_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap())
}

struct Region {
    side: Side,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    /// The IN STOCK list: every complete row there has its "… INVENTORY" line.
    in_stock: bool,
}

impl Region {
    fn contains(&self, w: &OcrWord) -> bool {
        w.x0 >= self.x0 && w.x1 <= self.x1 && w.cy() >= self.y0 && w.cy() <= self.y1
    }

    /// Pulls the right edge in to just past the list's right-aligned prices, so
    /// the game world beside the panel doesn't leak into the rows.
    fn tighten(&mut self, words: &[OcrWord], lh: f32) {
        let edges: Vec<f32> =
            words.iter().filter(|w| self.contains(w) && matches!(tokenise(&w.text), Tok::PerScu(_))).map(|w| w.x1).collect();
        if !edges.is_empty() {
            self.x1 = self.x1.min(median(edges) + lh * 3.0);
        }
    }
}

/// `words` come from OCR of `img` (the frame resampled to LOCATE_H).
pub fn locate(words: &[OcrWord], img: &GrayImage, catalog: &Catalog) -> Option<Located> {
    let lh = median(words.iter().filter(|w| w.text.len() >= 2).map(|w| w.h()).collect());
    if lh <= 0.0 {
        return None;
    }
    let joined = join_split_numbers(words, lh);
    let words = joined.as_slice();
    let ctx = Ctx { w: img.width() as f32, h: img.height() as f32, lh };
    let all: Vec<&OcrWord> = words.iter().collect();
    let lines = group_lines(&all, lh * 0.5);

    // 1. The terminal's own labels.
    let demand = first_phrase(&lines, "IN DEMAND", lh);
    let in_stock = first_phrase(&lines, "IN STOCK", lh);
    let shop = first_phrase(&lines, "SHOP INVENTORY", lh);
    let header = first_phrase(&lines, "COMMODITIES", lh);
    let inventories = first_phrase(&lines, "YOUR INVENTORIES", lh);
    let buy_anchor = in_stock.or(shop);
    if demand.is_none() && buy_anchor.is_none() {
        return None;
    }
    // One label on its own could be anything; the terminal shows several.
    let labels = [header, demand, in_stock, shop, inventories].iter().filter(|l| l.is_some()).count();
    if labels < 2 {
        return None;
    }
    let out_of_stock = buy_anchor.and_then(|a| {
        find_phrase(&lines, "OUT OF STOCK", lh).into_iter().filter(|b| b.y0 > a.y1 && (b.x0 - a.x0).abs() < lh * 4.0).next()
    });

    // 2. The lists.
    let mut regions = Vec::new();
    if let Some(d) = demand {
        let limit = buy_anchor.map(|b| b.x0 - lh).unwrap_or(f32::MAX).min(d.x0 + lh * 45.0);
        regions.push(Region { side: Side::Sell, x0: d.x0 - lh * 2.0, x1: limit, y0: d.y1, y1: ctx.h, in_stock: false });
    }
    if let Some(a) = buy_anchor {
        let top = in_stock.map(|s| s.y1).unwrap_or(a.y1);
        let bottom = out_of_stock.map(|o| o.y0).unwrap_or(ctx.h);
        regions.push(Region { side: Side::Buy, x0: a.x0 - lh * 2.0, x1: ctx.w, y0: top, y1: bottom, in_stock: true });
        if let Some(o) = out_of_stock {
            regions.push(Region { side: Side::Buy, x0: a.x0 - lh * 2.0, x1: ctx.w, y0: o.y1, y1: ctx.h, in_stock: false });
        }
    }

    let mut rows = Vec::new();
    for mut region in regions {
        region.tighten(words, lh);
        let inside: Vec<&OcrWord> = words.iter().filter(|w| region.contains(w)).collect();
        rows.extend(read_rows(&ctx, &region, &group_lines(&inside, lh * 0.5), catalog));
    }
    let rows = merge_variants(rows);
    if rows.is_empty() {
        return None;
    }

    // 3. The location, from the YOUR INVENTORIES dropdown: the line between
    //    that label and IN DEMAND. The label itself often misreads ("(OLR
    //    INVENTOR"), so the left column above IN DEMAND is searched first.
    let location_text = demand.and_then(|d| location_above(&lines, d, lh)).or_else(|| inventories.and_then(|inv| {
        lines.iter().find_map(|line| {
            let names: Vec<&str> = line
                .iter()
                .filter(|w| w.y0 > inv.y1 && w.y0 < inv.y1 + lh * 3.0 && w.x0 >= inv.x0 - lh && w.x0 < inv.x1 + lh * 12.0)
                .filter(|w| matches!(tokenise(&w.text), Tok::Alpha(_)) || normalise(&w.text).len() >= 2)
                .map(|w| w.text.as_str())
                .collect();
            (!names.is_empty()).then(|| names.join(" "))
        })
    }));
    let terminal_ids = location_text.as_deref().map(|t| match_location(t, catalog)).unwrap_or_default();
    Some(Located { rows, location_text, terminal_ids })
}

/// The location dropdown's text: the lowest line above IN DEMAND in its column
/// that isn't a label. "Select Location" (nothing chosen) gives None.
fn location_above(lines: &[Vec<&OcrWord>], demand: Bbox, lh: f32) -> Option<String> {
    let skip = ["SELECT", "SUB", "CATEGORY", "COMMODITIES", "YOUR", "LOCATION"];
    lines
        .iter()
        .filter(|l| l.iter().any(|w| w.y1 <= demand.y0 && w.y0 > demand.y0 - lh * 8.0))
        .filter_map(|line| {
            let words: Vec<&OcrWord> = line
                .iter()
                .copied()
                .filter(|w| w.y1 <= demand.y0 && w.x0 >= demand.x0 - lh * 2.0 && w.x0 < demand.x0 + lh * 20.0)
                .collect();
            if words.is_empty() {
                return None;
            }
            let label = words.iter().any(|w| {
                let n = normalise(&w.text);
                skip.contains(&n.as_str()) || (n.len() >= 7 && strsim::levenshtein(&n, "INVENTORIES") <= 3)
            });
            let names: Vec<&str> = words
                .iter()
                .filter(|w| matches!(tokenise(&w.text), Tok::Alpha(a) if a.len() >= 3))
                .map(|w| w.text.as_str())
                .collect();
            (!label && !names.is_empty()).then(|| (words[0].y0, names.join(" ")))
        })
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .map(|(_, t)| t)
}

/// OCR sometimes splits one number into two words ("4136," + "076/SCU",
/// "10." + "260/SCU"). Joins a word ending in a digit or separator with a
/// digit-led word right next to it on the same line.
fn join_split_numbers(words: &[OcrWord], lh: f32) -> Vec<OcrWord> {
    let mut sorted: Vec<OcrWord> = words.to_vec();
    sorted.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap());
    let mut out: Vec<OcrWord> = Vec::with_capacity(sorted.len());
    for w in sorted {
        let starts_digit = w.text.chars().next().is_some_and(|c| c.is_ascii_digit());
        let prev = out.iter_mut().rev().find(|p| {
            (p.cy() - w.cy()).abs() < lh * 0.4
                && w.x0 >= p.x1 - lh * 0.2
                && w.x0 - p.x1 < lh * 0.7
                && p.text.chars().last().is_some_and(|c| c.is_ascii_digit() || c == ',' || c == '.')
        });
        match prev {
            Some(p) if starts_digit => {
                p.text.push_str(&w.text);
                p.confidence = p.confidence.min(w.confidence);
                p.x1 = p.x1.max(w.x1);
                p.y0 = p.y0.min(w.y0);
                p.y1 = p.y1.max(w.y1);
            }
            _ => out.push(w),
        }
    }
    out
}

fn name_words<'a>(line: &[&'a OcrWord], lh: f32) -> Vec<&'a OcrWord> {
    // A stock-level line ("MAX INVENTGRY") is never a name.
    if line.iter().any(|w| is_inventory_word(&w.text)) {
        return vec![];
    }
    // Taller than any text: a fragment of the commodity's icon.
    let is_name = |w: &OcrWord| w.h() < lh * 2.5 && matches!(tokenise(&w.text), Tok::Alpha(a) if a.len() >= 2 && !is_label(&a));
    let Some(start) = line.iter().position(|w| is_name(w)) else { return vec![] };
    let mut out = vec![line[start]];
    for w in &line[start + 1..] {
        if !is_name(w) || w.x0 - out.last().unwrap().x1 > lh * 1.5 {
            break;
        }
        out.push(w);
    }
    out
}

/// The words of a line that continue a wrapped name: the run of words starting
/// in line with the name, up to the gap before the right-hand column (where the
/// price may sit). Not a label (bar "SIZE 2") and not a stock level.
fn name_continuation<'a>(line: &[&'a OcrWord], name_x0: f32, lh: f32) -> Vec<&'a OcrWord> {
    let Some(first) = line.iter().position(|w| (w.x0 - name_x0).abs() < lh * 1.5) else { return vec![] };
    let lead = normalise(&line[first].text);
    if lead.is_empty() || (is_label(&lead) && lead != "SIZE") {
        return vec![];
    }
    let mut out = vec![line[first]];
    for w in &line[first + 1..] {
        if w.x0 - out.last().unwrap().x1 > lh * 1.5 || !matches!(tokenise(&w.text), Tok::Alpha(_) | Tok::Number(_)) {
            break;
        }
        out.push(w);
    }
    if out.iter().any(|w| is_inventory_word(&w.text)) || !matches!(tokenise(&out[0].text), Tok::Alpha(_)) {
        return vec![];
    }
    out
}

fn is_cargo_label(line: &[&OcrWord]) -> bool {
    let near = |w: &OcrWord, label: &str| strsim::levenshtein(&normalise(&w.text), label) <= label.len() / 4;
    line.iter().any(|w| near(w, "CARGO") || near(w, "AVAILABLE")) && line.iter().any(|w| near(w, "SIZE"))
}

fn clip_rect(rect: NormRect, bounds: NormRect) -> Option<NormRect> {
    let x = rect.x.max(bounds.x);
    let y = rect.y.max(bounds.y);
    let right = (rect.x + rect.w).min(bounds.x + bounds.w);
    let bottom = (rect.y + rect.h).min(bounds.y + bounds.h);
    (right > x && bottom > y).then_some(NormRect { x, y, w: right - x, h: bottom - y })
}

/// Lines of one list → commodity rows. A row starts at a line with a commodity
/// name and takes the lines below it until the next name.
fn read_rows(ctx: &Ctx, region: &Region, lines: &[Vec<&OcrWord>], catalog: &Catalog) -> Vec<LocatedRow> {
    struct Row<'a> {
        name: Vec<&'a OcrWord>,
        commodity: Option<NameMatch>,
        lines: Vec<Vec<&'a OcrWord>>,
        last_y: f32,
        /// Only name lines so far: the next line may continue the name.
        naming: bool,
    }
    let lh = ctx.lh;
    let mut rows: Vec<Row> = Vec::new();
    for line in lines {
        let cy = line.iter().map(|w| w.cy()).sum::<f32>() / line.len() as f32;
        // A damaged label (e.g. CARGO → CARED) must not start a new commodity.
        if is_cargo_label(line) {
            if let Some(cur) = rows.last_mut() {
                cur.lines.push(line.clone());
                cur.last_y = cy;
                cur.naming = false;
            }
            continue;
        }
        // A long name wraps ("SHIP / AMMUNITION - / SIZE 2"): a line right under
        // the name, starting in line with it, carries on the name.
        // Measured in the name's own font, which is bigger than the labels.
        if let Some(cur) = rows.last_mut().filter(|r| r.naming && cy - r.last_y < r.name.last().map_or(lh, |w| w.h()).max(lh) * 1.8) {
            // Nothing in the name's column and no name of its own (the shop
            // quantity to the right, bits of the icon to the left): part of the
            // row, and the name may go on.
            let own: String = name_words(line, lh).iter().map(|w| normalise(&w.text)).collect();
            if own.len() < 4 && !line.iter().any(|w| (w.x0 - cur.name[0].x0).abs() < lh * 1.5) {
                cur.lines.push(line.clone());
                continue;
            }
            let more = name_continuation(line, cur.name[0].x0, lh);
            if !more.is_empty() {
                cur.name.extend(more);
                cur.lines.push(line.clone());
                cur.last_y = cy;
                continue;
            }
        }
        let name = name_words(line, lh);
        let text: String = name.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        // An unknown name still starts a row (the player can pick the commodity),
        // as long as it looks like a word and not a stray letter.
        if normalise(&text).len() >= 4 || match_commodity(&text, catalog).is_some() {
            rows.push(Row { name, commodity: None, lines: vec![line.clone()], last_y: cy, naming: true });
        } else if let Some(cur) = rows.last_mut().filter(|r| cy - r.last_y < lh * 4.0) {
            cur.lines.push(line.clone());
            cur.last_y = cy;
            cur.naming = false;
        }
    }
    for r in &mut rows {
        let text: String = r.name.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        r.commodity = match_commodity(&text, catalog);
    }

    // OCR blobs from button borders can look like names. Only actual commodity
    // matches or rows with a per-SCU price establish the next card boundary.
    let card_tops: Vec<f32> = rows.iter().filter(|r| {
        r.commodity.is_some() || r.lines.iter().flatten().any(|w| per_scu_number(&w.text).is_some())
    }).map(|r| r.name.iter().map(|w| w.y0).fold(f32::INFINITY, f32::min)).collect();
    let mut out = Vec::new();
    for r in rows {
        let all: Vec<&OcrWord> = r.lines.iter().flatten().copied().collect();
        // The price: a parsed per-SCU number, or failing that anything with the
        // per-SCU unit ("B2n22/SCl)"): the close read settles the number.
        let price = all
            .iter()
            .find_map(|w| match tokenise(&w.text) {
                Tok::PerScu(t) => Some((*w, t)),
                _ => None,
            })
            .or_else(|| all.iter().find_map(|w| per_scu_number(&w.text).map(|t| (*w, t.to_string()))));
        let Some(price) = price else { continue };
        let card_top = r.name.iter().map(|w| w.y0).fold(f32::INFINITY, f32::min);
        let card_bottom = card_tops.iter().copied().filter(|y| *y > card_top).fold(region.y1, f32::min);
        let card = NormRect {
            x: region.x0 / ctx.w, y: card_top / ctx.h,
            w: (region.x1 - region.x0) / ctx.w, h: (card_bottom - card_top) / ctx.h,
        };

        let mut row = LocatedRow {
            side: region.side,
            row: ctx.pad(ctx.rect_of(&all).unwrap()),
            name: ctx.pad(ctx.rect_of(&r.name).unwrap()),
            name_text: r.name.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
            commodity: r.commodity,
            price: Some(ctx.pad(ctx.rect(price.0))),
            price_text: Some(price.1),
            quantity: None,
            quantity_text: None,
            sizes: None,
            sizes_are_button_band: false,
            additional_size_bands: vec![],
            sizes_found: vec![],
            confidence: all.iter().map(|w| w.confidence).fold(1.0, f32::min),
        };

        if region.side == Side::Buy {
            for line in &r.lines {
                let toks: Vec<Tok> = line.iter().map(|w| tokenise(&w.text)).collect();
                let hit = (0..line.len()).find_map(|i| match &toks[i] {
                    Tok::NumScu(t) => Some((i, t.clone())),
                    Tok::Number(t) if toks.get(i + 1) == Some(&Tok::Scu) && line[i + 1].x0 - line[i].x1 < lh * 1.5 => Some((i, t.clone())),
                    _ => None,
                });
                if let Some((i, text)) = hit {
                    row.quantity = Some(ctx.pad(ctx.rect(line[i])));
                    row.quantity_text = Some(text);
                    break;
                }
            }
        }
        if let Some(start) = r.lines.iter().position(|l| is_cargo_label(l)) {
            // Only the size-button line immediately below the label. An
            // expanded sell row continues with counts, time and totals.
            let boxes: Vec<&OcrWord> = r.lines.get(start + 1).into_iter().flatten().copied()
                .filter(|w| w.y0 >= card_top && w.y1 <= card_bottom && !box_sizes(&w.text).is_empty()).collect();
            row.sizes_found = boxes.iter().flat_map(|w| box_sizes(&w.text)).collect();
            row.sizes_found.sort_unstable();
            row.sizes_found.dedup();
            row.sizes = ctx.rect_of(&boxes).map(|r| ctx.pad(r));
            if row.sizes.is_none() {
                // Button borders can make sparse OCR return one unreadable blob.
                // Locate its number band from the label so the close pass still runs.
                let label = ctx.rect_of(&r.lines[start]).unwrap();
                row.sizes = Some(NormRect {
                    x: label.x,
                    y: label.y + label.h + lh * 0.3 / ctx.h,
                    w: (region.x1 / ctx.w - label.x).max(0.0),
                    h: lh * 2.5 / ctx.h,
                });
                row.sizes_are_button_band = true;
            }
            // Clamp both padded glyph crops and inferred bands before merging
            // quality variants. Each OCR call must stay in its original card.
            row.sizes = row.sizes.and_then(|rect| clip_rect(rect, card));
        }
        out.push((row, r.lines.iter().flatten().any(|w| is_inventory_word(&w.text))));
    }
    // A row cut off by the bottom of the IN STOCK list keeps its price but loses
    // its "… INVENTORY" line, and its name is often clipped too ("SHIP
    // AMMUNITION - SI"). Only the last row can be cut off, so only it is checked;
    // a misread inventory line higher up never drops a real row.
    if region.in_stock && out.last().is_some_and(|(_, complete)| !complete) {
        out.pop();
    }
    out.into_iter().map(|(row, _)| row).collect()
}

/// The same commodity once per quality (Savrilium q322, q580): one row per
/// commodity and side. Differing prices make the kept row low-confidence.
fn merge_variants(rows: Vec<LocatedRow>) -> Vec<LocatedRow> {
    let mut out: Vec<LocatedRow> = Vec::new();
    for r in rows {
        let key = r.commodity.as_ref().filter(|m| m.ids.len() == 1).map(|m| m.ids[0].clone());
        let dup = key.as_ref().and_then(|k| {
            out.iter_mut().find(|o| o.side == r.side && o.commodity.as_ref().is_some_and(|m| m.ids == [k.clone()]))
        });
        match dup {
            Some(o) => {
                let price = |x: &LocatedRow| x.price_text.as_deref().and_then(parse::parse_price).map(|p| p.0);
                if price(o) != price(&r) {
                    o.confidence = o.confidence.min(0.3);
                }
                // Each quality variant can expose different inventory sizes.
                // Keep the first crop for review, and retain all positive sizes.
                if r.side == Side::Sell {
                    // Changes on another quality row must invalidate crop dedupe.
                    o.row = o.row.union(r.row);
                    o.confidence = o.confidence.min(r.confidence);
                    o.sizes_found.extend(r.sizes_found);
                    o.sizes_found.sort_unstable();
                    o.sizes_found.dedup();
                    if o.sizes.is_none() {
                        o.sizes = r.sizes;
                        o.sizes_are_button_band = r.sizes_are_button_band;
                    } else if r.sizes_are_button_band {
                        o.additional_size_bands.extend(r.sizes);
                    }
                }
            }
            None => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Commodity, CommodityTerminal};

    /// A word at 1080p screen coordinates, scaled to the locate image (×4/3).
    fn w(text: &str, x: f32, y: f32) -> OcrWord {
        let s = LOCATE_H as f32 / 1080.0;
        let width = text.chars().count() as f32 * 9.0;
        OcrWord { text: text.into(), confidence: 0.9, x0: x * s, y0: y * s, x1: (x + width) * s, y1: (y + 14.0) * s }
    }

    fn catalog() -> Catalog {
        let c = |id: &str, game: &str| Commodity { id: id.into(), name: game.into(), display_name: None, game_name: Some(game.into()) };
        let t = |id: &str, name: &str, loc: &str| CommodityTerminal {
            id: id.into(),
            name: name.into(),
            location: loc.into(),
            place: None,
            system: "stanton".into(),
        };
        Catalog {
            commodities: vec![
                c("savrilium", "Savrilium"),
                c("tungsten", "Tungsten"),
                c("organics", "Organics"),
                c("saldynium-ore", "Saldynium (Ore)"),
                c("rmc", "Construction Materials"),
                c("cmat", "Construction Materials"),
            ],
            terminals: vec![
                t("admin-orison", "Admin", "Orison"),
                t("admin-lorville", "Admin", "Lorville"),
                t("cbd-lorville", "Central Business District", "Lorville"),
            ],
            fetched_at: None,
        }
    }

    /// The words OCR finds on an Orison terminal at 1080p, where they appear.
    fn orison_words() -> Vec<OcrWord> {
        vec![
            w("COMMODITIES", 270.0, 127.0),
            w("CURRENT", 1494.0, 138.0), w("BALANCE:", 1556.0, 138.0), w("¤2,369,554", 1519.0, 160.0), w("AUEC", 1617.0, 160.0),
            w("YOUR", 249.0, 215.0), w("INVENTORIES", 300.0, 215.0),
            w("SHOP", 1296.0, 210.0), w("INVENTORY", 1342.0, 210.0),
            w("ORISON", 263.0, 253.0),
            w("BUY", 1310.0, 249.0), w("LOCAL", 1438.0, 250.0), w("MARKET", 1486.0, 250.0), w("VALUE", 1552.0, 250.0),
            w("SELECT", 264.0, 300.0), w("SUB-CATEGORY", 318.0, 300.0),
            w("IN", 1295.0, 306.0), w("STOCK", 1315.0, 306.0),
            w("IN", 244.0, 336.0), w("DEMAND", 274.0, 336.0),
            // Sell: Savrilium q322, expanded with the selling widget.
            w("SAVRILIUM", 319.0, 384.0), w("9", 583.0, 385.0), w("SCU", 603.0, 385.0),
            w("QUALITY", 319.0, 410.0), w("322", 383.0, 410.0), w("¤135,876/SCU", 537.0, 407.0),
            w("AVAILABLE", 458.0, 441.0), w("CARGO", 517.0, 441.0), w("SIZE", 566.0, 441.0), w("(SCU)", 608.0, 441.0),
            w("1", 492.0, 464.0), w("4", 539.0, 464.0), w("MAX", 598.0, 472.0),
            w("X1", 488.0, 482.0), w("X2", 535.0, 482.0),
            w("EST.", 390.0, 521.0), w("UNLOADING", 418.0, 521.0), w("TIME", 510.0, 521.0), w("00:01:18", 550.0, 519.0),
            w("4", 277.0, 558.0), w("X2", 273.0, 575.0),
            w("TOTAL:", 271.0, 605.0), w("9", 317.0, 605.0), w("SCU", 333.0, 605.0), w("¤1,087,013", 373.0, 623.0), w("SELL", 576.0, 614.0),
            // Sell: Savrilium q580, same price.
            w("SAVRILIUM", 327.0, 679.0), w("1", 588.0, 679.0), w("SCU", 607.0, 679.0),
            w("QUALITY", 327.0, 704.0), w("580", 390.0, 704.0), w("¤135,876/SCU", 541.0, 698.0),
            w("AVAILABLE", 464.0, 732.0), w("CARGO", 523.0, 732.0), w("SIZE", 572.0, 732.0), w("(SCU)", 612.0, 732.0),
            w("1", 544.0, 754.0), w("MAX", 603.0, 761.0), w("X1", 540.0, 772.0),
            // Sell: Tungsten.
            w("TUNGSTEN", 331.0, 819.0), w("3", 589.0, 817.0), w("SCU", 608.0, 817.0),
            w("QUALITY", 331.0, 844.0), w("530", 392.0, 844.0), w("¤10,260/SCU", 553.0, 837.0),
            w("AVAILABLE", 467.0, 870.0), w("CARGO", 526.0, 870.0), w("SIZE", 575.0, 870.0), w("(SCU)", 615.0, 870.0),
            w("2", 500.0, 893.0), w("1", 546.0, 893.0), w("MAX", 605.0, 899.0), w("X1", 497.0, 910.0), w("X1", 544.0, 910.0),
            // Buy: Organics.
            w("ORGANICS", 1362.0, 348.0), w("SHOP", 1570.0, 347.0), w("QUANTITY", 1601.0, 347.0),
            w("VERY", 1362.0, 370.0), w("LOW", 1399.0, 370.0), w("INVENTORY", 1428.0, 370.0), w("29", 1599.0, 362.0), w("SCU", 1625.0, 362.0),
            w("¤10,232/SCU", 1572.0, 385.0),
            w("AVAILABLE", 1491.0, 410.0), w("CARGO", 1550.0, 410.0), w("SIZE", 1599.0, 410.0), w("(SCU)", 1640.0, 410.0),
            w("OUT", 1293.0, 503.0), w("OF", 1330.0, 503.0), w("STOCK", 1356.0, 503.0),
            // The game world between the panels.
            w("STOR-ALL", 1145.0, 433.0), w("UNLING", 1185.0, 383.0), w("8", 1000.0, 470.0),
        ]
    }

    #[test]
    fn reads_the_orison_terminal() {
        let img = GrayImage::new(2560, 1440);
        let loc = locate(&orison_words(), &img, &catalog()).expect("terminal");
        assert_eq!(loc.location_text.as_deref(), Some("ORISON"));
        assert_eq!(loc.terminal_ids, vec!["admin-orison"]);

        let id = |r: &LocatedRow| r.commodity.as_ref().map(|m| m.ids.join(",")).unwrap_or_default();
        let summary: Vec<(Side, String, Option<String>)> = loc.rows.iter().map(|r| (r.side, id(r), r.price_text.clone())).collect();
        assert_eq!(
            summary,
            vec![
                (Side::Sell, "savrilium".into(), Some("¤135,876".into())),
                (Side::Sell, "tungsten".into(), Some("¤10,260".into())),
                (Side::Buy, "organics".into(), Some("¤10,232".into())),
            ]
        );

        let sav = &loc.rows[0];
        assert!(sav.quantity.is_none(), "player cargo quantity is not terminal demand");
        assert_eq!(sav.sizes_found, vec![1, 4]);
        assert!(sav.sizes.is_some());
        assert_eq!(loc.rows[1].sizes_found, vec![1, 2]);
        assert!(sav.confidence > 0.5, "quality variants at one price merge cleanly");

        let org = &loc.rows[2];
        assert_eq!(org.quantity_text.as_deref(), Some("29"));
        assert!(org.sizes_found.is_empty());
    }

    #[test]
    fn buy_side_cargo_boxes() {
        let mut words = orison_words();
        words.extend([w("1", 1560.0, 432.0), w("8", 1600.0, 432.0), w("X1", 1556.0, 450.0), w("X2", 1596.0, 450.0)]);
        let loc = locate(&words, &GrayImage::new(2560, 1440), &catalog()).unwrap();
        assert_eq!(loc.rows[2].sizes_found, vec![1, 8]);
    }

    #[test]
    fn sell_sizes_merge_quality_variants_without_counts_or_expanded_controls() {
        let mut words = orison_words();
        for word in &mut words {
            // The second quality exposes a size not present on the first row.
            if word.text == "1" && (word.y0 - 754.0 * 4.0 / 3.0).abs() < 1.0 { word.text = "16".into(); }
            // A selected container below the button strip is not acceptance evidence.
            if word.text == "4" && (word.y0 - 558.0 * 4.0 / 3.0).abs() < 1.0 { word.text = "32".into(); }
        }
        let loc = locate(&words, &GrayImage::new(2560, 1440), &catalog()).unwrap();
        assert_eq!(loc.rows[0].sizes_found, vec![1, 4, 16]);
        assert!(loc.rows[0].quantity.is_none());
    }

    #[test]
    fn cargo_label_survives_misread_word_and_unreadable_buttons() {
        let mut words = orison_words();
        for word in &mut words {
            if word.text == "CARGO" { word.text = "CARED".into(); }
        }
        // Match the sparse OCR failure on the real screenshot: the first strip
        // is one tall blob rather than separate size digits.
        words.retain(|word| !(word.y0 >= 458.0 * 4.0 / 3.0 && word.y0 < 500.0 * 4.0 / 3.0 && word.x0 > 450.0 * 4.0 / 3.0 && word.x0 < 650.0 * 4.0 / 3.0));
        words.push(OcrWord { text: "Li".into(), x0: 633.0, y0: 610.0, x1: 857.0, y1: 664.0, confidence: 0.2 });
        let loc = locate(&words, &GrayImage::new(2560, 1440), &catalog()).unwrap();
        let sav = &loc.rows[0];
        assert!(sav.sizes_are_button_band);
        let band = sav.sizes.expect("missing glyphs still need a close-read band");
        assert!(band.y + band.h < 519.0 / 1080.0, "exclude expanded controls");
        assert_eq!(loc.rows[1].sizes_found, vec![1, 2]);
    }

    #[test]
    fn inferred_size_band_stops_before_next_commodity_card() {
        let mut words = orison_words();
        words.retain(|w| !(w.y0 >= 441.0 * 4.0 / 3.0 && w.y0 < 500.0 * 4.0 / 3.0 && w.x0 > 450.0 * 4.0 / 3.0 && w.x0 < 650.0 * 4.0 / 3.0));
        // A close-to-bottom label would otherwise project into the next card.
        words.extend([w("AVAILABLE", 458.0, 650.0), w("CARGO", 517.0, 650.0), w("SIZE", 566.0, 650.0)]);
        let loc = locate(&words, &GrayImage::new(2560, 1440), &catalog()).unwrap();
        let band = loc.rows[0].sizes.unwrap();
        assert!(loc.rows[0].sizes_are_button_band);
        assert!(band.y + band.h <= 679.0 / 1080.0 + 1e-6);
        assert!(band.h < 0.02, "clip the inferred height at the next card");
    }

    #[test]
    fn crop_clipping_rejects_other_cards_and_limits_all_edges() {
        let card = NormRect { x: 0.25, y: 0.25, w: 0.25, h: 0.25 };
        let oversized = NormRect { x: 0.1, y: 0.2, w: 0.7, h: 0.7 };
        assert_eq!(clip_rect(oversized, card), Some(card));
        assert!(clip_rect(NormRect { y: 0.6, ..card }, card).is_none());
    }

    #[test]
    fn a_row_cut_off_at_the_bottom_is_dropped() {
        // Below Organics (whose list would continue): a row clipped after its price.
        let mut words = orison_words();
        words.retain(|x| !(x.text == "OUT" || x.text == "OF" || (x.text == "STOCK" && x.y0 > 600.0)));
        words.extend([w("TUNGSTEN", 1362.0, 460.0), w("SHOP", 1570.0, 459.0), w("QUANTITY", 1601.0, 459.0), w("¤10,500/SCU", 1572.0, 497.0)]);
        let loc = locate(&words, &GrayImage::new(2560, 1440), &catalog()).unwrap();
        let buy: Vec<_> = loc.rows.iter().filter(|r| r.side == Side::Buy).map(|r| r.commodity.as_ref().unwrap().ids[0].clone()).collect();
        assert_eq!(buy, vec!["organics"]);
    }

    #[test]
    fn world_text_alone_is_not_a_terminal() {
        let words = vec![w("SAVRILIUM", 100.0, 200.0), w("¤135,876/SCU", 400.0, 200.0), w("IN", 1295.0, 306.0), w("STOCK", 1315.0, 306.0)];
        assert!(locate(&words, &GrayImage::new(2560, 1440), &catalog()).is_none());
    }

    #[test]
    fn names_match_game_names_and_report_ties() {
        let cat = catalog();
        assert_eq!(match_commodity("SAVRlLIUM", &cat).unwrap().ids, vec!["savrilium"]);
        assert_eq!(match_commodity("SALDYNIUM (ORE)", &cat).unwrap().ids, vec!["saldynium-ore"]);
        assert_eq!(match_commodity("CONSTRUCTION MATERIALS", &cat).unwrap().ids, vec!["rmc", "cmat"]);
        assert!(match_commodity("QUANTAINIUM", &cat).is_none());
    }

    #[test]
    fn garbled_units_and_split_numbers() {
        assert_eq!(tokenise("510,2392/8CU"), Tok::PerScu("510,2392".into()));
        assert_eq!(tokenise("¤135,876/SCU"), Tok::PerScu("¤135,876".into()));
        assert_eq!(tokenise("(SCU)"), Tok::Scu);
        let split = vec![w("4136,", 541.0, 698.0), OcrWord { x0: (541.0 + 45.0) * 4.0 / 3.0 + 3.0, ..w("076/SCU", 0.0, 698.0) }];
        let joined = join_split_numbers(&split, 14.0 * 4.0 / 3.0);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].text, "4136,076/SCU");
        assert_eq!(tokenise(&joined[0].text), Tok::PerScu("4136,076".into()));
    }

    #[test]
    fn quantities_prices_and_boxes_as_ocr_reads_them() {
        assert_eq!(tokenise("85SCU"), Tok::NumScu("85".into()));
        assert_eq!(tokenise("12,000SCU"), Tok::NumScu("12,000".into()));
        assert_eq!(tokenise("LSCu"), Tok::NumScu("L".into()));
        assert_eq!(tokenise("8.640/SCU"), Tok::PerScu("8.640".into()));
        assert_eq!(tokenise("6.868/5CuU"), Tok::PerScu("6.868".into()));
        for (text, want) in [("|16", vec![16]), ("1,2", vec![1, 2]), ("4/8", vec![4, 8]), ("|B", vec![8]), ("[24", vec![24]), ("32'", vec![32])] {
            assert_eq!(box_sizes(text), want, "{text}");
        }
        assert!(is_inventory_word("INVENGRY") && is_inventory_word("INVENIURY") && !is_inventory_word("INVENTORIES"));
    }

    #[test]
    fn location_prefers_admin() {
        assert_eq!(match_location("LORVILLE", &catalog()), vec!["admin-lorville", "cbd-lorville"]);
    }
}
