use crate::prayer;
use chrono::Local;
use salah::prelude::*;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub const TRAY_ID: &str = "athan-tray";

/// Show the main window and tell the webview which view to display
/// (`"main"` = prayer card, `"settings"` = settings). The view is reset on
/// every show so the window always opens where the user expects, regardless of
/// what it was last showing before being hidden to the tray.
fn show_window(app: &AppHandle, view: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = w.emit("navigate", view);
    }
}

/// Build the tray menu. When `entries` is provided, today's times are listed.
fn menu(app: &AppHandle, entries: Option<&Vec<prayer::PrayerEntry>>) -> tauri::Result<Menu<Wry>> {
    let mut b = MenuBuilder::new(app);
    let header = MenuItemBuilder::with_id("header", "Athan — Prayer Times")
        .enabled(false)
        .build(app)?;
    b = b.item(&header);

    if let Some(list) = entries {
        b = b.separator();
        for e in list {
            let label = format!("{:<8} {}", e.name, e.time);
            let item = MenuItemBuilder::with_id(format!("t_{}", e.key), label)
                .enabled(false)
                .build(app)?;
            b = b.item(&item);
        }
    }

    b = b.separator();
    let settings = MenuItemBuilder::with_id("settings", "Open Settings").build(app)?;
    let stop = MenuItemBuilder::with_id("stop", "Stop Athan").build(app)?;
    let redetect = MenuItemBuilder::with_id("redetect", "Re-detect Location").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    b = b
        .item(&settings)
        .item(&stop)
        .item(&redetect)
        .separator()
        .item(&quit);

    b.build()
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .expect("failed to load tray icon");
    let initial = menu(app, None)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Athan")
        .menu(&initial)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => show_window(app, "settings"),
            "stop" => crate::send_audio(app, crate::audio::AudioCmd::Stop),
            "redetect" => crate::trigger_redetect(app),
            "quit" => {
                // Tear down the webview window before exiting so WebView2 shuts
                // down cleanly (avoids the noisy "Failed to unregister class
                // Chrome_WidgetWin_0" warning on Windows).
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.destroy();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // Left-clicking the tray icon brings up the prayer card.
                show_window(tray.app_handle(), "main");
            }
        })
        .build(app)?;
    Ok(())
}

pub fn set_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Refresh tooltip (next prayer + countdown) and rebuild the menu when the day's
/// times change (tracked via `last_sig`).
pub fn update(app: &AppHandle, pt: &PrayerTimes, last_sig: &mut String) {
    let entries = prayer::entries(pt);

    let next = pt.next();
    let next_time = pt.time(next).with_timezone(&Local);
    let mins = (next_time - Local::now()).num_minutes().max(0);
    let tip = format!(
        "Athan — Next: {} {} (in {}h {:02}m)",
        next.name(),
        next_time.format("%H:%M"),
        mins / 60,
        mins % 60
    );
    set_tooltip(app, &tip);

    let sig: String = entries.iter().map(|e| format!("{}{}", e.key, e.time)).collect();
    if *last_sig != sig {
        *last_sig = sig;
        if let Ok(m) = menu(app, Some(&entries)) {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(m));
            }
        }
    }
}
