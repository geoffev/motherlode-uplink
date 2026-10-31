// Duplicate readings. A terminal screen stays up for a while, the player
// scrolls, looks away and back: the same row gets read many times. Two checks:
//   - the row's crop looks the same as one seen recently (perceptual hash):
//     skip it before spending any OCR on it;
//   - same terminal + commodity + side + values within the window: one reading.
//     Different values for a reading still in the queue update that reading.
// Confirmed and rejected rows stay in the window too, so the same row isn't
// queued again while it's still on screen; changed values are a new reading.

use std::time::{Duration, Instant};

use crate::model::Side;
use crate::vision::{hamming, Hash};

const WINDOW: Duration = Duration::from_secs(10 * 60);
/// Bits (of 256, see vision::dhash_wide) two row crops may differ by and still
/// count as the same. Kept low: one changed digit must not be skipped.
const SAME_CROP: u32 = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub terminal: Option<String>,
    /// Commodity id, or the name as read if it matched none.
    pub commodity: String,
    pub side: Side,
}

#[derive(Debug, Clone)]
pub struct Values {
    pub price: Option<i64>,
    pub quantity: Option<f64>,
    pub sizes: Vec<u32>,
}

impl PartialEq for Values {
    fn eq(&self, o: &Values) -> bool {
        let q = match (self.quantity, o.quantity) {
            (Some(a), Some(b)) => (a - b).abs() < 0.01,
            (a, b) => a.is_none() && b.is_none(),
        };
        self.price == o.price && q && self.sizes == o.sizes
    }
}

struct Entry {
    at: Instant,
    key: Key,
    values: Values,
    row_hash: Hash,
    reading_id: String,
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    New,
    /// Already have this one (the reading's id).
    Duplicate(String),
    /// Same row, new values: this reading should be updated if still queued.
    Update(String),
}

#[derive(Default)]
pub struct Recent {
    entries: Vec<Entry>,
}

impl Recent {
    fn prune(&mut self) {
        self.entries.retain(|e| e.at.elapsed() < WINDOW);
    }

    pub fn seen_crop(&mut self, h: &Hash) -> bool {
        self.prune();
        self.entries.iter().any(|e| hamming(&e.row_hash, h) <= SAME_CROP)
    }

    pub fn check(&mut self, key: &Key, values: &Values) -> Verdict {
        self.prune();
        match self.entries.iter().rev().find(|e| &e.key == key) {
            None => Verdict::New,
            Some(e) if &e.values == values => Verdict::Duplicate(e.reading_id.clone()),
            Some(e) => Verdict::Update(e.reading_id.clone()),
        }
    }

    pub fn record(&mut self, key: Key, values: Values, row_hash: Hash, reading_id: String) {
        self.entries.retain(|e| e.reading_id != reading_id);
        self.entries.push(Entry { at: Instant::now(), key, values, row_hash, reading_id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_then_update() {
        let mut r = Recent::default();
        let key = Key { terminal: Some("t".into()), commodity: "agricium".into(), side: Side::Sell };
        let v = Values { price: Some(10), quantity: Some(1.0), sizes: vec![1] };
        assert_eq!(r.check(&key, &v), Verdict::New);
        r.record(key.clone(), v.clone(), [0; 4], "a".into());
        assert_eq!(r.check(&key, &v), Verdict::Duplicate("a".into()));
        let v2 = Values { price: Some(11), ..v };
        assert_eq!(r.check(&key, &v2), Verdict::Update("a".into()));
        assert!(r.seen_crop(&[1, 0, 0, 0]));
        assert!(!r.seen_crop(&[u64::MAX, 0, 0, 0]));
    }
}
