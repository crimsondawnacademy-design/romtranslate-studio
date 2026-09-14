# RomTranslate Studio — instruções para agentes

Spec completa do produto e roadmap de sprints: `docs/SPEC.md`. Leia antes de
implementar qualquer sprint novo. Estado atual do projeto: `.ctx/STATE.md`.

## Regras que não se negocia

- `crates/core` NUNCA depende de Tauri; UI é shell fino sobre commands.
- Parser binário: bounds check em todo acesso; probes retornam confidence 0.0 em
  vez de panicar. Teste com input truncado/vazio/aleatório é obrigatório.
- Arquivo original do usuário: nunca copiado pro projeto, nunca modificado.
  Escrita futura só em working copy + verify.
- Nenhuma ROM comercial, BIOS, key ou byte de jogo real no repo — fixtures são
  sintéticas (`crates/core/src/synth.rs`).
- Secrets fora de logs, config versionada e repo.
- Sem download de jogos, sem links pra ROMs, sem promessa de suporte universal.
- UI: strings por ID de mensagem em `apps/desktop/src/i18n.ts` (pt-BR + en-US
  sempre juntos — teste de paridade quebra se faltar chave).

## Validação (tudo precisa passar antes de commit)

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
pnpm lint && pnpm test && pnpm web:build
```
