---
projeto: romtranslate-studio
status: em-andamento
ultima_edicao: 2026-09-14
ultima_ferramenta: claude
ultima_modelo: claude-fable-5
ultima_maquina: Mac mini M1 (casa)
branch: main
---

# STATE.md — RomTranslate Studio

> Arquivo leve (~50 linhas), sempre atualizado no fim de cada sessão/feature.
> Lido por Claude Code, Codex e ZCode antes de qualquer trabalho.
> Política CCP — não delete seções, só atualize.

## TL;DR
App desktop open source (Tauri 2 + React + Rust) pra tradução de ROMs com IA, local-first, patch-first. Spec em `docs/SPEC.md`. Sprints 0–5 PRONTOS: detecção, projeto `.rtsproj`, extração, tradução (validada com Ollama real), editor+validação, e agora REINSERÇÃO: adapter RTSF (Camada B completa — fixed slots, relocáveis com pointer table, checksum), trait com extract_structured/apply_text/verify, working copy verificada, bloqueio de erros com modo avançado. Round-trip provado por teste. 58 testes. Próximo: Sprint 6 (patch export IPS + manifest).

## Feito nesta sessão
- [2026-09-14] claude/fable-5 — Sprint 5: formato sintético RTSF + adapter `synthetic.rtsf` (probe 0.99, extract estruturado com max_bytes real, apply all-or-nothing com relocação de blob + update de pointer table + checksum recalculado, verify), trait GameAdapter ganhou extract_structured/apply_text/verify (default "não suportado"), `reinsert::reinsert_project` (§15: confere SHA-256 da origem, valida, bloqueia Error salvo allow_errors, grava working/ atômico, relê e verifica), commands extract_structured/reinsert_project, UI: botão de extração estruturada + painel de reinserção. 8 testes novos incl. round-trip completo e headers mentirosos. Antes: Sprint 4: módulo `validate` (tokens {..}/<..>/[..]/%x/\x em multiset, encoded_len por encoding, overflow vs max_bytes ou espaço original, vazias, anomalia 3x), `validate_project_db` ajusta statuses (Error↔Machine, Reviewed preservado), `apply_manual_translation` (edição grava TM + valida), `set_reviewed` exige validação limpa; commands update_entry/set_entry_reviewed/validate_project; UI: filtros por status+issue, painel editor com bytes live e issues, validação automática pós-tradução. Antes na mesma sessão: Sprints 0+1+2+3 (ver TOOL-LOG): trait `TranslationProvider` (async_trait) + providers Ollama (/api/chat, format=json) e OpenAI-compatible (/chat/completions), prompt spec §12 com glossário relevante por batch, parser tolerante (objeto/array/fences/<think>), `ProjectDb` SQLite (entries/tm/glossary/provider_runs, upsert preserva traduções em re-scan), pipeline com TM-first + batches sequenciais + retry backoff + cancel + progresso, settings.toml + secrets.json 0600 com guarda allow_remote_translation, 6 commands Tauri novos + evento translation-progress, UI: painel de tradução (provider/modelo/teste/progresso/cancelar), coluna tradução + status na tabela, glossário CRUD. Teste real com Ollama local passou (3/3, glossário respeitado).

## Próximos passos
1. [P1] Sprint 6 — patch export: abstração de patching, IPS funcional (BPS depois), manifest.json com sha256/locale/adapter/contagens, UI de export (spec §16, §25). DoD: patch aplicado à fixture original produz exatamente a working copy.
2. [P2] Checkbox "modo avançado" na UI (core já suporta allow_errors; UI passa false).
3. [P2] Batches concorrentes; TM global; keychain; CI build Tauri por SO; screenshot no README.

## Bloqueios / Pendências
- [ ] Nenhum bloqueio. Repo PRIVADO na conta crimsondawnacademy-design; tornar público quando Rhuan decidir.

## Caminhos críticos
- Providers: `crates/core/src/provider.rs` (trait+prompt+parse) + `crates/core/src/providers/{ollama,openai_compat}.rs`
- Pipeline: `crates/core/src/pipeline.rs` (run_translation: TM→batches→retry→cancel)
- DB do projeto: `crates/core/src/db.rs` (`<proj>.rtsproj/translations.sqlite`)
- Settings do app: `apps/desktop/src-tauri/src/settings.rs` (settings.toml + secrets.json no app_config_dir)
- Commands: `apps/desktop/src-tauri/src/lib.rs` · UI do projeto: `apps/desktop/src/ProjectView.tsx`
- Teste real Ollama: `cargo test -p romtranslate-core --test translate -- --ignored --nocapture`
- Validação: ver bloco em `CLAUDE.md` do repo

## Glossário do projeto
- **provider**: implementação de `TranslationProvider` (async); Ollama local ou endpoint OpenAI-compatible; modelo NUNCA hardcoded
- **TM**: translation memory por projeto; chave = texto normalizado (trim+collapse, case preservado) + par de idiomas
- **glossário**: termos por projeto injetados no prompt SÓ quando aparecem no batch; `no_translate` = manter original
- **allow_remote_translation**: guarda de privacidade — endpoint fora de localhost exige opt-in explícito
- **scanner (Camada A)**: descoberta genérica em `scan.rs`; NÃO garante reinserção segura (Camada B, Sprint 5)
- **.rtsproj**: diretório do usuário (project.json + translations.sqlite + subdirs); NUNCA contém cópia da ROM
- **synth**: fixtures sintéticas — proibido byte de jogo real no repo
