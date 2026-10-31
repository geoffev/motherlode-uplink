// The review queue, and the frames/crops behind it.
//
// Crops are kept on disk (crops/<date>/<screen>/) for the retention period:
// they're training data for the OCR model. A confirmed reading also gets a
// <reading>.json next to its crops with the values the player confirmed, i.e.
// the label for each crop. Rejected readings' crops are deleted.

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use image::RgbaImage;

use crate::model::{Confirmation, Reading};

pub struct Readings {
    path: PathBuf,
    list: Vec<Reading>,
}

impl Readings {
    pub fn load(path: PathBuf) -> Readings {
        let list = crate::util::load_json(&path).unwrap_or_default();
        Readings { path, list }
    }

    pub fn save(&self) {
        crate::util::save_json(&self.path, &self.list);
    }

    /// Newest first.
    pub fn all(&self) -> Vec<Reading> {
        let mut v = self.list.clone();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    pub fn get(&self, id: &str) -> Option<&Reading> {
        self.list.iter().find(|r| r.id == id)
    }

    pub fn upsert(&mut self, r: Reading) {
        match self.list.iter_mut().find(|x| x.id == r.id) {
            Some(x) => *x = r,
            None => self.list.push(r),
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Reading> {
        let i = self.list.iter().position(|r| r.id == id)?;
        Some(self.list.remove(i))
    }

    /// Takes out readings last seen before `cutoff` (and saves, if any went).
    pub fn prune_older_than(&mut self, cutoff: DateTime<Utc>) -> Vec<Reading> {
        let (stale, keep): (Vec<Reading>, Vec<Reading>) = self.list.drain(..).partition(|r| r.updated_at < cutoff);
        self.list = keep;
        if !stale.is_empty() {
            self.save();
        }
        stale
    }
}

pub struct Crops {
    pub root: PathBuf,
}

impl Crops {
    pub fn screen_dir(&self, screen_id: &str) -> PathBuf {
        self.root.join(Utc::now().format("%Y-%m-%d").to_string()).join(screen_id)
    }

    pub fn save_frame(&self, dir: &Path, frame: &RgbaImage) -> Option<String> {
        std::fs::create_dir_all(dir).ok()?;
        let path = dir.join("frame.jpg");
        let rgb = image::DynamicImage::ImageRgba8(frame.clone()).to_rgb8();
        let file = std::fs::File::create(&path).ok()?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 85);
        match enc.encode_image(&rgb) {
            Ok(()) => Some(path.to_string_lossy().into_owned()),
            Err(e) => {
                log::warn!("could not save frame: {e}");
                None
            }
        }
    }

    pub fn save_crop(&self, dir: &Path, name: &str, crop: &RgbaImage) -> Option<String> {
        std::fs::create_dir_all(dir).ok()?;
        let path = dir.join(format!("{name}.png"));
        match crop.save_with_format(&path, image::ImageFormat::Png) {
            Ok(()) => Some(path.to_string_lossy().into_owned()),
            Err(e) => {
                log::warn!("could not save crop {name}: {e}");
                None
            }
        }
    }

    /// The confirmed values next to the crops: the training label.
    pub fn write_label(&self, reading: &Reading, confirmed: &Confirmation) {
        let Some(dir) = reading.commodity.crop.as_deref().and_then(|p| Path::new(p).parent()) else { return };
        let label = serde_json::json!({ "reading": reading, "confirmed": confirmed, "confirmedAt": Utc::now() });
        crate::util::save_json(&dir.join(format!("{}.json", reading.id)), &label);
    }

    /// Deletes a rejected reading's crops (the shared frame stays for its siblings).
    pub fn discard(&self, r: &Reading) {
        for p in [&r.commodity.crop, &r.price.crop, &r.quantity.crop, &r.sizes.crop].into_iter().flatten() {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Removes day folders older than the retention period.
    pub fn cleanup(&self, keep_days: u32) {
        let Ok(entries) = std::fs::read_dir(&self.root) else { return };
        let cutoff = Utc::now().date_naive() - chrono::Duration::days(keep_days as i64);
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Ok(day) = NaiveDate::parse_from_str(&name, "%Y-%m-%d") {
                if day < cutoff {
                    if let Err(err) = std::fs::remove_dir_all(e.path()) {
                        log::warn!("could not remove old crops {name}: {err}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Field, Side};

    fn reading(id: &str, hours_ago: i64) -> Reading {
        let at = Utc::now() - chrono::Duration::hours(hours_ago);
        Reading {
            id: id.into(),
            screen_id: "s".into(),
            created_at: at,
            updated_at: at,
            revision: 0,
            side: Side::Sell,
            terminal_id: None,
            terminal_source: None,
            location_text: None,
            commodity: Field::empty(),
            commodity_candidates: vec![],
            price: Field::empty(),
            quantity: Field::empty(),
            sizes: Field::empty(),
            frame: None,
            frame_width: 0,
            frame_height: 0,
        }
    }

    #[test]
    fn prunes_only_stale_readings() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = Readings::load(dir.path().join("readings.json"));
        r.upsert(reading("fresh", 1));
        r.upsert(reading("stale", 30));
        let gone = r.prune_older_than(Utc::now() - chrono::Duration::hours(24));
        assert_eq!(gone.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["stale"]);
        assert_eq!(r.all().len(), 1);
        assert!(Readings::load(dir.path().join("readings.json")).get("fresh").is_some(), "saved");
    }
}
