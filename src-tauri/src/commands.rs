// IPC commands for the web UI. Errors come back as user-facing strings.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::api::ApiError;
use crate::model::{Catalog, Confirmation, Me, OutboxItem, Reading, ReportStatus, Side, TerminalReport};
use crate::pipeline::{self, ScanOutcome};
use crate::secrets;
use crate::settings::Settings;
use crate::state::{AppState, CaptureStatus};
use crate::util::Guard;

type Res<T> = Result<T, String>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    signed_in: bool,
    masked_key: Option<String>,
    me: Option<Me>,
    /// A 401 stopped sending; the player needs to paste a new key.
    key_rejected: bool,
}

fn session_of(st: &AppState) -> Session {
    let key = st.api_key.guard().clone();
    Session {
        signed_in: key.is_some(),
        masked_key: key.as_deref().map(secrets::mask),
        me: st.me.guard().clone(),
        key_rejected: st.outbox.guard().paused_for_auth,
    }
}

#[tauri::command]
pub fn get_session(st: State<AppState>) -> Session {
    session_of(&st)
}

/// Fetches who the key belongs to, refreshing the cached copy.
pub async fn refresh_me(app: &AppHandle) -> Result<Me, ApiError> {
    let st = app.state::<AppState>();
    let client = st.client().ok_or_else(|| ApiError::Unauthorized("Not signed in".into()))?;
    let me = client.me().await;
    match &me {
        Ok(m) => {
            *st.me.guard() = Some(m.clone());
            crate::util::save_json(&st.paths.file("me.json"), m);
            st.wake_capture();
        }
        Err(ApiError::Unauthorized(_)) => {
            st.outbox.guard().paused_for_auth = true;
            let _ = app.emit("auth-required", ());
        }
        Err(_) => {}
    }
    let _ = app.emit("session", session_of(&st));
    me
}

#[tauri::command]
pub async fn sign_in(app: AppHandle, key: String) -> Res<Session> {
    let key = key.trim().to_string();
    if !key.starts_with("mlk_") {
        return Err("Motherlode API keys start with mlk_".into());
    }
    let st = app.state::<AppState>();
    let base = st.settings.guard().api_base.clone();
    let me = crate::api::Client::new(&base, &key).me().await.map_err(|e| match e {
        ApiError::Unauthorized(_) => "That key was not accepted. Check it, or create a new one on Motherlode.".to_string(),
        e => e.to_string(),
    })?;
    secrets::store_api_key(&key)?;
    *st.api_key.guard() = Some(key);
    *st.me.guard() = Some(me.clone());
    crate::util::save_json(&st.paths.file("me.json"), &me);
    st.outbox.guard().paused_for_auth = false;
    st.outbox_wake.notify_one();
    st.wake_capture();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = refresh_catalog_inner(&handle).await {
            log::warn!("catalog refresh after sign-in: {e}");
        }
    });
    Ok(session_of(&st))
}

#[tauri::command]
pub async fn refresh_session(app: AppHandle) -> Res<Session> {
    refresh_me(&app).await.map_err(|e| e.to_string())?;
    Ok(session_of(&app.state::<AppState>()))
}

#[tauri::command]
pub fn sign_out(st: State<AppState>) -> Res<Session> {
    secrets::delete_api_key()?;
    *st.api_key.guard() = None;
    *st.me.guard() = None;
    let _ = std::fs::remove_file(st.paths.file("me.json"));
    st.wake_capture();
    Ok(session_of(&st))
}

// ─── Catalog ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_catalog(st: State<AppState>) -> Arc<Catalog> {
    st.catalog()
}

