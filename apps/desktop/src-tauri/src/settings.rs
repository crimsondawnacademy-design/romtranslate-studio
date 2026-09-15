//! Configuracao do app (spec §18): settings.toml legivel + API key no
//! SECRET STORAGE nativo do SO (Keychain/Credential Manager/Secret Service).
//! `secrets.json` 0600 vira fallback quando o keychain nao esta disponivel;
//! chaves legadas no arquivo migram para o keychain na primeira leitura.
//! A key NUNCA vai para settings.toml, logs, projeto ou repo.

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
const KEYCHAIN_SERVICE: &str = "RomTranslate Studio";
const KEYCHAIN_USER: &str = "openai_compatible_api_key";

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

fn keychain_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).ok()
}

/// Le a API key: keychain primeiro; arquivo legado como fallback (e, achando
/// legado com keychain funcional, migra e apaga o arquivo plaintext).
pub fn load_api_key(config_dir: &Path) -> Option<String> {
    if let Some(entry) = keychain_entry() {
        if let Ok(key) = entry.get_password() {
            if !key.is_empty() {
                return Some(key);
            }
        }
    }
    let legacy = load_api_key_file(config_dir)?;
    if let Some(entry) = keychain_entry() {
        if entry.set_password(&legacy).is_ok() {
            let _ = fs::remove_file(config_dir.join(SECRETS_FILE));
            tracing::info!("API key migrada do secrets.json para o keychain do sistema");
        }
    }
    Some(legacy)
}

/// Grava a API key no keychain (fallback: arquivo 0600). Chave vazia remove
/// dos dois lugares.
pub fn save_api_key(config_dir: &Path, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        if let Some(entry) = keychain_entry() {
            let _ = entry.delete_credential();
        }
        let _ = fs::remove_file(config_dir.join(SECRETS_FILE));
        return Ok(());
    }
    if let Some(entry) = keychain_entry() {
        if entry.set_password(key).is_ok() {
            // Nunca deixar copia plaintext quando o keychain aceitou.
            let _ = fs::remove_file(config_dir.join(SECRETS_FILE));
            return Ok(());
        }
    }
    tracing::warn!("keychain indisponivel; usando secrets.json 0600 como fallback");
    save_api_key_file(config_dir, key)
}

// ---- Fallback legado em arquivo (0600) ----

fn load_api_key_file(config_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(config_dir.join(SECRETS_FILE)).ok()?;
    let secrets: Secrets = serde_json::from_str(&text).ok()?;
    (!secrets.openai_api_key.is_empty()).then_some(secrets.openai_api_key)
}

fn save_api_key_file(config_dir: &Path, key: &str) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = config_dir.join(SECRETS_FILE);
    let secrets = Secrets {
        openai_api_key: key.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rts-set-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_roundtrip_and_defaults() {
        let dir = tempdir("cfg");
        let loaded = load_settings(&dir);
        assert_eq!(loaded.provider, "ollama");

        let mut s = AppSettings::default();
        s.openai_compatible.model = "gpt-5-mini".into();
        save_settings(&dir, &s).unwrap();
        assert_eq!(load_settings(&dir).openai_compatible.model, "gpt-5-mini");
        // settings.toml jamais carrega a key (nenhum campo de segredo existe).
        let text = fs::read_to_string(dir.join(SETTINGS_FILE))
            .unwrap()
            .to_lowercase();
        assert!(!text.contains("key") && !text.contains("secret"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_file_fallback_roundtrip() {
        let dir = tempdir("legacy");
        assert!(load_api_key_file(&dir).is_none());
        save_api_key_file(&dir, "sk-test-legacy").unwrap();
        assert_eq!(load_api_key_file(&dir).as_deref(), Some("sk-test-legacy"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Toca o keychain REAL do sistema (cria e apaga um item de teste).
    /// Roda manual: cargo test -p romtranslate-desktop -- --ignored
    #[test]
    #[ignore]
    fn keychain_roundtrip_real() {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, "rts_test_key").unwrap();
        entry.set_password("valor-de-teste").unwrap();
        assert_eq!(entry.get_password().unwrap(), "valor-de-teste");
        entry.delete_credential().unwrap();
        assert!(entry.get_password().is_err());
    }
}
