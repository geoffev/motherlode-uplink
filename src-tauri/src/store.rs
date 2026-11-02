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

    pub fn sanitise_paths(&mut self, crops: &Crops) {
        for reading in &mut self.list { crops.sanitise_reading_paths(reading); }
    }

    pub fn save(&self) {
        crate::util::save_json(&self.path, &self.list);
    }

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

    pub fn prune_older_than(&mut self, cutoff: DateTime<Utc>) -> Vec<Reading> {
        let (stale, keep): (Vec<Reading>, Vec<Reading>) = self.list.drain(..).partition(|r| r.updated_at < cutoff);
        self.list = keep;
        if !stale.is_empty() {
            self.save();
        }
        stale
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT, including junctions
    }
    #[cfg(not(windows))]
    { metadata.file_type().is_symlink() }
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
        let (id, field) = name.rsplit_once('-')?;
        if !canonical_uuid(id) || !["commodity", "price", "quantity", "sizes"].contains(&field) { return None; }
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

    /// Accept only an existing capture file under this storage root, with its
    /// expected date/screen/filename layout. Canonicalization resolves junctions
    /// and symlinks before containment is checked; component checks reject `..`.
    fn reading_file(&self, reading: &Reading, stored: &str, field: &str) -> Option<PathBuf> {
        if !canonical_uuid(&reading.id) || !canonical_uuid(&reading.screen_id) { return None; }
        let path = Path::new(stored);
        if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        let root = self.root.canonicalize().ok()?;
        let resolved = path.canonicalize().ok()?;
        let relative = resolved.strip_prefix(&root).ok()?;
        let parts: Vec<_> = relative.iter().collect();
        if parts.len() != 3 { return None; }
        let date = parts[0].to_str()?;
        let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        if day.format("%Y-%m-%d").to_string() != date || parts[1] != std::ffi::OsStr::new(&reading.screen_id) {
            return None;
        }
        let expected = if field == "frame" { "frame.jpg".to_string() } else { format!("{}-{field}.png", reading.id) };
        if parts[2] != std::ffi::OsStr::new(&expected) || !resolved.is_file() { return None; }
        // Reject reparse points as well as symlinks in the capture tree.
        let mut ancestor = root.clone();
        for part in parts {
            ancestor.push(part);
            if is_link(&std::fs::symlink_metadata(&ancestor).ok()?) { return None; }
        }
        if is_link(&std::fs::symlink_metadata(path).ok()?) { return None; }
        Some(resolved)
    }

    /// Strip unsafe/missing references before persisted data reaches the frontend.
    pub fn sanitise_reading_paths(&self, reading: &mut Reading) {
        let validate = |path: &Option<String>, field| path.as_deref()
            .and_then(|p| self.reading_file(reading, p, field))
            .map(|p| p.to_string_lossy().into_owned());
        let paths = [validate(&reading.frame, "frame"), validate(&reading.commodity.crop, "commodity"),
            validate(&reading.price.crop, "price"), validate(&reading.quantity.crop, "quantity"),
            validate(&reading.sizes.crop, "sizes")];
        let [frame, commodity, price, quantity, sizes] = paths;
        reading.frame = frame;
        reading.commodity.crop = commodity;
        reading.price.crop = price;
        reading.quantity.crop = quantity;
        reading.sizes.crop = sizes;
    }

    pub fn allow_reading_previews(&self, app: &tauri::AppHandle, reading: &Reading) {
        use tauri::Manager;
        for (path, field) in [(&reading.frame, "frame"), (&reading.commodity.crop, "commodity"),
            (&reading.price.crop, "price"), (&reading.quantity.crop, "quantity"), (&reading.sizes.crop, "sizes")] {
            let Some(path) = path.as_deref().and_then(|p| self.reading_file(reading, p, field)) else { continue };
            if let Err(e) = app.asset_protocol_scope().allow_file(path) {
                log::warn!("could not enable image preview: {e}");
            }
        }
    }

    /// The confirmed values next to the validated crop: the training label.
    pub fn write_label(&self, reading: &Reading, confirmed: &Confirmation) {
        let Some(crop) = reading.commodity.crop.as_deref()
            .and_then(|p| self.reading_file(reading, p, "commodity")) else { return };
        let Some(dir) = crop.parent() else { return };
        let label = serde_json::json!({ "reading": reading, "confirmed": confirmed, "confirmedAt": Utc::now() });
        // Confirmation creates one label. Never follow or replace an existing label/temp symlink.
        let result = (|| -> std::io::Result<()> {
            let file = std::fs::OpenOptions::new().write(true).create_new(true)
                .open(dir.join(format!("{}.json", reading.id)))?;
            use std::io::Write;
            let mut writer = std::io::BufWriter::new(file);
            serde_json::to_writer(&mut writer, &label)?;
            writer.flush()
        })();
        if let Err(e) = result { log::warn!("could not save crop label: {e}"); }
    }

    /// Deletes validated crops only; a stored path never grants deletion rights.
    pub fn discard(&self, r: &Reading) {
        for (path, field) in [(&r.commodity.crop, "commodity"), (&r.price.crop, "price"),
            (&r.quantity.crop, "quantity"), (&r.sizes.crop, "sizes")] {
            if let Some(path) = path.as_deref().and_then(|p| self.reading_file(r, p, field)) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    pub fn cleanup(&self, keep_days: u32) {
        let Ok(root) = self.root.canonicalize() else { return };
        let Ok(entries) = std::fs::read_dir(&root) else { return };
        let cutoff = Utc::now().date_naive() - chrono::Duration::days(keep_days as i64);
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Ok(day) = NaiveDate::parse_from_str(&name, "%Y-%m-%d") {
                let safe_directory = std::fs::symlink_metadata(e.path())
                    .is_ok_and(|m| m.is_dir() && !is_link(&m))
                    && e.path().canonicalize().is_ok_and(|p| p.parent() == Some(root.as_path()));
                if day < cutoff && safe_directory {
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

    fn capture_fixture(root: &Path) -> (Crops, Reading, PathBuf) {
        let crops = Crops { root: root.to_path_buf() };
        let mut r = reading(&uuid::Uuid::new_v4().to_string(), 0);
        r.screen_id = uuid::Uuid::new_v4().to_string();
        let dir = crops.screen_dir(&r.screen_id);
        std::fs::create_dir_all(&dir).unwrap();
        let crop = dir.join(format!("{}-commodity.png", r.id));
        let frame = dir.join("frame.jpg");
        std::fs::write(&crop, b"crop").unwrap();
        std::fs::write(&frame, b"frame").unwrap();
        r.commodity.crop = Some(crop.to_string_lossy().into_owned());
        r.frame = Some(frame.to_string_lossy().into_owned());
        (crops, r, dir)
    }

    fn confirmation(r: &Reading) -> Confirmation {
        Confirmation { reading_id: r.id.clone(), terminal_id: "terminal".into(),
            commodity_id: "commodity".into(), side: Side::Sell, price: Some(1),
            quantity: None, sizes: vec![], observed_at: None }
    }

    #[test]
    fn valid_capture_paths_can_be_previewed_labelled_and_discarded() {
        let tmp = tempfile::tempdir().unwrap();
        let (crops, mut r, dir) = capture_fixture(&tmp.path().join("crops"));
        crops.sanitise_reading_paths(&mut r);
        assert!(r.commodity.crop.is_some() && r.frame.is_some());
        crops.write_label(&r, &confirmation(&r));
        let label = dir.join(format!("{}.json", r.id));
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&label).unwrap()).unwrap();
        assert_eq!(saved["reading"]["id"], r.id);
        crops.discard(&r);
        assert!(!Path::new(r.commodity.crop.as_ref().unwrap()).exists());
        assert!(Path::new(r.frame.as_ref().unwrap()).exists());
        assert!(label.exists());
    }

    #[test]
    fn persisted_outside_and_traversal_paths_cannot_read_delete_or_write() {
        let tmp = tempfile::tempdir().unwrap();
        let (crops, r, _) = capture_fixture(&tmp.path().join("crops"));
        let outside_dir = tmp.path().join("crops-other").join("2026-01-01").join(&r.screen_id);
        std::fs::create_dir_all(&outside_dir).unwrap();
        let outside = outside_dir.join(format!("{}-commodity.png", r.id));
        std::fs::write(&outside, b"unrelated file").unwrap();
        let traverse = crops.root.join("..").join("crops-other").join("2026-01-01")
            .join(&r.screen_id).join(format!("{}-commodity.png", r.id));
        for path in [outside.clone(), traverse, PathBuf::from("relative.png")] {
            let mut tampered = r.clone();
            tampered.commodity.crop = Some(path.to_string_lossy().into_owned());
            assert!(crops.reading_file(&tampered, tampered.commodity.crop.as_ref().unwrap(), "commodity").is_none());
            crops.write_label(&tampered, &confirmation(&tampered));
            crops.discard(&tampered);
            crops.sanitise_reading_paths(&mut tampered);
            assert!(tampered.commodity.crop.is_none());
            assert_eq!(std::fs::read(&outside).unwrap(), b"unrelated file");
            assert!(!outside_dir.join(format!("{}.json", r.id)).exists());
        }
    }

    #[test]
    fn persisted_ids_and_mismatched_capture_names_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let (crops, r, dir) = capture_fixture(&tmp.path().join("crops"));
        for id in ["../escape", "nested/file", "not-a-uuid", "file:stream"] {
            let mut tampered = r.clone();
            tampered.id = id.into();
            crops.sanitise_reading_paths(&mut tampered);
            assert!(tampered.commodity.crop.is_none() && tampered.frame.is_none());
            crops.write_label(&tampered, &confirmation(&tampered));
            assert!(crops.save_crop(&dir, &format!("{id}-commodity"), &RgbaImage::new(1, 1)).is_none());
        }
        let wrong_name = dir.join("unrelated.png");
        std::fs::write(&wrong_name, b"private image").unwrap();
        let mut tampered = r.clone();
        tampered.commodity.crop = Some(wrong_name.to_string_lossy().into_owned());
        crops.discard(&tampered);
        assert!(wrong_name.exists());
        crops.sanitise_reading_paths(&mut tampered);
        assert!(tampered.commodity.crop.is_none());
    }

    #[test]
    fn label_creation_never_overwrites_an_existing_target() {
        let tmp = tempfile::tempdir().unwrap();
        let (crops, r, dir) = capture_fixture(&tmp.path().join("crops"));
        let label = dir.join(format!("{}.json", r.id));
        std::fs::write(&label, b"existing file").unwrap();
        crops.write_label(&r, &confirmation(&r));
        assert_eq!(std::fs::read(&label).unwrap(), b"existing file");
    }

    // Directory junctions do not need the Windows symbolic-link privilege.
    // Kept test-only: no shell or process execution is needed to create the fixture.
    #[cfg(windows)]
    fn link_directory(target: &Path, link: &Path) -> std::io::Result<()> {
        use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn DeviceIoControl(handle: *mut c_void, code: u32, input: *const c_void,
                input_len: u32, output: *mut c_void, output_len: u32,
                returned: *mut u32, overlapped: *mut c_void) -> i32;
        }
        let canonical = target.canonicalize()?;
        let wide: Vec<u16> = canonical.as_os_str().encode_wide().collect();
        // canonicalize returns a verbatim local path (\\?\C:\...).
        let print = wide.strip_prefix(&[92, 92, 63, 92]).unwrap_or(&wide);
        let substitute: Vec<u16> = r"\??\".encode_utf16().chain(print.iter().copied()).collect();
        let sub_len = (substitute.len() * 2) as u16;
        let print_len = (print.len() * 2) as u16;
        // REPARSE_DATA_BUFFER with MountPointReparseBuffer followed by two UTF-16 strings.
        let data_len = 8 + sub_len + 2 + print_len + 2;
        let mut buffer = 0xA0000003u32.to_le_bytes().to_vec(); // IO_REPARSE_TAG_MOUNT_POINT
        for value in [data_len, 0, 0, sub_len, sub_len + 2, print_len] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
        for value in substitute.iter().chain([0u16].iter()).chain(print).chain([0u16].iter()) {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
        std::fs::create_dir(link)?;
        let directory = std::fs::OpenOptions::new().write(true)
            .custom_flags(0x02200000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
            .open(link)?;
        let mut returned = 0;
        // SAFETY: the directory handle and the initialized input buffer remain alive
        // for this synchronous FSCTL_SET_REPARSE_POINT call. No output buffer is used.
        let result = unsafe { DeviceIoControl(directory.as_raw_handle(), 0x000900A4,
            buffer.as_ptr().cast(), buffer.len() as u32, std::ptr::null_mut(), 0,
            &mut returned, std::ptr::null_mut()) };
        if result == 0 { return Err(std::io::Error::last_os_error()); }
        Ok(())
    }

    #[cfg(unix)]
    fn link_directory(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn retention_cleanup_does_not_follow_linked_date_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("crops");
        let outside = tmp.path().join("unrelated");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let protected = outside.join("keep.txt");
        std::fs::write(&protected, b"keep").unwrap();
        link_directory(&outside, &root.join("2000-01-01")).unwrap();
        let old_capture = root.join("2000-01-02");
        std::fs::create_dir_all(&old_capture).unwrap();
        std::fs::write(old_capture.join("frame.jpg"), b"old").unwrap();
        Crops { root }.cleanup(14);
        assert_eq!(std::fs::read(protected).unwrap(), b"keep");
        assert!(!old_capture.exists());
    }

    #[test]
    fn loading_sanitises_only_paths_and_preserves_reading_values() {
        let tmp = tempfile::tempdir().unwrap();
        let (crops, mut r, _) = capture_fixture(&tmp.path().join("crops"));
        let outside = tmp.path().join("private.png");
        std::fs::write(&outside, b"private").unwrap();
        r.price.value = Some(12345);
        r.price.crop = Some(outside.to_string_lossy().into_owned());
        let path = tmp.path().join("readings.json");
        crate::util::save_json(&path, &vec![r.clone()]);
        let mut stored = Readings::load(path);
        stored.sanitise_paths(&crops);
        let validated = stored.get(&r.id).unwrap();
        assert_eq!(validated.price.value, Some(12345));
        assert!(validated.price.crop.is_none());
        assert!(validated.commodity.crop.is_some() && validated.frame.is_some());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn linked_screen_directory_cannot_escape_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let (crops, mut r, dir) = capture_fixture(&tmp.path().join("crops"));
        // Move the capture outside the root, then put a directory symlink in its place.
        let outside = tmp.path().join("outside");
        std::fs::rename(&dir, &outside).unwrap();
        link_directory(&outside, &dir).unwrap();
        crops.write_label(&r, &confirmation(&r));
        crops.discard(&r);
        assert!(outside.join(format!("{}-commodity.png", r.id)).exists());
        assert!(!outside.join(format!("{}.json", r.id)).exists());
        crops.sanitise_reading_paths(&mut r);
        assert!(r.commodity.crop.is_none() && r.frame.is_none());
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
