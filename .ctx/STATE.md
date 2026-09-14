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
App desktop open source (Tauri 2 + React + Rust) pra tradução de ROMs com IA, local-first, patch-first. Spec em `docs/SPEC.md`. Sprints 0–4 PRONTOS: detecção GBA/NES/SNES, projeto `.rtsproj`, extração, tradução (Ollama/OpenAI-compat + TM + glossário, validada com Ollama real) e agora EDITOR+VALIDAÇÃO: placeholders/tags/bytes/anomalias, statuses com revisão manual bloqueada por erro, edição inline que alimenta a TM, filtros. 50 testes (42 Rust + 8 vitest). Próximo: Sprint 5 (fixture adapter de reinserção).

## Feito nesta sessão
- [2026-09-14] claude/fable-5 — Sprint 4: módulo `validate` (tokens {..}/<..>/[..]/%x/\x em multiset, encoded_len por encoding, overflow vs max_bytes ou espaço original, vazias, anomalia 3x), `validate_project_db` ajusta statuses (Error↔Machine, Reviewed preservado), `apply_manual_translation` (edição grava TM + valida), `set_reviewed` exige validação limpa; commands update_entry/set_entry_reviewed/validate_project; UI: filtros por status+issue, painel editor com bytes live e issues, validação automática pós-tradução. Antes na mesma sessão: Sprints 0+1+2+3 (ver TOOL-LOG): trait `TranslationProvider` (async_trait) + providers Ollama (/api/chat, format=json) e OpenAI-compatible (/chat/completions), prompt spec §12 com glossário relevante por batch, parser tolerante (objeto/array/fences/<think>), `ProjectDb` SQLite (entries/tm/glossary/provider_runs, upsert preserva traduções em re-scan), pipeline com TM-first + batches sequenciais + retry backoff + cancel + progresso, settings.toml + secrets.json 0600 com guarda allow_remote_translation, 6 commands Tauri novos + evento translation-progress, UI: painel de tradução (provider/modelo/teste/progresso/cancelar), coluna tradução + status na tabela, glossário CRUD. Teste real com Ollama local passou (3/3, glossário respeitado).

## Próximos passos
1. [P1] Sprint 5 — adapter sintético de reinserção: fixed strings, relocatable, pointer table update, checksum, round-trip provado por teste (spec §15, §25). A UI deve BLOQUEAR reinserção de entries com status Error (validador já marca).
2. [P2] Sprint 6 — patch export (IPS primeiro) + manifest.
3. [P2] Batches concorrentes; TM global cross-projeto; API key em keychain; CI build Tauri por SO; screenshot no README.

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
