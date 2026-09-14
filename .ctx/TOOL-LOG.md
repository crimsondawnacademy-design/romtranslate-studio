# TOOL-LOG.md — RomTranslate Studio

> Rastro de auditoria anti-alucinação. Uma linha por sessão.
> Lido por qualquer ferramenta pra saber quem mexeu e quando.
> Não apague entradas antigas — é o histórico completo.

| data | maquina | ferramenta | modelo | o_que_fez | arquivos_tocados |
|---|---|---|---|---|---|
| 2026-09-14 | Mac mini M1 (casa) | claude | claude-fable-5 | Sprint 2: scanners ASCII/UTF/tabela .tbl, export JSON/CSV, tela de extração + abrir projeto, +11 testes | `crates/core/src/{scan,tbl,export}.rs`, `apps/desktop/src/App.tsx`, `apps/desktop/src-tauri/src/lib.rs` |
| 2026-09-14 | Mac mini M1 (casa) | claude | claude-fable-5 | Sprints 0+1 do zero: workspace, core (detecção GBA/NES/SNES + hash + projeto), app Tauri, UI i18n, 20 testes, CI, docs | `Cargo.toml`, `crates/core/**`, `apps/desktop/**`, `.github/workflows/ci.yml`, `README.md`, `docs/SPEC.md` |

<!-- Adicione nova linha no topo (logo abaixo do header da tabela). Ordem: mais recente primeiro. -->
