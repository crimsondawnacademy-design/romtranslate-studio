//! Configuracao do app (spec §18): settings.toml legivel + secrets SEPARADOS
//! (secrets.json, 0600). API key nunca vai para settings, logs ou o repo.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EndpointSettings {
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub provider: String,
    pub ollama: EndpointSettings,
    pub openai_compatible: EndpointSettings,
    pub allow_remote_translation: bool,
    pub batch_size: usize,
    pub timeout_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            provider: "ollama".to_string(),
            ollama: EndpointSettings {
                base_url: "http://localhost:11434".to_string(),
                model: String::new(),
            },
            openai_compatible: EndpointSettings {
                base_url: "https://api.openai.com/v1".to_string(),
                model: String::new(),
            },
            allow_remote_translation: false,
            batch_size: 10,
            timeout_secs: 120,
        }
    }
}

const SETTINGS_FILE: &str = "settings.toml";
const SECRETS_FILE: &str = "secrets.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Secrets {
    #[serde(default)]
    openai_api_key: String,
}

pub fn load_settings(config_dir: &Path) -> AppSettings {
    let path = config_dir.join(SETTINGS_FILE);
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(config_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let text = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{SETTINGS_FILE}.tmp"));
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, config_dir.join(SETTINGS_FILE)).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_api_key(config_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(config_dir.join(SECRETS_FILE)).ok()?;
    let secrets: Secrets = serde_json::from_str(&text).ok()?;
    (!secrets.openai_api_key.is_empty()).then_some(secrets.openai_api_key)
}

/// Chave vazia remove o secret. Arquivo com permissao 0600 (dono apenas).
pub fn save_api_key(config_dir: &Path, key: &str) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = config_dir.join(SECRETS_FILE);
    if key.trim().is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let secrets = Secrets {
        openai_api_key: key.trim().to_string(),
    };
    let json = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
