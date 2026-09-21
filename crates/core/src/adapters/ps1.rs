//! Adapter de PlayStation (PS1): BIN raw 2352 ou ISO 2048, ISO 9660 com
//! system_id "PLAYSTATION" e SYSTEM.CNF com linha `BOOT =` (PS2 usa BOOT2 —
//! e o discriminador canonico). Extracao por arquivo do filesystem; a
//! reinsercao e in-place nos dois formatos — no raw, cada setor alterado tem
//! EDC/ECC regenerado (modulo `cdrom`, ECMA-130) e o verify confere o EDC.
//! Traducao maior passa se a string tiver ponteiro numa tabela de offsets do
//! arquivo: vai pra sobra do setor final dele (o hardware le setor inteiro).
//! So o PS1: PS2/PSP leem bytes, e a sobra nao chega garantida na RAM.

use super::iso9660::{
    self, annotate_sector_slack, apply_iso, detect_map, extract_ascii_by_file,
    identify_playstation, parse_pvd, walk, PsKind, SectorMap,
};
use crate::adapter::{AppliedImage, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::types::{
    AdapterCapabilities, Platform, ProbeResult, ResourceDescriptor, SupportLevel, TextEntry,
};

pub struct Ps1Adapter;

impl GameAdapter for Ps1Adapter {
    fn id(&self) -> &'static str {
        "ps1.generic"
    }

    fn display_name(&self) -> &'static str {
        "PlayStation (CD)"
    }

    fn platform(&self) -> Platform {
        Platform::Ps1
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            detect: true,
            extract: true,
            reinsert: true, // in-place; raw 2352 regenera EDC/ECC dos setores alterados
            patch: true,
            compression: false,
            pointer_relocation: true, // so dentro da sobra do setor final do arquivo
            font_table: false,
            experimental: true,
            support_level: SupportLevel::Experimental,
        }
    }

    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        let map = detect_map(head);
        let Ok(info) = parse_pvd(head, map) else {
            return ProbeResult::no_match(self.id(), self.platform());
        };
        if !info.system_id.contains("PLAYSTATION") {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        let mut evidence = vec![format!(
            "ISO 9660 com system id \"{}\" (volume \"{}\")",
            info.system_id, info.volume_id
        )];
        if map == SectorMap::Raw2352 {
            evidence.push("imagem raw de CD (2352 bytes/setor, BIN)".to_string());
        }
        match identify_playstation(head, map) {
            Some((PsKind::One, boot)) => {
                evidence.push(format!("SYSTEM.CNF: {boot}"));
                ProbeResult {
                    confidence: 0.96,
                    evidence,
                    ..ProbeResult::no_match(self.id(), self.platform())
                }
            }
            Some((PsKind::Two, _)) => ProbeResult::no_match(self.id(), self.platform()),
            None => {
                evidence.push(
                    "SYSTEM.CNF fora da janela de probe — PS1 vs PS2 confirma na extracao"
                        .to_string(),
                );
                ProbeResult {
                    // Raw 2352 e forte indicio de CD (PS1); DVD de PS2 nunca e raw.
                    confidence: if map == SectorMap::Raw2352 { 0.6 } else { 0.4 },
                    evidence,
                    ..ProbeResult::no_match(self.id(), self.platform())
                }
            }
        }
    }

    fn list_resources(&self, data: &[u8]) -> Result<Vec<ResourceDescriptor>> {
        let map = detect_map(data);
        let files = walk(data, map)?;
        Ok(iso9660::to_resources(data, map, &files))
    }

    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> {
        let map = detect_map(data);
        let mut entries = extract_ascii_by_file(data, map)?;
        annotate_sector_slack(data, map, &mut entries)?;
        Ok(entries)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let map = detect_map(data);
        parse_pvd(data, map)
            .map_err(|_| CoreError::Project("ps1: imagem sem ISO 9660 valido".to_string()))?;
        apply_iso(data, map, entries, true)
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        verify_iso(self, data, |map, d| {
            matches!(identify_playstation(d, map), Some((PsKind::One, _)))
        })
    }
}

/// Verify comum aos adapters ISO 9660: PVD + filesystem + re-extracao +
/// checagem de identidade da plataforma.
pub(super) fn verify_iso(
    adapter: &dyn GameAdapter,
    data: &[u8],
    identity: impl Fn(SectorMap, &[u8]) -> bool,
) -> Result<VerificationReport> {
    let mut checks = Vec::new();
    let mut problems = Vec::new();
    let map = detect_map(data);

    match parse_pvd(data, map) {
        Ok(info) => checks.push(format!("PVD integro (system id \"{}\")", info.system_id)),
        Err(e) => problems.push(format!("PVD invalido: {e}")),
    }
    match walk(data, map) {
        Ok(files) => checks.push(format!("filesystem integro: {} arquivos", files.len())),
        Err(e) => problems.push(format!("filesystem corrompido: {e}")),
    }
    if problems.is_empty() {
        if identity(map, data) {
            checks.push("identidade da plataforma confirmada".to_string());
        } else {
            problems.push("identidade da plataforma nao confere".to_string());
        }
        match adapter.extract_structured(data) {
            Ok(entries) => checks.push(format!("{} strings re-extraidas", entries.len())),
            Err(e) => problems.push(format!("re-extracao falhou: {e}")),
        }
        if map == SectorMap::Raw2352 {
            let (ok_count, bad, no_sync) = iso9660::raw_edc_scan(data);
            if bad == 0 {
                let mut line = format!("EDC integro em {ok_count} setores");
                if no_sync > 0 {
                    line.push_str(&format!(
                        " ({no_sync} setores sem sync — audio? — ignorados)"
                    ));
                }
                checks.push(line);
            } else {
                problems.push(format!("{bad} setores com EDC invalido"));
            }
        }
    }
    Ok(VerificationReport {
        ok: problems.is_empty(),
        checks,
        problems,
    })
}
