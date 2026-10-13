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

/// ChatGPT pelo Codex CLI: sem URL nem chave — o login e do proprio Codex.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CodexCliSettings {
    /// Nome ou caminho do executavel.
    pub command: String,
    /// Vazio = o modelo padrao do Codex.
    pub model: String,
    /// `true` = o app usa um login so dele (pasta propria); `false` = a conta
    /// em que o Codex deste computador ja esta logado.
    pub own_account: bool,
}

impl Default for CodexCliSettings {
    fn default() -> Self {
        CodexCliSettings {
            command: romtranslate_core::providers::codex_cli::DEFAULT_COMMAND.to_string(),
            model: String::new(),
            own_account: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub provider: String,
    pub ollama: EndpointSettings,
    pub openai_compatible: EndpointSettings,
    /// Claude pela API da Anthropic, com chave de API.
    pub anthropic: EndpointSettings,
    pub codex_cli: CodexCliSettings,
    /// Preco em dolares por MILHAO de tokens, pra conta de gasto. Zero = nao
    /// informado (a tela usa a tabela dos modelos que conhece).
    pub price_input: f64,
    pub price_output: f64,
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
            anthropic: EndpointSettings {
                base_url: romtranslate_core::providers::anthropic::DEFAULT_BASE_URL.to_string(),
                model: romtranslate_core::providers::anthropic::DEFAULT_MODEL.to_string(),
            },
            codex_cli: CodexCliSettings::default(),
            price_input: 0.0,
            price_output: 0.0,
            allow_remote_translation: false,
            batch_size: 10,
            timeout_secs: 120,
        }
    }
}

const SETTINGS_FILE: &str = "settings.toml";
const SECRETS_FILE: &str = "secrets.json";
const KEYCHAIN_SERVICE: &str = "RomTranslate Studio";

/// De quem e a chave. Cada uma tem a sua entrada no cofre do sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKey {
    OpenAi,
    Anthropic,
}

impl ApiKey {
    fn keychain_user(self) -> &'static str {
        match self {
            ApiKey::OpenAi => "openai_compatible_api_key",
            ApiKey::Anthropic => "anthropic_api_key",
        }
    }

    fn slot(self, secrets: &mut Secrets) -> &mut String {
        match self {
            ApiKey::OpenAi => &mut secrets.openai_api_key,
            ApiKey::Anthropic => &mut secrets.anthropic_api_key,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Secrets {
    #[serde(default)]
    openai_api_key: String,
    #[serde(default)]
    anthropic_api_key: String,
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

fn keychain_entry(which: ApiKey) -> Option<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, which.keychain_user()).ok()
}

/// Le a API key: keychain primeiro; arquivo legado como fallback (e, achando
/// legado com keychain funcional, migra e tira a chave do arquivo plaintext).
pub fn load_api_key(config_dir: &Path, which: ApiKey) -> Option<String> {
    if let Some(entry) = keychain_entry(which) {
        if let Ok(key) = entry.get_password() {
            if !key.is_empty() {
                return Some(key);
            }
        }
    }
    let legacy = load_api_key_file(config_dir, which)?;
    if let Some(entry) = keychain_entry(which) {
        if entry.set_password(&legacy).is_ok() {
            let _ = save_api_key_file(config_dir, which, "");
            tracing::info!("API key migrada do secrets.json para o keychain do sistema");
        }
    }
    Some(legacy)
}

/// Grava a API key no keychain (fallback: arquivo 0600). Chave vazia remove
/// dos dois lugares.
pub fn save_api_key(config_dir: &Path, which: ApiKey, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        if let Some(entry) = keychain_entry(which) {
            let _ = entry.delete_credential();
        }
        return save_api_key_file(config_dir, which, "");
    }
    if let Some(entry) = keychain_entry(which) {
        if entry.set_password(key).is_ok() {
            // Nunca deixar copia plaintext quando o keychain aceitou.
            return save_api_key_file(config_dir, which, "");
        }
    }
    tracing::warn!("keychain indisponivel; usando secrets.json 0600 como fallback");
    save_api_key_file(config_dir, which, key)
}

// ---- Fallback legado em arquivo (0600) ----

fn read_secrets(config_dir: &Path) -> Secrets {
    fs::read_to_string(config_dir.join(SECRETS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn load_api_key_file(config_dir: &Path, which: ApiKey) -> Option<String> {
    let mut secrets = read_secrets(config_dir);
    let key = std::mem::take(which.slot(&mut secrets));
    (!key.is_empty()).then_some(key)
}

/// Grava (ou apaga, com chave vazia) UMA chave no arquivo, preservando a
/// outra. Sem nenhuma chave, o arquivo deixa de existir.
fn save_api_key_file(config_dir: &Path, which: ApiKey, key: &str) -> Result<(), String> {
    let path = config_dir.join(SECRETS_FILE);
    let mut secrets = read_secrets(config_dir);
    *which.slot(&mut secrets) = key.to_string();
    if secrets.openai_api_key.is_empty() && secrets.anthropic_api_key.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
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
        assert_eq!(loaded.codex_cli.command, "codex");
        assert_eq!(loaded.anthropic.model, "claude-opus-5-5");

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
        assert!(load_api_key_file(&dir, ApiKey::OpenAi).is_none());
        save_api_key_file(&dir, ApiKey::OpenAi, "sk-test-legacy").unwrap();
        assert_eq!(
            load_api_key_file(&dir, ApiKey::OpenAi).as_deref(),
            Some("sk-test-legacy")
        );
        // As duas chaves convivem no arquivo; apagar uma preserva a outra, e
        // sem nenhuma o arquivo some.
        save_api_key_file(&dir, ApiKey::Anthropic, "sk-ant-test").unwrap();
        assert_eq!(
            load_api_key_file(&dir, ApiKey::OpenAi).as_deref(),
            Some("sk-test-legacy")
        );
        save_api_key_file(&dir, ApiKey::OpenAi, "").unwrap();
        assert!(load_api_key_file(&dir, ApiKey::OpenAi).is_none());
        assert_eq!(
            load_api_key_file(&dir, ApiKey::Anthropic).as_deref(),
            Some("sk-ant-test")
        );
        save_api_key_file(&dir, ApiKey::Anthropic, "").unwrap();
        assert!(!dir.join(SECRETS_FILE).exists());
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
