use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Nes,
    Snes,
    Gba,
    Nds,
    GameCube,
    Wii,
    WiiU,
    Ps1,
    Ps2,
    Psp,
    /// Fixture sintetica do proprio RomTranslate (formato RTSF, para demo/testes).
    Synthetic,
    Unknown,
}

impl Platform {
    pub fn display_name(&self) -> &'static str {
        match self {
            Platform::Nes => "Nintendo Entertainment System",
            Platform::Snes => "Super Nintendo",
            Platform::Gba => "Game Boy Advance",
            Platform::Nds => "Nintendo DS",
            Platform::GameCube => "GameCube",
            Platform::Wii => "Wii",
            Platform::WiiU => "Wii U",
            Platform::Ps1 => "PlayStation",
            Platform::Ps2 => "PlayStation 2",
            Platform::Psp => "PSP",
            Platform::Synthetic => "Fixture sintetica (RTSF)",
            Platform::Unknown => "Desconhecida",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Full,
    Partial,
    ExtractOnly,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Created,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationStatus {
    #[default]
    Untranslated,
    Machine,
    Reviewed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextEncoding {
    #[default]
    Ascii,
    Utf8,
    Utf16Le,
    Utf16Be,
    ShiftJis,
    /// Tabela customizada (.tbl) identificada por id.
    Table(String),
}

/// O que um adapter declara saber fazer. Probes do Sprint 1 so detectam.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterCapabilities {
    pub detect: bool,
    pub extract: bool,
    pub reinsert: bool,
    pub patch: bool,
    pub compression: bool,
    pub pointer_relocation: bool,
    pub font_table: bool,
    pub experimental: bool,
    pub support_level: SupportLevel,
}

impl AdapterCapabilities {
    pub fn detect_only() -> Self {
        AdapterCapabilities {
            detect: true,
            extract: false,
            reinsert: false,
            patch: false,
            compression: false,
            pointer_relocation: false,
            font_table: false,
            experimental: true,
            support_level: SupportLevel::Experimental,
        }
    }
}

/// Recurso interno de um container (arquivo num filesystem de cartucho/disco).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDescriptor {
    pub path: String,
    /// Offset absoluto do recurso dentro da imagem.
    pub offset: u64,
    pub size: u64,
}

/// Resultado de um probe. `confidence` em [0.0, 1.0]; `evidence` explica o porque.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub adapter_id: String,
    pub platform: Platform,
    pub confidence: f32,
    pub evidence: Vec<String>,
    /// Preenchido pelo pipeline de deteccao a partir das capabilities do adapter.
    #[serde(default = "default_support")]
    pub support_level: SupportLevel,
}

fn default_support() -> SupportLevel {
    SupportLevel::Unsupported
}

impl ProbeResult {
    pub fn no_match(adapter_id: &str, platform: Platform) -> Self {
        ProbeResult {
            adapter_id: adapter_id.to_string(),
            platform,
            confidence: 0.0,
            evidence: Vec::new(),
            support_level: SupportLevel::Unsupported,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProject {
    pub id: Uuid,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub source_size: u64,
    pub platform: Platform,
    pub adapter_id: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
}

/// Unidade de texto extraida. Usada a partir do Sprint 2; ja definida aqui
/// para o modelo de dominio ficar estavel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEntry {
    pub id: String,
    pub resource_path: Option<String>,
    pub offset: Option<u64>,
    #[serde(with = "serde_bytes_hex")]
    pub original_bytes: Vec<u8>,
    pub source_text: String,
    pub translated_text: Option<String>,
    pub context: Option<String>,
    pub max_bytes: Option<usize>,
    pub encoding: TextEncoding,
    pub status: TranslationStatus,
    pub metadata: serde_json::Value,
}

/// Serializa bytes como hex — legivel em project.json/exports e sem base64 dep.
mod serde_bytes_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        s.serialize_str(&hex)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let hex = String::deserialize(d)?;
        if hex.len() % 2 != 0 {
            return Err(serde::de::Error::custom("hex de tamanho impar"));
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&hex[i..i + 2], 16)
                    .map_err(|e| serde::de::Error::custom(format!("hex invalido: {e}")))
            })
            .collect()
    }
}