pub async fn refresh_catalog_inner(app: &AppHandle) -> Res<Arc<Catalog>> {
    let st = app.state::<AppState>();
    let client = st.client().ok_or("Not signed in")?;
    let (commodities, terminals) = tokio::join!(client.commodities(), client.commodity_terminals());
    let mut cat = Catalog {
        commodities: commodities.map_err(|e| e.to_string())?,
        terminals: terminals.map_err(|e| e.to_string())?,
        fetched_at: Some(Utc::now()),
    };
    cat.commodities.sort_by(|a, b| a.label().to_lowercase().cmp(&b.label().to_lowercase()));
    cat.terminals.sort_by_key(|t| t.label().to_lowercase());
    crate::util::save_json(&st.paths.file("catalog.json"), &cat);
    let cat = Arc::new(cat);
    *st.catalog.guard() = cat.clone();
    let _ = app.emit("catalog", &*cat);
    Ok(cat)
}

#[tauri::command]
pub async fn refresh_catalog(app: AppHandle) -> Res<Arc<Catalog>> {
    refresh_catalog_inner(&app).await
}

// ─── Review queue ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_readings(st: State<AppState>) -> Vec<Reading> {
    st.readings.guard().all()
}

#[tauri::command]
pub fn confirm_readings(app: AppHandle, st: State<AppState>, items: Vec<Confirmation>) -> Res<Vec<OutboxItem>> {
    let catalog = st.catalog();
    if items.is_empty() {
        return Err("Nothing to confirm".into());
    }
    // Validate everything and build the reports first, so "Confirm all" queues
    // all or nothing.
    let mut by_terminal: BTreeMap<String, Vec<Confirmation>> = BTreeMap::new();
    for c in &items {
        if !catalog.terminals.iter().any(|t| t.id == c.terminal_id) {
            return Err("Pick the terminal these prices are from".into());
        }
        let commodity = catalog.commodities.iter().find(|x| x.id == c.commodity_id).ok_or("Pick a commodity")?;
        if c.price.is_some_and(|p| p < 0) || c.quantity.is_some_and(|q| q < 0.0) {
            return Err(format!("{}: prices and quantities can't be negative", commodity.label()));
        }
        let mut c = c.clone();
        // When this row's values were last on screen: its line's observedAt.
        c.observed_at = st.readings.guard().get(&c.reading_id).map(|r| r.updated_at);
        if c.side == Side::Sell {
            // Only the price on a sell row is the terminal's.
            c.quantity = None;
            c.sizes.clear();
        }
        if c.price.is_none() && c.quantity.is_none() && c.sizes.is_empty() {
            return Err(format!("{}: enter a price", commodity.label()));
        }
        by_terminal.entry(c.terminal_id.clone()).or_default().push(c);
    }

    let mut queued = Vec::new();
    {
        let readings = st.readings.guard();
        for (terminal_id, group) in &by_terminal {
            // When the prices were last seen on screen.
            let observed_at = group
                .iter()
                .filter_map(|c| readings.get(&c.reading_id).map(|r| r.updated_at))
                .max()
                .unwrap_or_else(Utc::now);
            let terminal_label = catalog.terminals.iter().find(|t| t.id == *terminal_id).map(|t| t.label()).unwrap_or_default();
            for report in TerminalReport::build(terminal_id, observed_at, group)? {
                let label = |id: &str| catalog.commodities.iter().find(|x| x.id == id).map(|x| x.label().to_string()).unwrap_or_default();
                let reading_ids = group
                    .iter()
                    .filter(|c| report.lines.iter().any(|l| l.commodity_id == c.commodity_id))
                    .map(|c| c.reading_id.clone())
                    .collect();
                queued.push(OutboxItem {
                    id: uuid::Uuid::new_v4().to_string(),
                    reading_ids,
                    terminal_label: terminal_label.clone(),
                    line_labels: report.lines.iter().map(|l| label(&l.commodity_id)).collect(),
                    report,
                    status: ReportStatus::Queued,
                    attempts: 0,
                    last_error: None,
                    line_errors: vec![],
                    next_attempt_at: Utc::now(),
                    created_at: Utc::now(),
                    sent_at: None,
                    report_id: None,
                });
            }
        }
    }

    let crops = st.crops();
    for c in by_terminal.values().flatten() {
        if let Some(r) = st.readings.guard().remove(&c.reading_id) {
            crops.write_label(&r, c);
        }
        let _ = app.emit("reading-removed", &c.reading_id);
    }
    st.readings.guard().save();
    for item in &queued {
        st.outbox.guard().push(item.clone());
        let _ = app.emit("outbox", item);
    }
    if let Some(last) = items.last() {
        let mut s = st.settings.guard();
        s.last_terminal_id = Some(last.terminal_id.clone());
        let _ = s.save(&st.paths.settings);
    }
    st.outbox_wake.notify_one();
    Ok(queued)
}

