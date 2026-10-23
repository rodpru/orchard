//! Self-update from GitHub Releases. The frontend calls `update_check` on a
//! timer when auto-update is on. macOS installs the new bundle in place right
//! away; it runs on the next launch, so no running agent is interrupted.
//! Installing a .deb needs root, and handing a file in the user's temp
//! directory to `pkexec dpkg -i` would let any process running as the user
//! (an agent, say) swap it before root reads it. So on Linux, and on macOS
//! when the app cannot replace itself, the check only reports the version and
//! the user installs it themselves.

use crate::error::{Error, Result};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct UpdateState {
    /// Version already installed, so later checks do not fetch it again. Held
    /// for the whole check, so overlapping checks never download twice.
    installed: Mutex<Option<String>>,
}

/// None when offline or GitHub is unreachable: the next check tries again,
/// so that is not worth reporting. Every other failure is an error.
fn unless_offline<T>(result: std::result::Result<T, tauri_plugin_updater::Error>) -> Result<Option<T>> {
    use tauri_plugin_updater::Error::{Network, Reqwest};
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Reqwest(_) | Network(_)) => Ok(None),
        Err(e) => Err(Error::Invalid(format!("update: {e}"))),
    }
}

fn writable(path: &Path) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else { return false };
    // SAFETY: plain syscall on a NUL-terminated path that outlives it.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

/// Whether this user can swap the app bundle in place. When not (an admin
/// installed it in a folder this user cannot write), the updater would ask
/// for an administrator password from a background check, again after every
/// cancel; the sidebar button is shown instead.
fn can_replace_bundle() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let Some(app) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) else { return false };
    writable(app) && app.parent().is_some_and(writable)
}

/// Installs a newer release if there is one. Returns the version the user
/// has to install by hand; None when up to date, offline or already installed.
#[tauri::command]
pub async fn update_check(app: AppHandle, state: State<'_, UpdateState>) -> Result<Option<String>> {
    // `cargo run` and `tauri dev` builds have no installed bundle to replace.
    if cfg!(debug_assertions) || tauri::utils::platform::bundle_type().is_none() {
        return Ok(None);
    }
    let mut installed = state.installed.lock().await;
    let updater = app.updater().map_err(|e| Error::Invalid(format!("update: {e}")))?;
    let Some(Some(update)) = unless_offline(updater.check().await)? else {
        return Ok(None);
    };
    if cfg!(target_os = "linux") || !can_replace_bundle() {
        return Ok(Some(update.version));
    }
    if installed.as_deref() == Some(update.version.as_str()) {
        return Ok(None);
    }
    let Some(bytes) = unless_offline(update.download(|_, _| {}, || {}).await)? else {
        return Ok(None);
    };
    update.install(&bytes).map_err(|e| Error::Invalid(format!("update: {e}")))?;
    *installed = Some(update.version);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writable_follows_the_folder_permissions() {
        let dir = tempfile::tempdir().unwrap();
        assert!(writable(dir.path()));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!writable(dir.path()));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn offline_is_not_an_error_but_a_bad_signature_is() {
        let offline = unless_offline::<()>(Err(tauri_plugin_updater::Error::Network("down".into())));
        assert!(matches!(offline, Ok(None)));
        let bad = unless_offline::<()>(Err(tauri_plugin_updater::Error::MissingSignedVersion));
        assert!(bad.is_err());
    }
}
