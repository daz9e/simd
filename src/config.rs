use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub user: String,
    pub password_hash: String,
}

pub fn read_config(path: &Path) -> Option<AppConfig> {
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn write_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let data = serde_json::to_string_pretty(config)
        .map_err(|_| "Failed to serialize config".to_string())?;
    fs::write(path, &data).map_err(|_| "Failed to write config".to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "Failed to set config permissions".to_string())?;
    Ok(())
}