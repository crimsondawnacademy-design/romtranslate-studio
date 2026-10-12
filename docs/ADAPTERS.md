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
    fn extract(&self, data: &[u8]) -> Result<Extraction> { /* default: não suportado */ }
    // extract_structured(data) já vem pronto: são só as entries de extract.
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

## extract — Camada B

`extract` devolve um `Extraction`: as entries e, em número, o que ficou de
fora (`discarded`, `truncated`). Cada `TextEntry` precisa carregar o
suficiente para reinserção segura:

| campo | regra |
|---|---|
| `id` | determinístico e estável entre execuções (ex.: `fixed-3`, `scan-0000a1c0`) |
| `offset` | posição real no arquivo |
| `original_bytes` | os bytes exatos que a string ocupa hoje (sanity check no apply) |
| `max_bytes` | limite REAL de reinserção (`None` só se houver relocação de verdade) |
| `encoding` | o encoding de destino — o validador calcula bytes com ele |
| `metadata` | o que o SEU apply precisa (`kind`, índice de ponteiro, flags). A chave `encoded` é reservada (veja "Texto lido por tabela") |

Se a plataforma tem tabela de ponteiros, modele-a explicitamente (veja o blob +
ponteiros do RTSF). **Nunca** faça busca/substituição global de bytes.

**Varreu bytes atrás de texto? Descarte o que não parece texto, e conte.**
A maior parte das sequências imprimíveis de um jogo é gráfico, código e
tabela; entry de lixo na extração estruturada é tradução gravada por cima
disso. A regra é uma só (`scan::looks_like_text`) e o teto é um só
(`scan::MAX_ENTRIES`):

- sem ponteiros: passe `only_text: true` no `ScanConfig` — o scanner filtra
  antes do teto e devolve `discarded`;
- com ponteiros: varra, marque os ponteiros (`mark_relocatable`) e só então
  chame `scan::keep_text`. String com ponteiro em tabela nunca é descartada, e
  quem sabe disso é a entry já marcada;
- some os descartes em `Extraction::discarded` e ligue `truncated` se parou
  no teto. Nada sai em silêncio: a interface mostra os dois;
- não mude o `id`: projeto existente guarda a tradução por ele.

**Uma leitura por trecho de bytes.** Se você varre o mesmo trecho em dois
encodings (o NDS faz ASCII + UTF-16LE), as leituras se sobrepõem — texto
ASCII lido como UTF-16 vira "CJK" falso no mesmo offset, e o `id` é só o
offset. No banco as duas se fundem e a tradução de uma é gravada com o
encoding da outra. Resolva a sobreposição na extração (veja
`resolve_encoding_overlaps` no `nds.rs`).

## apply_text — all-or-nothing

- Qualquer tradução que não serializa (não cabe, encoding errado, ponteiro
  inexistente) → `Err` e NADA é aplicado. Imagem parcial não existe.
- Confira que `data[offset..]` ainda bate com `original_bytes` (anti-drift:
  o arquivo pode não ser o mesmo de quando extraiu).
- Atualize o que o formato exigir: ponteiros, terminadores, **checksums**
  (o GBA recalcula o header checksum; o RTSF, o checksum do arquivo).
- Tradução maior que o espaço original: só realoque se souber ONDE estão os
  ponteiros E onde o texto novo pode morar. `adapters::pointers` reconhece
  **tabelas** de ponteiros LE (nunca busca/troca global de bytes, que a spec
  proíbe), descritas por um `PointerFormat` (largura 4 ou 2, base, run
  mínimo, relativo ou não): `find_pointer_tables` com base absoluta (o GBA
  define o dele: u32, run 2) ou `file_relative_tables` pra offsets dentro de
  um arquivo (NDS, PS1: u32 com run 3 e u16 com run 4). Em offset relativo a
  tabela tem que ser **estritamente crescente** e alvo 0 não conta — número
  pequeno aparece à toa em dado binário, e sem isso padding de zeros vira
  "tabela" apontando pra string que abre o arquivo. `relocate` faz
  anti-drift, confere se o ponteiro de 16 bits alcança o destino, e
  auto-checagem. Fluxo: marque as entries com
  `mark_relocatable`, chame `plan_in_place(.., true)`,
  `ensure_pointers_untouched` e `relocate` no buffer certo (a imagem no GBA,
  o arquivo no NDS/PS1). Sem tabela → `Err` pedindo texto menor.
- **Nunca anexe texto a algo que o jogo COPIA pra RAM com tamanho
  declarado** (ARM9 do DS, EXE do PS1): logo depois vêm BSS e heap, e o texto
  é zerado no boot ou sobrescrito. Só é seguro onde o dado novo comprovadamente
  chega à memória: ROM mapeada (GBA), arquivo que o jogo acha pela FAT (NDS),
  sobra de setor lido inteiro (PS1).
- **Texto lido por tabela** (`TextEncoding::Table(id)`, vindo do scanner com
  `.tbl`): o adapter não recebe tabelas. A orquestração (`reinsert.rs`) lê
  `<projeto>/tables/<id>.tbl`, codifica a tradução com
  `TblTable::encode_slot` — já com a sobra preenchida — e entrega os bytes na
  própria entry (`metadata.encoded`, lido por `TextEntry::encoded()`). Quem
  usa `plan_in_place`/`apply_in_place` não faz nada: o helper grava esses
  bytes e devolve `Err` se a entry de tabela chegar sem eles (chamada por
  fora da orquestração). Adapter com escrita própria grava
  `entry.encoded()` em vez de codificar. Hoje só o NES tem teste de ida e
  volta com tabela (`tests/table_reinsert.rs`).
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
