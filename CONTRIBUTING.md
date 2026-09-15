# Contribuindo

## Ambiente

- Rust estável (`rustup`), Node 20+, `pnpm`, pré-requisitos do
  [Tauri 2](https://tauri.app/start/prerequisites/).
- `pnpm install` na raiz, depois `pnpm dev`.

## Fluxo

1. Branch a partir de `main` (`feat/...`, `fix/...`, `adapter/...`).
2. Antes do PR, tudo verde:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test --workspace
   pnpm lint && pnpm test && pnpm web:build
   ```
3. Commits pequenos e logicamente separáveis, mensagem no imperativo.

## Criando um adapter de plataforma

O guia completo (contrato, regras, fixtures, testes obrigatórios e esqueleto)
está em **[docs/ADAPTERS.md](docs/ADAPTERS.md)** — leia antes de escrever
qualquer parser. Resumo do que não se negocia:

- parser **nunca** panica com input malformado (bounds check em tudo; testes
  com truncados/headers mentirosos são obrigatórios);
- `AdapterCapabilities` honestas — `reinsert: true` só com round-trip testado;
- fixtures 100% sintéticas em `synth.rs`; **nunca** commite ROM, BIOS, keys ou
  bytes copiados de jogos reais;
- adapter com reinserção atualiza a matriz de compatibilidade do README.

## Testes

`cargo test --workspace` roda unit + integração. Testes de parser binário devem
cobrir inputs truncados em tamanhos arbitrários (veja `truncated_inputs_never_panic`
em `crates/core/tests/pipeline.rs`).
