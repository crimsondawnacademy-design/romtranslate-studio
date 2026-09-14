# Fixtures

Somente dados **sintéticos** — nenhum byte de jogo comercial, BIOS ou firmware.

Os arquivos de exemplo não são commitados; gere localmente:

```bash
cargo run -p romtranslate-core --example gen_fixtures
```

Saída em `fixtures/generated/`:

| arquivo | o que é |
|---|---|
| `synthetic.gba` | header GBA válido (0x96 + checksum) |
| `synthetic.nes` | iNES com PRG/CHR coerentes |
| `synthetic_lorom.sfc` | SNES LoROM com header interno válido |
| `synthetic_headered.smc` | idem, com copier header de 512 bytes |
| `not_a_rom.bin` | bytes aleatórios (teste negativo) |

Os geradores vivem em `crates/core/src/synth.rs` e são os mesmos usados nos testes.
