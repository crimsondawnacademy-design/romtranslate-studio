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
| Workspace Cargo+pnpm | pronto | raiz | — |
| Detecção GBA/NES/SNES | pronto (Experimental) | `crates/core/src/adapters/` | — |
| Projeto `.rtsproj` | pronto (criar/abrir/verificar hash) | `crates/core/src/project.rs` | — |
| Extração (Camada A) | pronto (ascii/utf8/utf16/.tbl) | `crates/core/src/scan.rs` | Shift-JIS qdo houver caso |
| Persistência entries | pronto (SQLite, preserva traduções) | `crates/core/src/db.rs` | — |
| Tradução Ollama/OpenAI-compat | pronto, validado com Ollama REAL | `crates/core/src/{provider,providers,pipeline}.rs` | — |
| TM + glossário | pronto (por projeto) | `crates/core/src/db.rs` | TM global depois |
| Settings + secrets | pronto (toml + secrets 0600) | `apps/desktop/src-tauri/src/settings.rs` | keyring qdo distribuir |
| UI tradução | pronto (config/progresso/cancel/glossário) | `apps/desktop/src/ProjectView.tsx` | — |
| Editor + validação | pronto (tokens/bytes/statuses/filtros) | `crates/core/src/validate.rs` + ProjectView | — |
| Reinserção (RTSF + GBA conservador) | pronto (round-trip testado) | `crates/core/src/{adapters/{rtsf,gba},reinsert}.rs` | NES/SNES no mesmo padrão |
| Patch export IPS + manifest | pronto (DoD testado) | `crates/core/src/patch.rs` | BPS p/ >16 MiB/truncate |
| Comunidade (Sprint 7) | pronto | `docs/ADAPTERS.md`, README matrix, `.github/` templates | repo público qdo Rhuan decidir |
| NDS (Sprint 8) | pronto (Experimental: FS + in-place) | `crates/core/src/adapters/nds.rs` | BPS p/ ROM real >16 MiB |
| GC/Wii probes | pronto (detect-only; Wii cifrado) | `adapters/{gamecube,wii}.rs` | FST do GC; Wii U após verificar formato |

