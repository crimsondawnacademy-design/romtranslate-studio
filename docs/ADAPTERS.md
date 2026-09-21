# Criando um adapter de plataforma

Adapters são o coração do RomTranslate Studio: cada um sabe detectar um formato,
extrair texto com limites reais e (quando declarado) reinserir traduções. Este
guia acompanha o contrato em [`crates/core/src/adapter.rs`](../crates/core/src/adapter.rs);
o adapter de referência com o ciclo completo é
[`crates/core/src/adapters/rtsf.rs`](../crates/core/src/adapters/rtsf.rs), e
[`gba.rs`](../crates/core/src/adapters/gba.rs) mostra o padrão "reinserção
conservadora" para uma plataforma real.

> **Escopo atual**: adapters são compilados no workspace e registrados em
> `adapters::all()`. Plugin loading dinâmico (SDK externo) é fase futura — a
> interface pública deste documento é o que virará o SDK.

## O contrato

```rust
pub trait GameAdapter: Send + Sync {
    fn id(&self) -> &'static str;            // ex.: "gba.generic", "snes.meujogo"
    fn display_name(&self) -> &'static str;
    fn platform(&self) -> Platform;
    fn capabilities(&self) -> AdapterCapabilities;

    fn probe(&self, input: &GameInput) -> ProbeResult;

    // Camada B — implemente SOMENTE o que a capability declara:
    fn extract_structured(&self, data: &[u8]) -> Result<Vec<TextEntry>> { /* default: não suportado */ }
    fn apply_text(&self, data: &[u8], entries: &[TextEntry]) -> Result<AppliedImage> { /* idem */ }
    fn verify(&self, data: &[u8]) -> Result<VerificationReport> { /* idem */ }
}
```

## Regras inegociáveis

1. **Nunca panique com input malformado.** Todo acesso a bytes passa por bounds
   check (`get(..)`, aritmética `checked_*`). Header truncado, offset mentiroso,
   contagem absurda → `Err` com mensagem clara, jamais `panic!`/index direto.
   O teste `malformed_headers_error_cleanly_never_panic` (tests/reinsert.rs) é o
   modelo: rode seu parser contra o arquivo truncado em TODOS os tamanhos.
2. **Capabilities honestas.** Não declare `reinsert: true` sem teste de
   round-trip. `support_level` aparece na UI — `Experimental` é um nível digno;
   prometer demais é bug (spec §26: nada de "qualquer ROM").
3. **Nenhum byte de jogo real no repo.** Fixtures são sintéticas, geradas em
   [`synth.rs`](../crates/core/src/synth.rs) — headers válidos construídos do
   zero. Isso inclui logos/bitmaps proprietários (o probe de GBA detecta SEM o
   logo Nintendo de propósito).
4. **O arquivo original do usuário é intocável.** `apply_text` recebe bytes e
   devolve bytes novos; quem grava é a orquestração (`reinsert.rs`), sempre em
   working copy.

## probe — detecção com evidências

- Leia só de `input.head` (primeiros 128 KiB) com bounds check.
- `confidence` 0.0–1.0; `evidence` legível por humanos explicando cada ponto
  ("magic presente", "checksum válido", "tamanho bate com o header").
- Retorne `ProbeResult::no_match(...)` quando não reconhecer — nunca erro.
- Não use extensão de arquivo como prova; no máximo como desempate.

## extract_structured — Camada B

Cada `TextEntry` precisa carregar o suficiente para reinserção segura:

| campo | regra |
|---|---|
| `id` | determinístico e estável entre execuções (ex.: `fixed-3`, `scan-0000a1c0`) |
| `offset` | posição real no arquivo |
| `original_bytes` | os bytes exatos que a string ocupa hoje (sanity check no apply) |
| `max_bytes` | limite REAL de reinserção (`None` só se houver relocação de verdade) |
| `encoding` | o encoding de destino — o validador calcula bytes com ele |
| `metadata` | o que o SEU apply precisa (`kind`, índice de ponteiro, flags) |

Se a plataforma tem tabela de ponteiros, modele-a explicitamente (veja o blob +
ponteiros do RTSF). **Nunca** faça busca/substituição global de bytes.

## apply_text — all-or-nothing

- Qualquer tradução que não serializa (não cabe, encoding errado, ponteiro
  inexistente) → `Err` e NADA é aplicado. Imagem parcial não existe.
- Confira que `data[offset..]` ainda bate com `original_bytes` (anti-drift:
  o arquivo pode não ser o mesmo de quando extraiu).
- Atualize o que o formato exigir: ponteiros, terminadores, **checksums**
  (o GBA recalcula o header checksum; o RTSF, o checksum do arquivo).
- Tradução maior que o espaço original: só realoque se souber ONDE estão os
  ponteiros. `adapters::pointers` resolve ponteiros absolutos de 32 bits LE
  (reconhece **tabelas** — 2+ ponteiros seguidos pra inícios de string —,
  nunca busca/troca global de bytes, que a spec proíbe; e `relocate` faz
  anti-drift + auto-checagem). O GBA é o exemplo: marque as entries com
  `metadata.pointers`, chame `plan_in_place(.., true)` e depois `relocate`.
  Sem tabela conhecida → `Err` pedindo texto menor.
- Entries sem tradução mantêm o original (`kept_original`); entries de outro
  scanner que você não entende → `ignored_generic`, nunca erro silencioso.
- Erros orientam o usuário: diga QUAL entry, QUANTOS bytes, o que fazer
  ("encurte o texto").

## verify — o que foi gravado é o que vale

Re-parseie a imagem final: estrutura íntegra, checksums batendo, re-extração
funciona. `problems` não-vazio bloqueia o fluxo. A orquestração chama `verify`
sobre os bytes RELIDOS do disco.

## Fixtures e testes obrigatórios

Gerador em `synth.rs` + testes de integração (`tests/`):

- **probe**: fixture detectada com a confiança esperada; bytes aleatórios,
  vazio e truncado → confidence 0.0, sem panic;
- **extract**: offsets/limites/metadata exatos;
- **round-trip** (o teste que importa): extrair → traduzir → `apply_text` →
  `verify` ok → re-extrair devolve exatamente o que foi aplicado, com as
  não-traduzidas intactas;
- **negativos**: overflow de campo/capacidade, encoding inválido e arquivo
  divergente retornam `Err` sem escrita parcial;
- **headers mentirosos**: offsets/contagens absurdos erram limpo.

## Registrando

1. Módulo novo em `crates/core/src/adapters/`.
2. Adicione em `adapters::all()` (ordem: mais específico primeiro).
3. `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test --workspace`.
4. Atualize a matriz de compatibilidade no README.

## Esqueleto mínimo

```rust
use crate::adapter::{GameAdapter, GameInput};
use crate::types::{AdapterCapabilities, Platform, ProbeResult};

pub struct MeuAdapter;

impl GameAdapter for MeuAdapter {
    fn id(&self) -> &'static str { "plataforma.variante" }
    fn display_name(&self) -> &'static str { "Minha Plataforma" }
    fn platform(&self) -> Platform { Platform::Nes }
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::detect_only() // suba capabilities SÓ com testes
    }
    fn probe(&self, input: &GameInput) -> ProbeResult {
        let head = &input.head;
        if head.len() < 16 || &head[0..4] != b"MAGI" {
            return ProbeResult::no_match(self.id(), self.platform());
        }
        ProbeResult {
            confidence: 0.9,
            evidence: vec!["magic MAGI presente".into()],
            ..ProbeResult::no_match(self.id(), self.platform())
        }
    }
}
```
