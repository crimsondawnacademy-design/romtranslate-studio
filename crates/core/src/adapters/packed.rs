//! Texto dentro de blocos LZ77 (Camada D da spec): as frases saem do bloco
//! descomprimido e voltam com o bloco recomprimido.
//!
//! - Dentro do bloco a traducao e in-place: o tamanho descomprimido nao muda,
//!   porque o jogo pode guardar posicoes pra dentro dele.
//! - O bloco recomprimido volta pro lugar se couber nos bytes que o original
//!   ocupava. Se nao couber, so muda de lugar com ponteiro em TABELA — quem
//!   acha a tabela e reaponta e o adapter, que conhece o formato do ponteiro.
//! - Sem uma coisa nem outra o bloco fica como no original e as traducoes
//!   dele sao contadas em `ApplyReport::packed_left_out`: nada invalido e
//!   gravado, e nada some em silencio.

use std::collections::BTreeMap;

use super::inplace::plan_in_place;
use super::pointers::Relocation;
use crate::adapter::ApplyReport;
use crate::error::{CoreError, Result};
use crate::lz77;
use crate::scan::{is_sentence, scan_bytes, ScanConfig};
use crate::types::{Pointer, TextEntry};

#[derive(Default)]
pub(crate) struct PackedText {
    pub entries: Vec<TextEntry>,
    /// (posicao, bytes ocupados) de TODO bloco valido, com texto ou nao, em
    /// ordem de posicao.
    pub blocks: Vec<(usize, usize)>,
    /// A varredura parou no teto de trabalho (`lz77::SCAN_BUDGET`).
    pub truncated: bool,
}

/// O bloco guarda texto, e nao grafico que por acaso tem byte imprimivel?
/// Pelo menos uma frase (12+ caracteres, com espaco) e um terco do bloco
/// descomprimido em strings com cara de texto.
// ponytail: calibrada em 13/10/2026 num jogo real de GBA de 16 MiB cujo texto
// NAO e comprimido (1.599 blocos, so contagens): exigindo so duas frases, um
// bloco de tiles de 13 KiB passava com 15 "frases" de letra solta, 2,5% do
// bloco; com o terco, nenhum passa. Bloco que mistura dado binario com pouco
// texto fica de fora — baixar o corte quando houver um jogo real com texto
// comprimido pra medir.
fn is_text_block(texts: &[TextEntry], unpacked_len: usize) -> bool {
    let text_bytes: usize = texts.iter().map(|e| e.original_bytes.len()).sum();
    texts.iter().any(|e| is_sentence(&e.source_text)) && text_bytes * 3 >= unpacked_len
}

/// Acha os blocos LZ77 de `data` e extrai as frases dos que guardam texto.
/// Cada entry leva `offset` = posicao DENTRO do bloco descomprimido,
/// `resource_path` = "lz77:<posicao do bloco>" e o bloco em `metadata.lz77`.
pub(crate) fn extract(data: &[u8]) -> PackedText {
    let mut entries = Vec::new();
    let mut blocks = Vec::new();
    let mut budget = lz77::SCAN_BUDGET;
    let truncated = lz77::each_block(data, &mut budget, |at, block| {
        blocks.push((at, block.packed_len));
        let config = ScanConfig {
            only_text: true,
            max_entries: usize::MAX,
            ..ScanConfig::default()
        };
        // Scan ASCII sem regiao nem tabela nao tem como falhar.
        let Ok(found) = scan_bytes(&block.data, &config) else {
            return;
        };
        if !is_text_block(&found.entries, block.data.len()) {
            return;
        }
        for mut e in found.entries {
            let inner = e.offset.unwrap_or(0);
            e.id = format!("lz-{at:08x}-{inner:06x}");
            e.resource_path = Some(format!("lz77:{at:08x}"));
            e.max_bytes = Some(e.original_bytes.len());
            e.context = Some(
                "comprimido (LZ77): traducao limitada ao espaco original dentro do bloco"
                    .to_string(),
            );
            e.metadata["lz77"] = serde_json::json!({
                "block": at,
                "packedLen": block.packed_len,
            });
            entries.push(e);
        }
    });
    PackedText {
        entries,
        blocks,
        truncated,
    }
}

/// Tira de `entries` a string crua (do scanner do arquivo inteiro) que cai
/// dentro de um bloco comprimido: sao os literais do stream, e traducao
/// gravada ali corrompe o bloco. String com ponteiro em tabela fica — ponteiro
/// e evidencia mais forte que um bloco que pode ser coincidencia. Devolve
/// quantas sairam. `blocks` em ordem de posicao.
pub(crate) fn drop_overlapping(entries: &mut Vec<TextEntry>, blocks: &[(usize, usize)]) -> usize {
    let before = entries.len();
    entries.retain(|e| {
        let Some(start) = e.offset.map(|o| o as usize) else {
            return true;
        };
        !e.pointers().is_empty()
            || !overlaps(blocks, start, e.original_bytes.len())
            || e.resource_path.is_some()
    });
    before - entries.len()
}

fn overlaps(blocks: &[(usize, usize)], start: usize, len: usize) -> bool {
    // Ultimo bloco que comeca antes do fim do trecho.
    let i = blocks.partition_point(|&(at, _)| at < start + len);
    i > 0 && blocks[i - 1].0 + blocks[i - 1].1 > start
}

/// A entry veio de dentro de um bloco comprimido.
pub(crate) fn is_packed(entry: &TextEntry) -> bool {
    entry.metadata.get("lz77").is_some_and(|v| v.is_object())
}