## 1. Arquitetura técnica
- **Patch (Sprint 6)**: `patch::create_ips/apply_ips` (Rust puro; apply lê RLE + truncate extension; create desvia do offset 0x454F46 e funde gaps <6B; limite 16 MiB com erro claro). `patch::export_patch(dir)` exige working copy, roda round-trip interno (apply==working senão aborta) e grava `exports/<stem>.<lang>.{ips,manifest.json,translations.csv}`; manifest snake_case (artefato público, spec §16).
- **GBA conservador**: `extract_structured` = scan ASCII com max_bytes = espaço do run (ids "scan-<off>" iguais ao scanner → upsert funde). `apply_text` in-place: sanity bytes==original_bytes (anti-drift), tradução ≤ espaço, padding 0x00 (run terminated) ou 0x20, header checksum SEMPRE recalculado. Sem relocação — overflow orienta encurtar.
- **Reinserção (Sprint 5)**: trait `GameAdapter` agora tem `extract_structured`/`apply_text`/`verify` (default "não suportado"; capabilities dizem quem implementa). Adapter `synthetic.rtsf` (formato próprio RTSF, `synth::make_rtsf_fixture`): slots fixos ASCII (max_bytes = slot-1), relocáveis em blob com tabela de ponteiros u32 reescrita no apply, checksum u32 (soma wrapping, campo zerado) recalculado. `reinsert::reinsert_project(dir, allow_errors)` = §15 completo: SHA-256 da origem confere → validação fresca (Error bloqueia salvo allow_errors) → apply all-or-nothing em memória → grava `working/<nome>` atômico → RELÊ do disco → verify (falhou = Err, arquivo fica pra inspeção). Original nunca é tocado (teste garante byte a byte).
- **Validação (Sprint 4)**: `validate::validate_entry` — tokens {..}/<..>/[..]/%x/\x comparados em multiset (remoção/invenção = Error), `encoded_len` por encoding (ASCII não-codificável = Error Unencodable; tabela/Shift-JIS = check pulado), overflow: Error acima de `max_bytes`, Warning acima do espaço original, vazia = Error, anomalia >3x = Warning. `validate_project_db` ajusta statuses (Error↔Machine; Reviewed limpo fica). `apply_manual_translation` = editar → status Machine + TM + valida. `set_reviewed(true)` recusa com Error pendente. UI valida automático pós-tradução; filtros derivam de status + issues.
- **Fluxo de tradução**: `pipeline::run_translation(db, provider, model, opts, cancel, on_progress)` — (1) carrega entries sem tradução; (2) fase TM: lookup por texto normalizado (trim+collapse, case preservado) + par de idiomas, aplica direto; (3) restante em batches sequenciais (`batch_size`, default 10): glossário filtrado ao batch → prompt (§12) → provider → retry com backoff exponencial (`max_attempts` 3, `retry_delay_ms`) → grava entry (status Machine) + TM; (4) `record_run` em provider_runs. Cancel = AtomicBool checado entre batches. Progresso = callback (Tauri emite `translation-progress`).
- **Providers**: trait `TranslationProvider` (async_trait) com `translate_batch`/`health_check`. Ollama usa `/api/chat` com `format:"json"`; OpenAI-compat usa `/chat/completions` com bearer opcional. `parse_model_response` tolera objeto/array/```fences/<think> e ids faltantes (contam como failed). Modelo vem SEMPRE de config.
- **DB**: `translations.sqlite` no `.rtsproj` (WAL). Upsert de entries preserva `translated_text`/`status` quando o re-scan vem sem tradução. Enums gravados como JSON string.
- **Settings do app**: `settings.toml` (camelCase serde, legível) + `secrets.json` 0600 no `app_config_dir`; `save_settings(newSettings, apiKey)` — apiKey None mantém, "" remove. Guarda: provider openai_compatible fora de localhost exige `allowRemoteTranslation`.
- **Commands**: inspect_file, create_project, open_project, scan_file, export_entries, save_entries, load_entries, glossary_*, get/save_settings, test_provider, translate_project (State TranslationState impede 2 simultâneas), cancel_translation.
- **UI**: `ProjectView.tsx` = tela do projeto (scan auto-salva no DB → tabela com tradução+status → painel de tradução → glossário em details). `App.tsx` = wizard.

## 2. Estrutura de arquivos
```
crates/core/src/       adapter.rs · adapters/ · detect.rs · scan.rs · tbl.rs · export.rs
                       provider.rs (trait+prompt+parse) · providers/{ollama,openai_compat}.rs
                       pipeline.rs (run_translation) · db.rs (ProjectDb) · project.rs
                       hash.rs · synth.rs · types.rs · error.rs
crates/core/tests/     pipeline.rs · scan.rs · translate.rs (mock + #[ignore] Ollama real)
apps/desktop/src-tauri/ lib.rs (commands+state) · settings.rs · tauri.conf.json
apps/desktop/src/      App.tsx (wizard) · ProjectView.tsx (tela do projeto) · i18n.ts · types.ts · util.ts
docs/SPEC.md           spec master — LER antes de sprint novo
```

## 3. Como validar
```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
pnpm lint && pnpm test && pnpm web:build
# prova de fogo com Ollama local (fora do CI):
cargo test -p romtranslate-core --test translate -- --ignored --nocapture
pnpm dev   # manual: synthetic.gba de fixtures/generated → criar projeto → escanear → traduzir
```

## 4. Armadilhas conhecidas
- Parsers/scanners nunca panicam; bounds check sempre (testes *_never_panic*).
- `TextEntry.original_bytes` cruza a ponte como HEX string.
- API key NUNCA em settings.toml, logs, .rtsproj ou repo — só secrets.json 0600.
- Ollama: modelo tem que estar puxado (`ollama pull llama3.2:3b`); o campo modelo vazio dá erro claro.
- llama3.2:3b traduz com escorregões de gramática ("À VILAREJO") — revisão é Sprint 4; pipeline está correto (placeholders/glossário preservados).
- Máquina do trabalho provavelmente sem Rust/pnpm (rustup + `npm i -g pnpm`).
- PROJETO PESSOAL: nada vai pra infra de trabalho nem pro assistente interno (DECISIONS.md).
