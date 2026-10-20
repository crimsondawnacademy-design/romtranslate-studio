//! Discos da NEC: PC Engine CD-ROM² / TurboGrafx-CD e PC-FX. Quase nenhum tem
//! ISO 9660: o jogo le a trilha de dados por numero de setor. Entao a trilha
//! e um bloco, como cartucho — varredura ASCII e Shift-JIS, reinsercao no
//! espaco da original, EDC/ECC refeito no setor raw de 2352 (Mode 1).
//!
//! Assinatura, conferida em 20/10/2026 no codigo aberto que reconhece esses
//! discos (rcheevos `rc_hash_disc.c`, FBNeo `cdlist.cpp`, `pce-mkcd` do
//! llvm-mos, Aaru) — nenhum byte de jogo:
//! - PC Engine CD: setor 1 da trilha, "PC Engine CD-ROM SYSTEM" em 0x20 e
//!   "Copyright HUDSON SOFT / NEC Home Electronics,Ltd." em 0x38; nome do
//!   jogo em 0x6A, 22 bytes;
//! - PC-FX: setor 0 comeca com "PC-FX:Hu_CD-ROM "; nome do jogo nos 32
//!   primeiros bytes do setor 1.
//!
//! Os setores 0 e 1 sao o que o BIOS confere pra dar partida: ficam fora da
//! extracao. Dump do Redump: o arquivo da trilha 2 comeca pelos 3 s de pregap
//! (INDEX 00); a trilha e achada pela assinatura nos primeiros setores.
//!
//! Lista de pedidos do Data Crystal (20/10/2026): PC Engine CD 77 jogos,
//! PC-FX 18, quase todos japoneses.

use std::ops::Range;

use super::iso9660::{apply_iso, raw_edc_scan, SectorMap, SECTOR_DATA, SECTOR_RAW, SYNC};
use crate::adapter::{AppliedImage, Extraction, GameAdapter, GameInput, VerificationReport};
use crate::error::{CoreError, Result};
use crate::scan::{scan_bytes, ScanConfig, ScanEncoding};
use crate::types::{AdapterCapabilities, Platform, ProbeResult, SupportLevel, TextEntry};

pub struct Disc {
    id: &'static str,
    name: &'static str,
    platform: Platform,
    /// Setor da trilha (0 ou 1) e posicao nele onde a assinatura comeca.
    sector: usize,
    at: usize,
    tag: &'static [u8],
    /// Texto do mesmo setor que confirma o console.
    maker: Option<&'static [u8]>,
    /// Onde fica o nome do jogo no setor 1.
    title: Range<usize>,
}

pub static DISCS: [Disc; 2] = [
    Disc {
        id: "pcecd.cd",
        name: "PC Engine CD-ROM² / TurboGrafx-CD",
        platform: Platform::PcEngineCd,
        sector: 1,
        at: 0x20,
        tag: b"PC Engine CD-ROM SYSTEM",
        maker: Some(b"NEC Home Electronics"),
        title: 0x6A..0x80,
    },
    Disc {
        id: "pcfx.cd",
        name: "PC-FX",
        platform: Platform::PcFx,
        sector: 0,
        at: 0,
        tag: b"PC-FX:Hu_CD-ROM",
        maker: None,
        title: 0..0x20,
    },
];

/// Os dois primeiros setores da trilha: a partida que o BIOS confere.
const BOOT_SECTORS: usize = 2;
/// Setores de pregap que podem vir antes da trilha no arquivo. O Redump
/// grava 3 s (225 setores); 10 s de folga.
const MAX_PREGAP: usize = 750;

/// Onde a trilha de dados esta no arquivo.
#[derive(Debug, Clone, Copy)]
struct Track {
    raw: bool,
    /// Setor do arquivo em que a trilha comeca (depois do pregap).
    first: usize,
}

impl Track {
    fn map(self) -> SectorMap {
        if self.raw {
            SectorMap::Raw2352
        } else {
            SectorMap::Plain2048
        }
    }

    /// Onde comecam os 2048 bytes de dados do setor `k` do arquivo. `None` se
    /// ele nao esta inteiro no arquivo ou, no raw, nao e setor Mode 1.
    fn user(self, data: &[u8], k: usize) -> Option<usize> {
        if self.raw {
            let base = k.checked_mul(SECTOR_RAW)?;
            let sector = data.get(base..base.checked_add(SECTOR_RAW)?)?;
            (sector[..SYNC.len()] == SYNC && sector[15] == 1).then_some(base + 16)
        } else {
            let base = k.checked_mul(SECTOR_DATA)?;
            data.get(base..base.checked_add(SECTOR_DATA)?)?;
            Some(base)
        }
    }