#[tauri::command]
pub fn reject_reading(app: AppHandle, st: State<AppState>, id: String) {
    let removed = st.readings.guard().remove(&id);
    if let Some(r) = removed {
        // The row stays in the duplicate check, so it isn't queued again while
        // it's still on screen; only changed values bring it back.
        st.crops().discard(&r);
        st.readings.guard().save();
    }
    let _ = app.emit("reading-removed", &id);
}

// ─── Outbox ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_outbox(st: State<AppState>) -> Vec<OutboxItem> {
    st.outbox.guard().items()
}

#[tauri::command]
pub fn retry_report(app: AppHandle, st: State<AppState>, id: String) -> Res<OutboxItem> {
    let item = st.outbox.guard().retry(&id).ok_or("That report isn't waiting for a retry")?;
    st.outbox_wake.notify_one();
    let _ = app.emit("outbox", &item);
    Ok(item)
}

// ─── Capture ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_capture_status(st: State<AppState>) -> CaptureStatus {
    st.capture_status.guard().clone()
}

#[tauri::command]
pub fn set_capture_enabled(app: AppHandle, enabled: bool) -> Res<Settings> {
    let st = app.state::<AppState>();
    let s = {
        let mut s = st.settings.guard();
        s.capture_enabled = enabled;
        s.save(&st.paths.settings).map_err(|e| e.to_string())?;
        s.clone()
    };
    st.wake_capture();
    crate::tray::sync(&app);
    let _ = app.emit("settings", &s);
    Ok(s)
}

/// Runs a screenshot file through the same pipeline as live capture.
#[tauri::command]
pub async fn scan_image(app: AppHandle, path: String) -> Res<ScanOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let img = image::open(&path).map_err(|e| format!("Could not open the image: {e}"))?.to_rgba8();
        pipeline::process_frame(&app, &img)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Settings ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
    crops_dir: String,
    ocr_engine: String,
    ocr_error: Option<String>,
    autostart: bool,
    /// Release builds: the Motherlode server can't be changed in Settings.
    server_locked: bool,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, st: State<AppState>) -> AppInfo {
    let ocr = st.ocr();
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: st.paths.data.to_string_lossy().into_owned(),
        crops_dir: st.crops().root.to_string_lossy().into_owned(),
        ocr_engine: ocr.name(),
        ocr_error: ocr.available().err(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        server_locked: crate::settings::SERVER_LOCKED,
    }
}

#[tauri::command]
pub fn get_settings(st: State<AppState>) -> Settings {
    st.settings.guard().clone()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Res<Settings> {
    let st = app.state::<AppState>();
    let next = settings.sanitised();
    let prev = st.settings.guard().clone();

    if next.start_with_computer != prev.start_with_computer {
        let al = app.autolaunch();
        let r = if next.start_with_computer { al.enable() } else { al.disable() };
        r.map_err(|e| format!("Could not change start-with-computer: {e}"))?;
    }
    if next.tesseract_path != prev.tesseract_path {
        // A file the player browsed to gets run, so make sure it's Tesseract first.
        if let Some(p) = &next.tesseract_path {
            crate::ocr::Tesseract::verify(p)?;
        }
        *st.ocr.guard() = crate::ocr::create(next.tesseract_path.as_deref());
    }
    if next.api_base != prev.api_base {
        // Another server: its catalog and the key's owner may differ.
        *st.me.guard() = None;
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = refresh_me(&handle).await;
            let _ = refresh_catalog_inner(&handle).await;
        });
    }
    next.save(&st.paths.settings).map_err(|e| e.to_string())?;
    *st.settings.guard() = next.clone();
    let _ = app.asset_protocol_scope().allow_directory(st.crops().root, true);
    st.wake_capture();
    crate::tray::sync(&app);
    Ok(next)
}

