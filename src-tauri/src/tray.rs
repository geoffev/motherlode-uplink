// The app lives in the system tray; closing the window only hides it.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::state::AppState;
use crate::util::Guard;

pub struct TrayItems {
    pub capture: CheckMenuItem<Wry>,
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let enabled = app.state::<AppState>().settings.guard().capture_enabled;
    let open = MenuItem::with_id(app, "open", "Open Price Reporter", true, None::<&str>)?;
    let capture = CheckMenuItem::with_id(app, "capture", "Watch for terminals", true, enabled, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &capture, &PredefinedMenuItem::separator(app)?, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Motherlode Price Reporter")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_main(app),
            "capture" => {
                let on = !app.state::<AppState>().settings.guard().capture_enabled;
                let _ = crate::commands::set_capture_enabled(app.clone(), on);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayItems { capture });
    Ok(())
}

/// Keeps the tray's checkbox in step with the setting.
pub fn sync(app: &AppHandle) {
    let enabled = app.state::<AppState>().settings.guard().capture_enabled;
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.capture.set_checked(enabled);
    }
}

/// The tray tooltip says whether anything is being recorded.
pub fn set_status(app: &AppHandle, status: &crate::state::CaptureStatus) {
    use crate::state::CaptureState;
    let line = match status.state {
        CaptureState::NoGame | CaptureState::Error => status.message.clone().unwrap_or_default(),
        CaptureState::Paused => "Paused: nothing is being recorded.".into(),
        CaptureState::Idle => "Not signed in: nothing is being recorded.".into(),
        CaptureState::Watching | CaptureState::Reading | CaptureState::Terminal => "Watching Star Citizen for trade terminals.".into(),
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("Motherlode Price Reporter\n{line}")));
    }
}
