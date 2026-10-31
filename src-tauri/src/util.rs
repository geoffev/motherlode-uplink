use std::path::Path;

/// Writes via a temp file + rename so a crash never leaves half a JSON file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let s = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&s) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("ignoring unreadable {}: {e}", path.display());
            None
        }
    }
}

pub fn save_json<T: serde::Serialize>(path: &Path, value: &T) {
    let res = (|| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        write_atomic(path, &serde_json::to_vec(value)?)
    })();
    if let Err(e) = res {
        log::error!("could not save {}: {e}", path.display());
    }
}

pub fn normalise(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// Locks a mutex even if a panic elsewhere poisoned it. The state behind it is
/// plain data that stays usable, and one failed frame mustn't take every later
/// command down with it.
pub trait Guard<T> {
    fn guard(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> Guard<T> for std::sync::Mutex<T> {
    fn guard(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
