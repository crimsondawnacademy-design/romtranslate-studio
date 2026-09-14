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

1. Novo módulo em `crates/core/src/adapters/` implementando `GameAdapter`
   (`crates/core/src/adapter.rs`).
2. Regras inegociáveis:
   - `probe()` **nunca** panica com input malformado — todo acesso a bytes com
     bounds check; devolva `confidence: 0.0` quando não reconhecer;
   - declare `AdapterCapabilities` honestas (não anuncie `reinsert` sem round-trip
     testado);
   - `evidence` legível por humanos explicando a confiança.
3. Registre em `adapters::all()`.
4. Fixture sintética em `synth.rs` + testes: positivo, negativo (bytes aleatórios),
   truncado e vazio. **Nunca** commite ROM comercial, BIOS, keys ou headers
   copiados de jogos reais.

## Testes

`cargo test --workspace` roda unit + integração. Testes de parser binário devem
cobrir inputs truncados em tamanhos arbitrários (veja `truncated_inputs_never_panic`
em `crates/core/tests/pipeline.rs`).
