use crate::error::Result;
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use ts_rs::TS;

const CONFIG_VERSION: u32 = 1;
/// Bundle identifier of the first builds; its config is copied over once.
const LEGACY_IDENTIFIER: &str = "dev.orchard.app";

#[derive(Serialize, Deserialize, TS, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Language {
    En,
    Pt,
}

/// An unknown language (hand-edited file, removed locale) falls back to the
/// system language instead of rejecting the whole config.
fn lenient_language<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Language>, D::Error> {
    Ok(Option::<serde_json::Value>::deserialize(d)?.and_then(|v| serde_json::from_value(v).ok()))
}

#[derive(Serialize, Deserialize, TS, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Config {
    pub version: u32,
    pub repos: Vec<String>,
    /// Worktree paths shown at the top of the sidebar, in pin order.
    pub pinned: Vec<String>,
    pub agent_command: String,
    pub lazygit_command: String,
    pub editor_command: String,
    /// Optional root for new worktrees; empty means next to the repo.
    pub worktree_root: String,
    #[serde(deserialize_with = "lenient_language")]
    pub language: Option<Language>,
    /// Check GitHub Releases for a new version and install it in the background.
    pub auto_update: bool,
    /// Fetch `origin` and open PR heads now and then, to check worktrees
    /// against them for conflicts.
    pub fetch_remote: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            repos: Vec::new(),
            pinned: Vec::new(),
            agent_command: "claude".into(),
            lazygit_command: "lazygit".into(),
            editor_command: "code".into(),
            worktree_root: String::new(),
            language: None,
            auto_update: true,
            fetch_remote: false,
        }
    }
}

fn config_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| crate::error::Error::Invalid(e.to_string()))?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("config.json");
    if let Some(parent) = dir.parent() {
        migrate_legacy(&path, &parent.join(LEGACY_IDENTIFIER).join("config.json"))?;
    }
    Ok(path)
}

/// Copies the config written under the old bundle identifier when the new
/// location has none yet. The old file is left in place as a backup.
fn migrate_legacy(path: &Path, legacy: &Path) -> Result<()> {
    if path != legacy && !path.exists() && legacy.exists() {
        fs::copy(legacy, path)?;
    }
    Ok(())
}

#[derive(Serialize, TS, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoadedConfig {
    pub config: Config,
    /// Where an unreadable config was moved, so the UI can tell the user.
    pub corrupt_backup: Option<String>,
}

/// Defaults only when the file does not exist. A corrupt file is moved aside
/// so the next save cannot overwrite the user's repos; any other read error
/// (permissions, I/O) is reported instead of silently starting empty.
fn load(path: &Path) -> Result<LoadedConfig> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(LoadedConfig { config: Config::default(), corrupt_backup: None });
        }
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_str(&raw) {
        Ok(config) => Ok(LoadedConfig { config, corrupt_backup: None }),
        Err(_) => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            let backup = path.with_extension(format!("json.corrupt-{stamp}"));
            fs::rename(path, &backup)?;
            Ok(LoadedConfig { config: Config::default(), corrupt_backup: Some(backup.to_string_lossy().into_owned()) })
        }
    }
}

/// Writes to a temporary file and renames it, so a crash mid-write never
/// leaves a truncated config.
fn save(path: &Path, config: &Config) -> Result<()> {
    let raw = serde_json::to_string_pretty(&Config { version: CONFIG_VERSION, ..config.clone() })?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[tauri::command]
pub async fn config_load(app: AppHandle) -> Result<LoadedConfig> {
    load(&config_path(&app)?)
}

#[tauri::command]
pub async fn config_save(app: AppHandle, config: Config) -> Result<()> {
    save(&config_path(&app)?, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_is_copied_once() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("old.json");
        let path = dir.path().join("config.json");
        fs::write(&legacy, r#"{"repos":["/r"]}"#).unwrap();
        migrate_legacy(&path, &legacy).unwrap();
        assert_eq!(load(&path).unwrap().config.repos, vec!["/r".to_string()]);
        assert!(legacy.exists());

        // An existing config is never overwritten by the legacy one.
        fs::write(&legacy, r#"{"repos":["/other"]}"#).unwrap();
        migrate_legacy(&path, &legacy).unwrap();
        assert_eq!(load(&path).unwrap().config.repos, vec!["/r".to_string()]);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("config.json")).unwrap().config, Config::default());
    }

    #[test]
    fn round_trips_and_keeps_old_files_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // A file written before `version` existed, with an unknown language.
        fs::write(&path, r#"{"repos":["/r"],"pinned":["/r"],"language":"xx"}"#).unwrap();
        let config = load(&path).unwrap().config;
        assert_eq!(config.repos, vec!["/r".to_string()]);
        assert_eq!(config.language, None);
        assert_eq!(config.agent_command, "claude");
        assert!(config.auto_update);
        assert!(!config.fetch_remote);

        save(&path, &Config { language: Some(Language::Pt), ..config }).unwrap();
        let back = load(&path).unwrap().config;
        assert_eq!(back.language, Some(Language::Pt));
        assert_eq!(back.version, CONFIG_VERSION);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "{ not json").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.corrupt_backup.is_some_and(|b| Path::new(&b).exists()));
        assert!(!path.exists());
        let backups: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(backups.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_file_is_an_error_not_defaults() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(load(&path).is_err());
    }
}
