//! Self-update from GitHub Releases. The frontend calls `update_check` on a
//! timer when auto-update is on. macOS installs the new bundle in place right
//! away; it runs on the next launch, so no running agent is interrupted.
//! Installing a .deb needs root, and handing a file in the user's temp
//! directory to `pkexec dpkg -i` would let any process running as the user
//! (an agent, say) swap it before root reads it. So on Linux the check only
//! reports the version and the user installs the .deb themselves.

use crate::error::{Error, Result};
use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct UpdateState {
    /// Version already installed, so later checks do not fetch it again. Held
    /// for the whole check, so overlapping checks never download twice.
    installed: Mutex<Option<String>>,
}

fn updater_error(e: tauri_plugin_updater::Error) -> Error {
    Error::Invalid(format!("update: {e}"))
}

/// Installs a newer release if there is one. Returns the version the user
/// has to install by hand (Linux); None when up to date or already installed.
#[tauri::command]
pub async fn update_check(app: AppHandle, state: State<'_, UpdateState>) -> Result<Option<String>> {
    // `cargo run` and `tauri dev` builds have no installed bundle to replace.
    if cfg!(debug_assertions) || tauri::utils::platform::bundle_type().is_none() {
        return Ok(None);
    }
    let mut installed = state.installed.lock().await;
    let Some(update) = app.updater().map_err(updater_error)?.check().await.map_err(updater_error)? else {
        return Ok(None);
    };
    if cfg!(target_os = "linux") {
        return Ok(Some(update.version));
    }
    if installed.as_deref() == Some(update.version.as_str()) {
        return Ok(None);
    }
    let bytes = update.download(|_, _| {}, || {}).await.map_err(updater_error)?;
    update.install(&bytes).map_err(updater_error)?;
    *installed = Some(update.version);
    Ok(None)
}
