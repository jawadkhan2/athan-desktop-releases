use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

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

async fn check_and_install(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
    if let Some(update) = app.updater()?.check().await? {
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
