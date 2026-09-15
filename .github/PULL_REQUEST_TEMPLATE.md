## O que muda

<!-- resumo curto; issue relacionada com #numero -->

## Checklist

- [ ] `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace`
- [ ] `pnpm lint && pnpm test && pnpm web:build`
- [ ] Nenhum byte de jogo real/BIOS/keys — fixtures sintéticas apenas
- [ ] Parser novo: testado com input truncado/malformado (nunca panica)
- [ ] Adapter novo/alterado: round-trip testado e matriz do README atualizada
- [ ] Strings de UI novas em pt-BR **e** en-US (`i18n.ts`)
