//! Adapter de PSP (UMD): ISO 9660 2048/setor com `UMD_DATA.BIN` na raiz e
//! `PSP_GAME/PARAM.SFO` (formato SFO da psdevwiki: magic \0PSF + index de
//! entries de 16 bytes). O probe le TITLE e DISC_ID reais do SFO.

use super::iso9660::{
    self, apply_iso, detect_map, extract_ascii_by_file, parse_pvd, read_file, walk, SectorMap,
};
use super::ps1::verify_iso;
use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct PspAdapter;

/// Le um campo string de um PARAM.SFO (retorna None em qualquer malformacao).
pub fn sfo_string(sfo: &[u8], wanted_key: &str) -> Option<String> {
    if sfo.len() < 0x14 || &sfo[0..4] != b"\x00PSF" {
        return None;
    }
    let u32le = |off: usize| -> Option<u32> {
        Some(u32::from_le_bytes(sfo.get(off..off + 4)?.try_into().ok()?))
    };
    let key_table = u32le(0x08)? as usize;
    let data_table = u32le(0x0C)? as usize;
    let count = u32le(0x10)? as usize;
    if count > 256 {
        return None;
    }
    for i in 0..count {
        let entry = 0x14 + i * 16;
        let key_off = u16::from_le_bytes(sfo.get(entry..entry + 2)?.try_into().ok()?) as usize;
        let len = u32le(entry + 4)? as usize;
        let data_off = u32le(entry + 12)? as usize;
        let key_start = key_table.checked_add(key_off)?;
        let key_region = sfo.get(key_start..sfo.len().min(key_start + 64))?;
        let key_end = key_region.iter().position(|&b| b == 0)?;
        if &key_region[..key_end] != wanted_key.as_bytes() {
            continue;
        }
        let start = data_table.checked_add(data_off)?;
        let value = sfo.get(start..start.checked_add(len)?)?;
        let text_end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
        return Some(String::from_utf8_lossy(&value[..text_end]).into_owned());
    }
    None
}

fn find_psp_markers(data: &[u8], map: SectorMap) -> Option<(bool, Option<Vec<u8>>)> {
    let files = walk(data, map).ok()?;
    let has_umd = files
        .iter()
        .any(|f| f.path.eq_ignore_ascii_case("UMD_DATA.BIN"));
    let sfo = files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("PSP_GAME/PARAM.SFO"))
        .and_then(|f| read_file(data, map, f.extent, f.size.min(64 * 1024)).ok());
    Some((has_umd, sfo))
}

impl GameAdapter for PspAdapter {
    fn id(&self) -> &'static str {
        "psp.generic"
    }

    fn display_name(&self) -> &'static str {
        "PSP (UMD)"
    }

    fn platform(&self) -> Platform {
        Platform::Psp
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true,
            patch: true,
            compression: false,
            pointer_relocation: false,
            font_table: false,
            experimental: true,
            support_level: SupportLevel::Experimental,
        }
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        let map = detect_map(head);
        if parse_pvd(head, map).is_err() {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let Some((has_umd, sfo)) = find_psp_markers(head, map) else {
            return ProbeResult::no_match(self.id(), self.platform());
        };
        let has_sfo = sfo.is_some();
        if !has_umd && !has_sfo {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut confidence: f32 = 0.5;
        let mut evidence = vec!["ISO 9660 de UMD".to_string()];
        if has_umd {
            confidence += 0.2;
            evidence.push("UMD_DATA.BIN presente na raiz".to_string());
        }
        if let Some(sfo) = sfo {
            confidence += 0.25;
            evidence.push("PSP_GAME/PARAM.SFO presente".to_string());
            if let Some(title) = sfo_string(&sfo, "TITLE") {
                evidence.push(format!("titulo: \"{title}\""));
            }
            if let Some(id) = sfo_string(&sfo, "DISC_ID") {
                evidence.push(format!("disc id: {id}"));
            }
        }
        ProbeResult {
            confidence: confidence.min(0.97),
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        let map = detect_map(data);
        let files = walk(data, map)?;
        Ok(iso9660::to_resources(data, map, &files))
    }

    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        extract_ascii_by_file(data, detect_map(data))
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let map = detect_map(data);
        parse_pvd(data, map)
            .map_err(|_| CoreError::Project("psp: imagem sem ISO 9660 valido".to_string()))?;
        apply_iso(data, map, entries)
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        verify_iso(self, data, |map, d| {
            find_psp_markers(d, map).is_some_and(|(umd, sfo)| umd || sfo.is_some())
        })
    }
}