/// Guarda na entry os ponteiros (u32, em tabela) que apontam pro BLOCO dela.
pub(crate) fn set_block_pointers(entry: &mut TextEntry, pointers: &[Pointer]) {
    let at: Vec<usize> = pointers.iter().map(|p| p.at).collect();
    entry.metadata["lz77"]["pointers"] = serde_json::json!(at);
    entry.context = Some(format!(
        "comprimido (LZ77): traducao limitada ao espaco original dentro do bloco; o bloco tem \
         {} ponteiro(s) em tabela e muda de lugar se crescer",
        pointers.len()
    ));
}

struct BlockRef {
    offset: usize,
    packed_len: usize,
    pointers: Vec<Pointer>,
}

fn block_ref(entry: &TextEntry) -> Option<BlockRef> {
    let meta = entry.metadata.get("lz77")?;
    let number = |key: &str| meta.get(key)?.as_u64().map(|n| n as usize);
    Some(BlockRef {
        offset: number("block")?,
        packed_len: number("packedLen")?,
        pointers: meta
            .get("pointers")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_u64())
            .map(|at| Pointer {
                at: at as usize,
                width: 4,
            })
            .collect(),
    })
}

pub(crate) struct PackedPlan {
    /// (posicao do bloco, stream novo com a sobra zerada ate o tamanho do
    /// stream original).
    pub writes: Vec<(usize, Vec<u8>)>,
    /// Blocos que cresceram e tem ponteiro em tabela: `original_offset` e a
    /// posicao do bloco, `bytes` o stream novo.
    pub relocations: Vec<Relocation>,
    /// (posicao, bytes ocupados) de todo bloco citado pelas entries, regravado
    /// ou nao. Nenhuma outra escrita pode cair ai.
    pub blocks: Vec<(usize, usize)>,
    pub report: ApplyReport,
}

impl PackedPlan {
    pub fn covers(&self, start: usize, len: usize) -> bool {
        overlaps(&self.blocks, start, len)
    }
}

/// Monta a regravacao dos blocos comprimidos que tem traducao. `data` e a
/// imagem ORIGINAL; entry que nao veio de bloco e ignorada.
pub(crate) fn plan(data: &[u8], entries: &[TextEntry]) -> Result<PackedPlan> {
    let mut groups: BTreeMap<usize, (BlockRef, Vec<TextEntry>)> = BTreeMap::new();
    for entry in entries {
        let Some(block) = block_ref(entry) else {
            continue;
        };
        groups
            .entry(block.offset)
            .or_insert_with(|| (block, Vec::new()))
            .1
            .push(entry.clone());
    }

    let mut plan = PackedPlan {
        writes: Vec::new(),
        relocations: Vec::new(),
        blocks: Vec::new(),
        report: ApplyReport::default(),
    };
    for (offset, (block, group)) in groups {
        plan.blocks.push((offset, block.packed_len));
        if group.iter().all(|e| e.translated_text.is_none()) {
            plan.report.kept_original += group.len();
            continue;
        }
        let original = data
            .get(offset..)
            .and_then(lz77::unpack)
            .filter(|u| u.packed_len == block.packed_len)
            .ok_or_else(|| {
                CoreError::Project(format!(
                    "o bloco comprimido em 0x{offset:X} nao e mais o que foi extraido — arquivo \
                     diferente? Re-extraia antes de reinserir"
                ))
            })?;
        let inner = plan_in_place(&original.data, &group, false)?;
        let mut unpacked = original.data;
        for (at, patch) in &inner.writes {
            unpacked[*at..at + patch.len()].copy_from_slice(patch);
        }
        // Rele o que acabou de comprimir: o tamanho exato do stream sai dai,
        // e um compressor errado nunca chega no arquivo.
        let packed = lz77::pack(&unpacked);
        let checked = packed.as_deref().and_then(lz77::unpack);
        let (Some(mut packed), Some(checked)) = (packed, checked) else {
            return Err(self_check_failed(offset));
        };
        if checked.data != unpacked {
            return Err(self_check_failed(offset));
        }

        if checked.packed_len <= block.packed_len {
            packed.truncate(checked.packed_len);
            packed.resize(block.packed_len, 0);
            plan.writes.push((offset, packed));
        } else if !block.pointers.is_empty() {
            plan.relocations.push(Relocation {
                entry_id: format!("bloco comprimido em 0x{offset:X}"),
                original_offset: offset,
                bytes: packed,
                pointers: block.pointers,
            });
            plan.report.relocated += inner.report.applied;
        } else {
            plan.report.packed_left_out += inner.report.applied;
            plan.report.kept_original += inner.report.kept_original;
            continue;
        }
        plan.report.applied += inner.report.applied;
        plan.report.kept_original += inner.report.kept_original;
    }
    Ok(plan)
}

fn self_check_failed(offset: usize) -> CoreError {
    CoreError::Project(format!(
        "auto-checagem da recompressao falhou no bloco em 0x{offset:X} (bug) — nada foi gravado"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_exact_at_the_block_edges() {
        let blocks = [(0x100, 0x20), (0x200, 0x10)];
        assert!(!overlaps(&blocks, 0xF0, 0x10));
        assert!(overlaps(&blocks, 0xF0, 0x11));
        assert!(overlaps(&blocks, 0x11F, 4));
        assert!(!overlaps(&blocks, 0x120, 0x40));
        assert!(overlaps(&blocks, 0x1F0, 0x40));
        assert!(!overlaps(&blocks, 0x210, 8));
        assert!(!overlaps(&[], 0, 8));
    }
}
