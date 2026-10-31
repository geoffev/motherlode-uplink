// Non-secret settings, kept as JSON in the app config dir. The API key is not
// here: it lives in the OS keychain (see secrets.rs).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PRODUCTION_URL: &str = "https://cryon.rocks";
/// Release builds only ever talk to production; development builds can point
/// anywhere (e.g. a local dev server).
pub const SERVER_LOCKED: bool = !cfg!(debug_assertions);
const DEFAULT_URL: &str = if SERVER_LOCKED { PRODUCTION_URL } else { "http://localhost:3000" };

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub api_base: String,
    pub capture_enabled: bool,
    pub capture_fps: f32,
    pub start_with_computer: bool,
    /// Where crops and frames are kept. None = the app data dir.
    pub crops_dir: Option<PathBuf>,
    pub crop_retention_days: u32,
    pub last_terminal_id: Option<String>,
    pub tesseract_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            api_base: DEFAULT_URL.into(),
            capture_enabled: true,
            capture_fps: 1.0,
            start_with_computer: false,
            crops_dir: None,
            crop_retention_days: 14,
            last_terminal_id: None,
            tesseract_path: None,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(Settings::sanitised)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::util::write_atomic(path, serde_json::to_string_pretty(self)?.as_bytes())
    }

    pub fn sanitised(mut self) -> Settings {
        self.api_base = self.api_base.trim().trim_end_matches('/').to_string();
        if SERVER_LOCKED {
            // Even a hand-edited settings file can't point a release elsewhere.
            self.api_base = PRODUCTION_URL.into();
        } else if self.api_base.is_empty() {
            self.api_base = DEFAULT_URL.into();
        }
        self.capture_fps = self.capture_fps.clamp(0.25, 2.0);
        self.crop_retention_days = self.crop_retention_days.clamp(1, 365);
        self
    }
}