    /// Os dados do setor `n` da trilha.
    fn sector(self, data: &[u8], n: usize) -> Option<&[u8]> {
        let at = self.user(data, self.first + n)?;
        data.get(at..at + SECTOR_DATA)
    }

    /// Onde comecam os dados de cada setor que a extracao varre: depois da
    /// partida, ate o fim do arquivo.
    fn game_sectors(self, data: &[u8]) -> Vec<usize> {
        let size = if self.raw { SECTOR_RAW } else { SECTOR_DATA };
        (self.first + BOOT_SECTORS..data.len() / size)
            .filter_map(|k| self.user(data, k))
            .collect()
    }
}

/// A trilha de `disc` no arquivo: a assinatura no lugar dela, em imagem raw
/// ou de 2048, depois de ate `MAX_PREGAP` setores.
fn find(disc: &Disc, data: &[u8]) -> Option<Track> {
    [true, false].into_iter().find_map(|raw| {
        (0..=MAX_PREGAP)
            .map(|first| Track { raw, first })
            .find(|t| {
                t.sector(data, disc.sector)
                    .and_then(|s| s.get(disc.at..disc.at + disc.tag.len()))
                    == Some(disc.tag)
            })
    })
}

/// O nome gravado na partida (ASCII ou Shift-JIS), ate o primeiro byte que
/// nao e letra.
fn title(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some((c, n)) = crate::sjis::decode_char(bytes, i) {
        out.push(c);
        i += n;
    }
    out.trim().to_string()
}

