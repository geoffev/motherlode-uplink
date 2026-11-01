// Types shared between the Rust core and the review UI (serialised camelCase).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Container sizes the game uses, in SCU. Readings and reports only ever carry these.
pub const CARGO_SIZES: [u32; 7] = [1, 2, 4, 8, 16, 24, 32];

pub const MARKET_REPORTS_WRITE: &str = "market-reports:write";

pub const MAX_REPORT_LINES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// The terminal sells to you (SHOP INVENTORY → IN STOCK): price, stock, sizes.
    Buy,
    /// The terminal buys from you (IN DEMAND): price and observed accepted sizes.
    /// The SCU quantity is the player's cargo, not terminal demand.
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl NormRect {
    pub fn union(self, o: NormRect) -> NormRect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w).max(o.x + o.w);
        let y1 = (self.y + self.h).max(o.y + o.h);
        NormRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    pub fn pad(self, px: f32, py: f32) -> NormRect {
        let x0 = (self.x - px).max(0.0);
        let y0 = (self.y - py).max(0.0);
        let x1 = (self.x + self.w + px).min(1.0);
        let y1 = (self.y + self.h + py).min(1.0);
        NormRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field<T> {
    pub value: Option<T>,
    pub text: String,
    pub confidence: f32,
    pub crop: Option<String>,
    pub rect: Option<NormRect>,
}

impl<T> Field<T> {
    pub fn empty() -> Self {
        Field { value: None, text: String::new(), confidence: 0.0, crop: None, rect: None }
    }
}

impl<T> Default for Field<T> {
    fn default() -> Self {
        Field::empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalSource {
    Screen,
    Last,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub id: String,
    pub screen_id: String,
    pub created_at: DateTime<Utc>,
    /// When the values were last seen on screen: the report's `observedAt`.
    pub updated_at: DateTime<Utc>,
    pub revision: u32,
    pub side: Side,
    pub terminal_id: Option<String>,
    pub terminal_source: Option<TerminalSource>,
    #[serde(default)]
    pub location_text: Option<String>,
    pub commodity: Field<String>,
    /// Several commodities share this game name (e.g. "Construction Materials"):
    /// the player picks one. Empty when the name matched a single commodity.
    #[serde(default)]
    pub commodity_candidates: Vec<String>,
    /// aUEC per SCU.
    pub price: Field<i64>,
    /// Buy side only: SCU in stock.
    pub quantity: Field<f64>,
    /// Container sizes in stock (buy) or observed accepted from inventory (sell).
    pub sizes: Field<Vec<u32>>,
    pub frame: Option<String>,
    pub frame_width: u32,
    pub frame_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmation {
    pub reading_id: String,
    pub terminal_id: String,
    pub commodity_id: String,
    pub side: Side,
    pub price: Option<i64>,
    pub quantity: Option<f64>,
    pub sizes: Vec<u32>,
    /// When these values were last seen on screen. Filled in by the core from
    /// the reading, not taken from the UI.
    #[serde(default, skip_deserializing)]
    pub observed_at: Option<DateTime<Utc>>,
}

/// One commodity in a terminal report. Both sides of the same commodity share a line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportLine {
    pub commodity_id: String,
    /// When this line's prices were read; Motherlode falls back to the report's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_price: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demand_scu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_cargo_sizes: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_price: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock_scu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_cargo_sizes: Option<Vec<u32>>,
}

impl ReportLine {
    pub fn is_empty(&self) -> bool {
        self.sell_price.is_none()
            && self.demand_scu.is_none()
            && self.sell_cargo_sizes.is_none()
            && self.buy_price.is_none()
            && self.stock_scu.is_none()
            && self.buy_cargo_sizes.is_none()
    }
}

/// POST body for `/api/public/v1/terminal-reports`: everything from one screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReport {
    pub terminal_id: String,
    pub observed_at: DateTime<Utc>,
    pub lines: Vec<ReportLine>,
}

impl TerminalReport {
    /// Buy and sell for the same commodity merge into one line.
    pub fn build(terminal_id: &str, observed_at: DateTime<Utc>, items: &[Confirmation]) -> Result<Vec<TerminalReport>, String> {
        let mut lines: BTreeMap<&str, ReportLine> = BTreeMap::new();
        for c in items {
            let line = lines
                .entry(&c.commodity_id)
                .or_insert_with(|| ReportLine { commodity_id: c.commodity_id.clone(), ..Default::default() });
            line.observed_at = line.observed_at.max(c.observed_at);
            match c.side {
                Side::Sell => {
                    if line.sell_price.is_some() || line.sell_cargo_sizes.is_some() {
                        return Err(format!("{} is on the sell side twice", c.commodity_id));
                    }
                    line.sell_price = c.price;
                    let sizes: Vec<u32> = CARGO_SIZES.iter().copied().filter(|s| c.sizes.contains(s)).collect();
                    line.sell_cargo_sizes = (!sizes.is_empty()).then_some(sizes);
                }
                Side::Buy => {
                    if line.buy_price.is_some() || line.stock_scu.is_some() {
                        return Err(format!("{} is on the buy side twice", c.commodity_id));
                    }
                    let sizes: Vec<u32> = CARGO_SIZES.iter().copied().filter(|s| c.sizes.contains(s)).collect();
                    line.buy_price = c.price;
                    line.stock_scu = c.quantity.map(|q| (q * 100.0).round() / 100.0);
                    line.buy_cargo_sizes = (!sizes.is_empty()).then_some(sizes);
                }
            }
        }
        let lines: Vec<ReportLine> = lines.into_values().filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            return Err("Nothing to report".into());
        }
        Ok(lines
            .chunks(MAX_REPORT_LINES)
            .map(|chunk| TerminalReport { terminal_id: terminal_id.to_string(), observed_at, lines: chunk.to_vec() })
            .collect())
    }

    /// The earliest time any line was read: Motherlode rejects the whole report
    /// if any line is more than 24 hours old.
    pub fn oldest_observation(&self) -> DateTime<Utc> {
        self.lines.iter().filter_map(|l| l.observed_at).fold(self.observed_at, DateTime::min)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    Queued,
    Sending,
    Sent,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineError {
    pub index: usize,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItem {
    pub id: String,
    pub reading_ids: Vec<String>,
    pub terminal_label: String,
    pub line_labels: Vec<String>,
    pub report: TerminalReport,
    pub status: ReportStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
    #[serde(default)]
    pub line_errors: Vec<LineError>,
    pub next_attempt_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub report_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: String,
    pub display_name: String,
    pub survey_corps: bool,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl Me {
    pub fn can_report(&self) -> bool {
        self.scopes.iter().any(|s| s == MARKET_REPORTS_WRITE)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commodity {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// What the game's terminals print, e.g. "Saldynium (Ore)". Match OCR against this.
    #[serde(default)]
    pub game_name: Option<String>,
}

impl Commodity {
    pub fn label(&self) -> &str {
        self.display_name.as_deref().filter(|s| !s.is_empty()).unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommodityTerminal {
    pub id: String,
    pub name: String,
    pub location: String,
    pub place: Option<String>,
    pub system: String,
}

impl CommodityTerminal {
    pub fn label(&self) -> String {
        format!("{} - {}", self.location, self.name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub commodities: Vec<Commodity>,
    pub terminals: Vec<CommodityTerminal>,
    pub fetched_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conf(commodity: &str, side: Side, price: i64) -> Confirmation {
        Confirmation {
            reading_id: "r".into(),
            terminal_id: "t".into(),
            commodity_id: commodity.into(),
            side,
            price: Some(price),
            quantity: None,
            sizes: vec![],
            observed_at: None,
        }
    }

    #[test]
    fn report_merges_sides_and_cleans_values() {
        let mut buy = conf("agricium", Side::Buy, 7047);
        buy.quantity = Some(1924.1349);
        buy.sizes = vec![8, 1, 3, 2];
        let mut sell = conf("agricium", Side::Sell, 10138);
        sell.sizes = vec![4, 1, 4, 3];
        sell.quantity = Some(9.0);
        let items = vec![sell, buy, conf("tungsten", Side::Sell, 10260)];
        let reports = TerminalReport::build("t", Utc::now(), &items).unwrap();
        assert_eq!(reports.len(), 1);
        let lines = &reports[0].lines;
        assert_eq!(lines.len(), 2);
        let a = &lines[0];
        assert_eq!((a.sell_price, a.buy_price, a.stock_scu), (Some(10138), Some(7047), Some(1924.13)));
        assert_eq!(a.buy_cargo_sizes, Some(vec![1, 2, 8]));
        assert!(a.demand_scu.is_none());
        assert_eq!(a.sell_cargo_sizes, Some(vec![1, 4]));
        let json = serde_json::to_value(&reports[0]).unwrap();
        assert_eq!(json["lines"][0]["sellCargoSizes"], serde_json::json!([1, 4]));
        assert_eq!(json["lines"][1], serde_json::json!({ "commodityId": "tungsten", "sellPrice": 10260 }));
        assert!(json["lines"][0].get("observedAt").is_none());
        assert!(json["observedAt"].is_string());
    }

    #[test]
    fn lines_carry_their_own_read_time() {
        let t = |m: i64| Utc::now() - chrono::Duration::minutes(m);
        let mut sell = conf("agricium", Side::Sell, 10138);
        sell.observed_at = Some(t(30));
        let mut buy = conf("agricium", Side::Buy, 7047);
        buy.observed_at = Some(t(10));
        let mut other = conf("tungsten", Side::Sell, 10260);
        other.observed_at = Some(t(50));
        let report = &TerminalReport::build("t", t(5), &[sell, buy, other.clone()]).unwrap()[0];
        let agricium = report.lines.iter().find(|l| l.commodity_id == "agricium").unwrap();
        let tungsten = report.lines.iter().find(|l| l.commodity_id == "tungsten").unwrap();
        assert!(agricium.observed_at.unwrap() > t(11), "the later of buy and sell");
        assert_eq!(tungsten.observed_at, other.observed_at);
        assert_eq!(report.oldest_observation(), other.observed_at.unwrap());
        let json = serde_json::to_value(report).unwrap();
        assert!(json["lines"][0]["observedAt"].is_string());
    }

    #[test]
    fn report_rejects_duplicate_side_and_splits_large_batches() {
        let dup = vec![conf("agricium", Side::Sell, 1), conf("agricium", Side::Sell, 2)];
        assert!(TerminalReport::build("t", Utc::now(), &dup).is_err());
        let many: Vec<_> = (0..150).map(|i| conf(&format!("c{i:03}"), Side::Sell, i)).collect();
        let reports = TerminalReport::build("t", Utc::now(), &many).unwrap();
        assert_eq!(reports.iter().map(|r| r.lines.len()).collect::<Vec<_>>(), vec![100, 50]);
    }
}