// ─── Development helpers ─────────────────────────────────────────────────────

/// Queues a made-up reading so the review UI can be worked on without the game.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn inject_demo_reading(app: AppHandle, st: State<AppState>) -> Res<Reading> {
    use crate::model::{Field, NormRect, Side};
    use image::{Rgba, RgbaImage};

    let catalog = st.catalog();
    let n = st.readings.guard().all().len();
    let commodity = catalog.commodities.get(n % catalog.commodities.len().max(1)).cloned();
    let mut frame = RgbaImage::from_pixel(1920, 1080, Rgba([12, 14, 20, 255]));
    let rects = [
        NormRect { x: 0.30, y: 0.30, w: 0.14, h: 0.035 },
        NormRect { x: 0.46, y: 0.30, w: 0.08, h: 0.035 },
        NormRect { x: 0.56, y: 0.30, w: 0.08, h: 0.035 },
        NormRect { x: 0.66, y: 0.30, w: 0.06, h: 0.035 },
    ];
    for r in &rects {
        let (x, y, w, h) = crate::vision::to_pixels(*r, 1920, 1080);
        for py in y..y + h {
            for px in x..x + w {
                let on = (px / 7 + py / 9) % 3 == 0;
                frame.put_pixel(px, py, if on { Rgba([232, 169, 32, 255]) } else { Rgba([30, 34, 44, 255]) });
            }
        }
    }
    let crops = st.crops();
    let id = uuid::Uuid::new_v4().to_string();
    let screen_id = uuid::Uuid::new_v4().to_string();
    let dir = crops.screen_dir(&screen_id);
    let save = |name: &str, r: NormRect| crops.save_crop(&dir, &format!("{id}-{name}"), &crate::vision::crop_rgba(&frame, r));
    let field = |r: NormRect, name: &str, conf: f32| Field::<()> { value: None, text: String::new(), confidence: conf, crop: save(name, r), rect: Some(r) };
    let (c, p, q, s) = (field(rects[0], "commodity", 0.92), field(rects[1], "price", 0.48), field(rects[2], "quantity", 0.81), field(rects[3], "sizes", 0.7));
    let buy = n % 2 == 1;
    let reading = Reading {
        id,
        screen_id,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        revision: 0,
        side: if buy { Side::Buy } else { Side::Sell },
        terminal_id: st.settings.guard().last_terminal_id.clone(),
        terminal_source: None,
        location_text: Some("ORISON".into()),
        commodity: Field {
            value: commodity.as_ref().map(|c| c.id.clone()),
            text: commodity.as_ref().map(|c| c.label().to_uppercase()).unwrap_or_default(),
            confidence: c.confidence,
            crop: c.crop,
            rect: c.rect,
        },
        commodity_candidates: vec![],
        price: Field { value: Some(10138 + n as i64 * 17), text: "10,138".into(), confidence: p.confidence, crop: p.crop, rect: p.rect },
        quantity: if buy {
            Field { value: Some(29.0), text: "29".into(), confidence: q.confidence, crop: q.crop, rect: q.rect }
        } else {
            Field::empty()
        },
        sizes: if buy {
            Field { value: Some(vec![1, 2, 4, 8]), text: "1 2 4 8".into(), confidence: s.confidence, crop: s.crop, rect: s.rect }
        } else {
            Field::empty()
        },
        frame: crops.save_frame(&dir, &frame),
        frame_width: 1920,
        frame_height: 1080,
    };
    st.readings.guard().upsert(reading.clone());
    st.readings.guard().save();
    let _ = app.emit("reading", &reading);
    Ok(reading)
}
