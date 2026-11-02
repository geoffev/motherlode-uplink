use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use serde::Serialize;

use crate::api;
use crate::dedupe::Recent;
use crate::model::{Catalog, Me};
use crate::ocr::OcrEngine;
use crate::outbox::{Outbox, Wake};
use crate::settings::Settings;
use crate::store::{Crops, Readings};
use crate::util::Guard;

pub struct Paths {
    pub settings: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    pub fn file(&self, name: &str) -> PathBuf {
        self.data.join(name)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CaptureState {
    Paused,
    /// Not signed in, or not in the Data Surveyors.
    Idle,
    /// Star Citizen isn't running (or is minimised): nothing is captured.
    NoGame,
    Watching,
    Reading,
    Terminal,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub message: Option<String>,
    pub source: String,
}

pub struct AppState {
    pub paths: Paths,
    pub settings: Mutex<Settings>,
    /// A custom storage root must first be chosen through the native folder picker.
    pub selected_crops_dir: Mutex<Option<PathBuf>>,
    pub api_key: Mutex<Option<String>>,
    pub me: Mutex<Option<Me>>,
    pub catalog: Mutex<Arc<Catalog>>,
    pub outbox: Mutex<Outbox>,
    pub outbox_wake: Wake,
    pub readings: Mutex<Readings>,
    pub recent: Mutex<Recent>,
    pub ocr: Mutex<Arc<dyn OcrEngine>>,
    pub capture_status: Mutex<CaptureStatus>,
    /// Pokes the capture loop when settings change (pause/resume).
    pub capture_wake: (Mutex<u64>, Condvar),
}

impl AppState {
    pub fn client(&self) -> Option<api::Client> {
        let (base, key) = self.api_target()?;
        Some(api::Client::new(&base, &key))
    }

    pub fn api_target(&self) -> Option<(String, String)> {
        let key = self.api_key.guard().clone()?;
        Some((self.settings.guard().api_base.clone(), key))
    }

    pub fn ocr(&self) -> Arc<dyn OcrEngine> {
        self.ocr.guard().clone()
    }

    pub fn catalog(&self) -> Arc<Catalog> {
        self.catalog.guard().clone()
    }

    pub fn crops(&self) -> Crops {
        let custom = self.settings.guard().crops_dir.clone();
        Crops { root: custom.unwrap_or_else(|| self.paths.data.join("crops")) }
    }

    /// Readings are only taken when the key may file reports (Data Surveyors members).
    pub fn can_report(&self) -> bool {
        self.api_key.guard().is_some() && self.me.guard().as_ref().is_some_and(|m| m.can_report())
    }

    pub fn wake_capture(&self) {
        let (lock, cv) = &self.capture_wake;
        *lock.guard() += 1;
        cv.notify_all();
    }
}
