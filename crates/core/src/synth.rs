//! Fixtures sinteticas para testes e demo. Nenhum byte vem de jogo real:
//! sao arquivos minimos que satisfazem as estruturas de header documentadas.

use crate::adapters::gba::header_checksum;
use crate::adapters::snes::LOROM_HEADER;

/// GBA: 1 KiB com header valido (entry branch, titulo, 0x96 fixo, checksum).
pub fn make_gba_rom(title: &str) -> Vec<u8> {
    let mut rom = vec![0u8; 1024];
    rom[0..4].copy_from_slice(&[0x2E, 0x00, 0x00, 0xEA]); // b +0xB8 (entry ARM)
    let title_bytes = title.as_bytes();
    let n = title_bytes.len().min(12);
    rom[0xA0..0xA0 + n].copy_from_slice(&title_bytes[..n]);
    rom[0xAC..0xB0].copy_from_slice(b"RTSY"); // game code sintetico
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    rom[0xBD] = header_checksum(&rom);
    rom
}

/// NES (iNES): header + 16 KiB PRG + 8 KiB CHR coerentes com o declarado.
pub fn make_nes_rom() -> Vec<u8> {
    let prg_units = 1usize;
    let chr_units = 1usize;
    let mut rom = vec![0u8; 16 + prg_units * 16 * 1024 + chr_units * 8 * 1024];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = prg_units as u8;
    rom[5] = chr_units as u8;
    rom[6] = 0x00;
    rom[7] = 0x00;
    for (i, b) in rom.iter_mut().enumerate().skip(16) {
        *b = (i % 251) as u8; // conteudo deterministico, nao-texto
    }
    rom
}

/// SNES LoROM: 32 KiB com header interno em 0x7FC0 (titulo, map mode 0x20,
/// par checksum/complement consistente).
pub fn make_snes_lorom(title: &str) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    let h = LOROM_HEADER;
    for b in rom[h..h + 21].iter_mut() {
        *b = b' ';
    }
    let title_bytes = title.as_bytes();
    let n = title_bytes.len().min(21);
    rom[h..h + n].copy_from_slice(&title_bytes[..n]);
    rom[h + 0x15] = 0x20; // LoROM, slow
    rom[h + 0x16] = 0x00; // ROM only
    rom[h + 0x17] = 0x08; // 256 KiB declarado (plausibilidade nao verificada no probe)
    rom[h + 0x18] = 0x00; // sem SRAM
    rom[h + 0x19] = 0x01; // regiao
                          // Probe valida o PAR complement^checksum == 0xFFFF, nao a soma real do arquivo.
    let checksum: u16 = 0x1234;
    rom[h + 0x1C..h + 0x1E].copy_from_slice(&(checksum ^ 0xFFFF).to_le_bytes());
    rom[h + 0x1E..h + 0x20].copy_from_slice(&checksum.to_le_bytes());
    rom
}

/// SNES com copier header (.smc): 512 bytes de prefixo + LoROM.
pub fn make_snes_headered(title: &str) -> Vec<u8> {
    let mut rom = vec![0u8; 512];
    rom.extend(make_snes_lorom(title));
    rom
}

/// Bytes pseudo-aleatorios deterministicos (xorshift), p/ testes negativos.
pub fn make_random(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.max(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state & 0xFF) as u8
        })
        .collect()
}
