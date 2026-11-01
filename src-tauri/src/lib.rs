mod api;
mod capture;
mod commands;
mod dedupe;
mod detect;
mod model;
mod ocr;
mod outbox;
mod parse;
mod pipeline;
mod secrets;
mod settings;
mod state;
mod store;
mod tray;
mod util;
mod vision;
#[cfg(windows)]
mod wgc;

use std::sync::{Arc, Condvar, Mutex};

use tauri::{Manager, WindowEvent};

use crate::state::{AppState, CaptureState, CaptureStatus, Paths};
use crate::util::Guard;

const MINIMIZED_FLAG: &str = "--minimized";

fn init_state(app: &tauri::App) -> Result<AppState, Box<dyn std::error::Error>> {
    let config = app.path().app_config_dir()?;
    let data = app.path().app_data_dir()?;
    std::fs::create_dir_all(&config)?;
    std::fs::create_dir_all(&data)?;
    let paths = Paths { settings: config.join("settings.json"), data };

    let settings = settings::Settings::load(&paths.settings);
    let catalog: model::Catalog = util::load_json(&paths.file("catalog.json")).unwrap_or_default();
    let me: Option<model::Me> = util::load_json(&paths.file("me.json"));
    let api_key = secrets::load_api_key();
    let resources = app.path().resource_dir()?;
    let ocr = ocr::create(settings.tesseract_path.as_deref(), Some(&resources));
    log::info!("OCR engine: {}", ocr.name());

    Ok(AppState {
        outbox: Mutex::new(outbox::Outbox::load(paths.file("outbox.json"))),
        readings: Mutex::new(store::Readings::load(paths.file("readings.json"))),
        me: Mutex::new(if api_key.is_some() { me } else { None }),
        api_key: Mutex::new(api_key),
        catalog: Mutex::new(Arc::new(catalog)),
        ocr: Mutex::new(ocr),
        settings: Mutex::new(settings),
        recent: Mutex::new(dedupe::Recent::default()),
        outbox_wake: Arc::new(tokio::sync::Notify::new()),
        capture_status: Mutex::new(CaptureStatus { state: CaptureState::Idle, message: None, source: String::new() }),
        capture_wake: (Mutex::new(0), Condvar::new()),
        paths,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Must be first: a second launch just brings the running one forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_FLAG]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

    builder
        .setup(|app| {
            let state = init_state(app)?;
            let signed_in = state.api_key.guard().is_some();
            let crops = state.crops();
            let retention = state.settings.guard().crop_retention_days;
            app.manage(state);

            std::fs::create_dir_all(&crops.root)?;
            let _ = app.asset_protocol_scope().allow_directory(&crops.root, true);
            std::thread::spawn(move || crops.cleanup(retention));

            tray::build(app.handle())?;

            // First run (no key yet) opens the window; otherwise stay in the tray
            // unless the player launched the app themselves.
            let minimized = std::env::args().any(|a| a == MINIMIZED_FLAG);
            if !signed_in || !minimized {
                tray::show_main(app.handle());
            }

            let handle = app.handle().clone();
            let wake = app.state::<AppState>().outbox_wake.clone();
            tauri::async_runtime::spawn(supervise_outbox(handle, wake));

            let handle = app.handle().clone();
            std::thread::Builder::new().name("capture".into()).spawn(move || supervise_capture(handle))?;

            if signed_in {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = commands::refresh_me(&handle).await;
                    if let Err(e) = commands::refresh_catalog_inner(&handle).await {
                        log::warn!("catalog refresh: {e}");
                    }
                });
            }

            #[cfg(not(debug_assertions))]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { check_for_update(handle).await });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_session,
            commands::sign_in,
            commands::sign_out,
            commands::refresh_session,
            commands::get_catalog,
            commands::refresh_catalog,
            commands::list_readings,
            commands::confirm_readings,
            commands::reject_reading,
            commands::list_outbox,
            commands::retry_report,
            commands::get_capture_status,
            commands::set_capture_enabled,
            commands::scan_image,
            commands::get_app_info,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

const RESTART_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

fn supervise_capture(app: tauri::AppHandle) {
    loop {
        let handle = app.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pipeline::run_capture(handle)));
        let Err(panic) = result else { return };
        let what = panic_message(&panic);
        log::error!("capture loop crashed, restarting: {what}");
        pipeline::report_crash(&app, &what);
        std::thread::sleep(RESTART_AFTER);
    }
}

async fn supervise_outbox(app: tauri::AppHandle, wake: outbox::Wake) {
    loop {
        match tauri::async_runtime::spawn(outbox::run(app.clone(), wake.clone())).await {
            Ok(()) => return,
            Err(e) => log::error!("send queue crashed, restarting: {e}"),
        }
        tokio::time::sleep(RESTART_AFTER).await;
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".into())
}

#[cfg(not(debug_assertions))]
async fn check_for_update(app: tauri::AppHandle) {
    use tauri_plugin_updater::UpdaterExt;
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            log::warn!("updater unavailable: {e}");
            return;
        }
    };
    match updater.check().await {
        Ok(Some(u)) => {
            log::info!("installing update {}", u.version);
            if let Err(e) = u.download_and_install(|_, _| {}, || {}).await {
                log::warn!("update failed: {e}");
            }
        }
        Ok(None) => {}
        Err(e) => log::warn!("update check failed: {e}"),
    }
}
