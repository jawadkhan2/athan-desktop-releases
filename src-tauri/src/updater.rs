use crate::AppState;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

/// While waiting out a playing athan, re-check this often.
const ATHAN_POLL: Duration = Duration::from_secs(30);

/// The app sits in the tray for days, so a startup-only check is not enough:
/// check shortly after launch, then re-check periodically.
const STARTUP_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);
        loop {
            let handle = app.clone();
            tauri::async_runtime::block_on(async move {
                if let Err(e) = check_and_install(&handle).await {
                    eprintln!("updater: {e}");
                }
            });
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// Manual "Check for updates" from the settings UI. Unlike the background loop,
/// this is user-initiated, so it installs immediately (no athan wait) and
/// reports the outcome back to the frontend. Returns `Some(version)` when an
/// update was found and is installing, `None` when already up to date.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<Option<String>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    match update {
        Some(update) => {
            let version = update.version.clone();
            let _ = app
                .notification()
                .builder()
                .title("Athan")
                .body(format!("Updating to v{version}…"))
                .show();
            update
                .download_and_install(|_chunk, _total| {}, || {})
                .await
                .map_err(|e| e.to_string())?;
            // Windows: the passive NSIS installer stops the app and relaunches;
            // other platforms reach the restart below.
            app.restart();
            #[allow(unreachable_code)]
            Ok(Some(version))
        }
        None => Ok(None),
    }
}

async fn check_and_install(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
    if let Some(update) = app.updater()?.check().await? {
        // The passive NSIS installer stops the running app. Never do that while
        // an athan is playing — wait for it to finish first.
        while app.state::<AppState>().athan_playing.load(Ordering::SeqCst) {
            std::thread::sleep(ATHAN_POLL);
        }
        let _ = app
            .notification()
            .builder()
            .title("Athan")
            .body(format!("Updating to v{}…", update.version))
            .show();
        update.download_and_install(|_chunk, _total| {}, || {}).await?;
        // On Windows the passive NSIS installer stops the app, installs and
        // relaunches it, so this line is only reached on other platforms.
        app.restart();
    }
    Ok(())
}