pub struct NecDisc(pub &'static Disc);

impl NecDisc {
    fn locate(&self, data: &[u8]) -> Option<Track> {
        let track = find(self.0, data)?;
        // Disco de PC-FX que tambem traz a marca do PC Engine (o Mednafen
        // cita o Battle Heat): e de PC-FX.
        if self.0.platform == Platform::PcEngineCd && find(&DISCS[1], data).is_some() {
            return None;
        }
        Some(track)
    }

    fn check(&self, data: &[u8]) -> Result<Track> {
        self.locate(data).ok_or_else(|| {
            CoreError::Project(format!(
                "{}: imagem sem a assinatura do console no setor {} da trilha de dados",
                self.0.name, self.0.sector
            ))
        })
    }
}

impl GameAdapter for NecDisc {
    fn id(&self) -> &'static str {
        self.0.id
    }

    fn display_name(&self) -> &'static str {
        self.0.name
    }

    fn platform(&self) -> Platform {
        self.0.platform
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
        // O pregap do Redump (225 setores raw) passa da janela do probe.
        let whole = input.whole();
        let data: &[u8] = whole.as_deref().unwrap_or(&input.head);
        let Some(track) = self.locate(data) else {
            return ProbeResult::no_match(self.id(), self.platform());
        };
        let mut confidence = 0.9;
        let mut evidence = vec![format!(
            "assinatura {:?} no setor {} da trilha de dados",
            String::from_utf8_lossy(self.0.tag),
            self.0.sector
        )];
        let boot = track.sector(data, self.0.sector).unwrap_or_default();
        if let Some(maker) = self.0.maker {
            if boot.windows(maker.len()).any(|w| w == maker) {
                confidence += 0.05;
                evidence.push(format!(
                    "{:?} no mesmo setor",
                    String::from_utf8_lossy(maker)
                ));
            }
        }
        let name = track
            .sector(data, 1)
            .and_then(|s| s.get(self.0.title.clone()))
            .map(title)
            .unwrap_or_default();
        if !name.is_empty() {
            evidence.push(format!("nome do jogo: {name:?}"));
        }
        if track.first > 0 {
            evidence.push(format!(
                "{} setores de pregap antes da trilha (dump do Redump)",
                track.first
            ));
        }
        if track.raw {
            evidence.push("imagem raw de CD (2352 bytes/setor, Mode 1)".to_string());
        }
        ProbeResult {
            confidence,
            evidence,
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }

    /// A trilha (sem a partida) na ordem em que o jogo a le, varrida de uma
    /// vez em ASCII e Shift-JIS; cada frase volta pro lugar dela na imagem. No
    /// raw os dados de um setor nao continuam no seguinte (EDC/ECC no meio): a
    /// frase que passa de um pro outro fica de fora, contada como descartada.
    // ponytail: copia a trilha inteira pra varrer (pico ~2x o disco, 650 MB
    // no maximo); varrer por regiao se um disco real apertar a memoria.
    fn extract(&self, data: &[u8]) -> Result<Extraction> {
        let track = self.check(data)?;
        let starts = track.game_sectors(data);
        let mut block = Vec::with_capacity(starts.len() * SECTOR_DATA);
        for &at in &starts {
            block.extend_from_slice(&data[at..at + SECTOR_DATA]);
        }
        let outcome = scan_bytes(
            &block,
            &ScanConfig {
                encoding: ScanEncoding::Ascii,
                japanese: true,
                only_text: true,
                skip_noise: true,
                ..ScanConfig::default()
            },
        )?;
        let mut out = Extraction {
            discarded: outcome.discarded,
            truncated: outcome.truncated,
            ..Extraction::default()
        };
        for mut e in outcome.entries {
            let rel = e.offset.unwrap_or_default() as usize;
            let len = e.original_bytes.len().max(1);
            let k = rel / SECTOR_DATA;
            let Some(&start) = starts.get(k) else {
                continue;
            };
            if track.raw && (rel + len - 1) / SECTOR_DATA != k {
                out.discarded += 1;
                continue;
            }
            let at = start + rel % SECTOR_DATA;
            e.id = format!("scan-{at:08x}");
            e.offset = Some(at as u64);
            e.max_bytes = Some(e.original_bytes.len());
            e.context = Some("in-place: traducao limitada ao espaco original".to_string());
            out.entries.push(e);
        }
        Ok(out)
    }

    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> {
        let track = self.check(data)?;
        apply_iso(data, track.map(), entries, false)
    }

    fn verify(&self, data: &[u8]) -> Result<VerificationReport> {
        let mut checks = Vec::new();
        let mut problems = Vec::new();
        match self.locate(data) {
            Some(track) => {
                checks.push(format!("assinatura de {} preservada", self.0.name));
                if track.raw {
                    let (ok, bad, _) = raw_edc_scan(data);
                    if bad == 0 {
                        checks.push(format!("EDC integro em {ok} setores"));
                    } else {
                        problems.push(format!("{bad} setores com EDC invalido"));
                    }
                }
            }
            None => problems.push(format!(
                "assinatura de {} ausente: o disco nao dara partida",
                self.0.name
            )),
        }
        Ok(VerificationReport {
            ok: problems.is_empty(),
            checks,
            problems,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::cdrom::sector_edc_ok;
    use crate::synth;
    use crate::types::TextEncoding;

    fn adapter(id: &str) -> NecDisc {
        NecDisc(DISCS.iter().find(|d| d.id == id).unwrap())
    }

    fn probe(id: &str, name: &str, data: &[u8]) -> f32 {
        adapter(id)
            .probe(&GameInput::from_bytes(name, data))
            .confidence
    }

    /// O adapter que fica com o arquivo, entre todos os do app.
    fn best(name: &str, data: &[u8]) -> &'static str {
        let input = GameInput::from_bytes(name, data);
        crate::adapters::all()
            .iter()
            .map(|a| (a.probe(&input).confidence, a.id()))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
            .unwrap()
    }

    #[test]
    fn each_disc_is_told_apart_from_the_other_and_from_the_other_consoles() {
        let pce = synth::make_pce_cd_bin(3);
        let fx = synth::make_pcfx_iso();
        assert!(probe("pcecd.cd", "a.bin", &pce) >= 0.9);
        assert_eq!(probe("pcfx.cd", "a.bin", &pce), 0.0);
        assert!(probe("pcfx.cd", "a.iso", &fx) >= 0.9);
        assert_eq!(probe("pcecd.cd", "a.iso", &fx), 0.0);
        assert_eq!(best("a.bin", &pce), "pcecd.cd");
        assert_eq!(best("a.iso", &fx), "pcfx.cd");
        let input = GameInput::from_bytes("a.bin", &pce);
        let evidence = adapter("pcecd.cd").probe(&input).evidence;
        assert!(
            evidence.iter().any(|e| e.contains("SYNTH PCE CD")),
            "{evidence:?}"
        );
        assert!(
            evidence.iter().any(|e| e.contains("NEC Home")),
            "{evidence:?}"
        );
        assert!(evidence.iter().any(|e| e.contains("3 setores de pregap")));
        for (name, other) in [
            ("a.bin", synth::make_ps1_bin()),
            ("a.bin", synth::make_saturn_bin()),
            ("a.iso", synth::make_mega_cd_iso()),
        ] {
            assert_ne!(best(name, &other), "pcecd.cd");
            for d in &DISCS {
                assert_eq!(probe(d.id, name, &other), 0.0, "{} pegou outro disco", d.id);
            }
        }
        // Disco de PC-FX com a marca do PC Engine tambem: e de PC-FX.
        let mut both = fx.clone();
        both[2048 + 0x20..2048 + 0x37].copy_from_slice(b"PC Engine CD-ROM SYSTEM");
        assert_eq!(probe("pcecd.cd", "a.iso", &both), 0.0);
        assert!(probe("pcfx.cd", "a.iso", &both) >= 0.9);
    }

    #[test]
    fn noise_empty_and_cut_discs_do_not_panic() {
        let pce = synth::make_pce_cd_bin(3);
        let fx = synth::make_pcfx_iso();
        let noise = synth::make_random(1 << 16, 7);
        let cuts = [
            &[][..],
            &noise[..],
            &pce[..10],
            &pce[..4 * SECTOR_RAW + 0x30],
            &pce[..5 * SECTOR_RAW + 7],
            &pce[..pce.len() - 1],
            &fx[..0x10],
            &fx[..SECTOR_DATA + 5],
            &fx[..fx.len() - 1],
        ];
        for data in cuts {
            for d in &DISCS {
                let a = adapter(d.id);
                let _ = a.probe(&GameInput::from_bytes("x.bin", data));
                let _ = a.extract(data);
                let _ = a.apply_text(data, &[]);
                let _ = a.verify(data);
            }
        }
        for d in &DISCS {
            assert_eq!(probe(d.id, "x.bin", &[]), 0.0);
            assert_eq!(probe(d.id, "x.bin", &noise), 0.0);
            assert!(adapter(d.id).extract(&noise).is_err());
        }
        // Corte no meio da partida: sem assinatura, sem disco.
        assert_eq!(
            probe("pcecd.cd", "x.bin", &pce[..4 * SECTOR_RAW + 0x30]),
            0.0
        );
        // Corte no fim: a trilha que sobrou ainda e lida.
        assert!(adapter("pcecd.cd").extract(&pce[..pce.len() - 1]).is_ok());
    }

    #[test]
    fn a_translation_goes_in_place_with_the_sector_redone() {
        let disc = synth::make_pce_cd_bin(3);
        let pce = adapter("pcecd.cd");
        let extraction = pce.extract(&disc).unwrap();
        let mut entries = extraction.entries;
        let texts: Vec<&str> = entries.iter().map(|e| e.source_text.as_str()).collect();
        // A partida fica fora; a frase que atravessa o setor tambem.
        assert!(!texts
            .iter()
            .any(|t| t.contains("CD-ROM SYSTEM") || t.contains("NOTICE")));
        assert!(!texts.iter().any(|t| t.contains("CROSSES")), "{texts:?}");
        assert!(extraction.discarded >= 1);
        let japanese = entries
            .iter()
            .position(|e| e.source_text.contains("ぼうけん"))
            .expect("o japones em Shift-JIS sai");
        let english = entries
            .iter()
            .position(|e| e.source_text == "PRESS RUN BUTTON")
            .expect("o ASCII sai");
        assert_eq!(entries[japanese].encoding, TextEncoding::ShiftJis);
        entries[english].translated_text = Some("APERTE RUN".into());
        entries[japanese].translated_text = Some("Aventura!".into());
        let out = pce.apply_text(&disc, &entries).unwrap();
        assert_eq!(out.report.applied, 2);
        assert_eq!(out.bytes.len(), disc.len());
        let changed: Vec<usize> = (0..disc.len() / SECTOR_RAW)
            .filter(|&k| {
                let at = k * SECTOR_RAW..(k + 1) * SECTOR_RAW;
                out.bytes[at.clone()] != disc[at]
            })
            .collect();
        assert_eq!(changed, [3 + BOOT_SECTORS], "so o setor do texto muda");
        let sector = &out.bytes[changed[0] * SECTOR_RAW..][..SECTOR_RAW];
        assert_eq!(sector_edc_ok(sector), Some(true), "EDC/ECC refeito");
        let report = pce.verify(&out.bytes).unwrap();
        assert!(report.ok, "{:?}", report.problems);
        assert!(report.checks.iter().any(|c| c.contains("EDC")));
        let again = pce.extract(&out.bytes).unwrap().entries;
        assert!(again.iter().any(|e| e.source_text == "APERTE RUN"));
        assert!(again.iter().any(|e| e.source_text == "Aventura!"));
    }

    #[test]
    fn the_pc_fx_iso_is_one_block_and_keeps_the_line_across_sectors() {
        let disc = synth::make_pcfx_iso();
        let fx = adapter("pcfx.cd");
        let mut entries = fx.extract(&disc).unwrap().entries;
        assert!(!entries.iter().any(|e| e.source_text.contains("Hu_CD")));
        let across = entries
            .iter()
            .position(|e| e.source_text == "THIS LINE CROSSES THE SECTOR")
            .expect("na ISO de 2048 os setores sao continuos");
        assert!(entries.iter().any(|e| e.source_text.contains("ぼうけん")));
        entries[across].translated_text = Some("ESTA LINHA PASSA DO SETOR".into());
        let out = fx.apply_text(&disc, &entries).unwrap();
        assert_eq!(out.report.applied, 1);
        assert!(fx.verify(&out.bytes).unwrap().ok);
        assert!(out
            .bytes
            .windows(25)
            .any(|w| w == b"ESTA LINHA PASSA DO SETOR"));
    }
}
