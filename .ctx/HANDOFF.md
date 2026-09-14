---
projeto: romtranslate-studio
handoff_gerado: 2026-09-14
gerado_por: claude
motivo: fim-de-feature
---

# HANDOFF.md — RomTranslate Studio

## 0. TL;DR
| Entregável | Estado | Path/URL | Próximo passo |
|---|---|---|---|
| Workspace Cargo+pnpm | pronto | raiz do repo | — |
| Detecção GBA/NES/SNES | pronto (Experimental, detect-only) | `crates/core/src/adapters/` | mais evidências conforme casos reais |
| Projeto `.rtsproj` | pronto (criar/abrir c/ verificação de hash) | `crates/core/src/project.rs` | — |
| Extração de strings (Camada A) | pronto (ascii/utf8/utf16le-be/.tbl) | `crates/core/src/scan.rs` | Shift-JIS quando houver caso real |
| Export JSON/CSV | pronto | `crates/core/src/export.rs` | persistir no projeto automaticamente |
| App desktop | pronto (wizard + extração) | `apps/desktop/` | Sprint 3: UI de tradução |
| CI | pronto (verde) | `.github/workflows/ci.yml` | job de build Tauri por SO |
| Sprint 3 (tradução) | não começou | spec §11-13 | provider trait + Ollama + TM/glossário |

## 1. Arquitetura técnica
- `romtranslate-core` (Rust, SEM Tauri):
  - `detect::inspect(path)` → `InspectionReport { size, sha256, results, best }`; probes via `adapters::all()`, best = confiança ≥ 0.5.
  - `scan::scan_file(path, &ScanConfig)` → `ScanOutcome { entries: Vec<TextEntry>, truncated, scanned_bytes }`. Config: encoding (ascii/utf8/utf16_le/utf16_be/table), tbl_path, min_chars (CARACTERES), region_start/end, max_entries (default 20k). Determinístico. Limite de arquivo do scan: 64 MiB (`MAX_SCAN_FILE_SIZE`). Filtro unicode `is_text_char` exclui controle, private-use, replacement e noncharacters (padding 0xFFFF!).
  - `tbl::TblTable` — `HEX=texto` (chaves 1-4 bytes), longest-match greedy, erros com número de linha.
  - `export::{export_json, export_csv}` — CSV RFC4180; `original_bytes` serializa como HEX string (serde custom em types.rs).
  - `project::{create_project, open_project}` — open_project reconfere SHA-256 e reporta `source_found`/`source_changed`.
- Shell Tauri: 5 commands (`inspect_file`, `create_project`, `open_project`, `scan_file`, `export_entries`) todos via helper `blocking()` (spawn_blocking). serde `rename_all=camelCase` espelhado em `apps/desktop/src/types.ts`.
- UI: React, tela única com 5 estados (home → inspecting → report → created → extract). "Abrir projeto" na home vai direto pro extract com warnings de origem. Strings SÓ via `t()` de `i18n.ts` (pt-BR/en-US, interpolação `{param}`, teste de paridade). Tabela de resultados: filtro client-side + cap de render 500.

## 2. Estrutura de arquivos
```
crates/core/src/       adapter.rs (trait+GameInput) · adapters/{gba,nes,snes}.rs
                       detect.rs · scan.rs · tbl.rs · export.rs · hash.rs
                       project.rs · synth.rs · types.rs · error.rs
crates/core/tests/     pipeline.rs (detecção+projeto) · scan.rs (scanners+export)
crates/core/examples/  gen_fixtures.rs → fixtures/generated/ (gitignored)
apps/desktop/src/      App.tsx · i18n.ts · types.ts · util.ts · *.test.ts
apps/desktop/src-tauri/ lib.rs (commands) · tauri.conf.json · capabilities/
docs/SPEC.md           spec master completa — LER antes de sprint novo
```

## 3. Como validar
```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
pnpm lint && pnpm test && pnpm web:build
pnpm dev   # teste manual: fixtures/generated/synthetic.gba tem strings plantadas
           # (WELCOME TO THE VILLAGE!, POTION, HP {0}: 120, SYNTH QUEST em utf16le)
```

## 4. Armadilhas conhecidas
- Probes e scanners NUNCA panicam: bounds check em todo acesso; testes `*_never_panic*` cobrem — mantenha o padrão.
- Não embutir bytes do logo Nintendo (nem de nenhum jogo) — ver DECISIONS.md.
- `TextEntry.original_bytes` cruza a ponte como string HEX, não array.
- Máquina de casa ganhou Rust/pnpm em 14/09; a do trabalho provavelmente NÃO tem (rustup + `npm i -g pnpm`).
- PROJETO PESSOAL: nada disso vai pra infra de trabalho nem pro assistente interno (decisão do Rhuan, DECISIONS.md).
