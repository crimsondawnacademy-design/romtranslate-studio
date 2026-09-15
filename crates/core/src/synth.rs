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

    // Strings plantadas para demo/teste do scanner (offsets estaveis; zeros ao
    // redor separam os runs). Header checksum cobre so 0xA0..=0xBC — nao muda.
    plant(&mut rom, 0x100, b"WELCOME TO THE VILLAGE!\0");
    plant(&mut rom, 0x120, b"POTION\0");
    plant(&mut rom, 0x128, b"HP {0}: 120\0");
    for (k, u) in "SYNTH QUEST".encode_utf16().enumerate() {
        rom[0x140 + k * 2..0x142 + k * 2].copy_from_slice(&u.to_le_bytes());
    }
    rom
}

fn plant(rom: &mut [u8], offset: usize, bytes: &[u8]) {
    rom[offset..offset + bytes.len()].copy_from_slice(bytes);
}

/// NES (iNES): header + 16 KiB PRG + 8 KiB CHR coerentes com o declarado,
/// filler nao-printable (>= 0x80) e strings ASCII plantadas para o scanner.
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
        *b = 0x80 | ((i % 0x60) as u8); // deterministico e nunca printable
    }
    plant(&mut rom, 0x100, b"PLAY BALL!\0");
    plant(&mut rom, 0x120, b"GAME OVER\0");
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

    // Strings plantadas no corpo, longe do header interno.
    plant(&mut rom, 0x1000, b"MAGIC SWORD\0");
    plant(&mut rom, 0x1010, b"NEW QUEST\0");

    // Par complement/checksum com a SOMA REAL (campos ainda zerados + 0x1FE).
    let checksum = crate::adapters::snes::snes_sum(&rom).wrapping_add(0x1FE);
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

/// Fixture RTSF (formato proprio, ver `adapters::rtsf`): 4 strings fixas em
/// slots de 24 bytes + 4 relocaveis num blob com folga, checksum correto.
pub fn make_rtsf_fixture() -> Vec<u8> {
    use crate::adapters::rtsf::{compute_checksum, HEADER_LEN, MAGIC, VERSION};

    let fixed: [&str; 4] = ["SAVE GAME", "LOAD GAME", "OPTIONS", "EXIT"];
    let reloc: [&str; 4] = [
        "WELCOME TO THE VILLAGE!",
        "HP {0} RESTORED",
        "YOU FOUND A POTION",
        "THANKS FOR PLAYING",
    ];
    let slot_size = 24usize;
    let fixed_offset = HEADER_LEN;
    let ptr_offset = fixed_offset + fixed.len() * slot_size;
    let blob_offset = ptr_offset + reloc.len() * 4;
    let blob_capacity = 256usize; // folga p/ traducoes maiores
    let total = blob_offset + blob_capacity;

    let mut data = vec![0u8; total];
    data[0..4].copy_from_slice(MAGIC);
    data[4] = VERSION;
    data[5] = fixed.len() as u8;
    data[6] = slot_size as u8;
    data[0x08..0x0C].copy_from_slice(&(fixed_offset as u32).to_le_bytes());
    data[0x0C..0x10].copy_from_slice(&(ptr_offset as u32).to_le_bytes());
    data[0x10..0x14].copy_from_slice(&(reloc.len() as u32).to_le_bytes());
    data[0x14..0x18].copy_from_slice(&(blob_offset as u32).to_le_bytes());
    data[0x18..0x1C].copy_from_slice(&(blob_capacity as u32).to_le_bytes());

    for (i, text) in fixed.iter().enumerate() {
        let off = fixed_offset + i * slot_size;
        data[off..off + text.len()].copy_from_slice(text.as_bytes());
    }
    let mut cursor = blob_offset;
    for (i, text) in reloc.iter().enumerate() {
        data[cursor..cursor + text.len()].copy_from_slice(text.as_bytes());
        let ptr_pos = ptr_offset + i * 4;
        data[ptr_pos..ptr_pos + 4].copy_from_slice(&(cursor as u32).to_le_bytes());
        cursor += text.len() + 1;
    }

    let checksum = compute_checksum(&data);
    data[0x1C..0x20].copy_from_slice(&checksum.to_le_bytes());
    data
}

/// NDS sintetico: header com CRC-16 valido e campo do logo = 0xCF56, filesystem
/// FNT/FAT com 2 arquivos na raiz — um com strings ASCII, outro com UTF-16LE.
pub fn make_nds_rom() -> Vec<u8> {
    use crate::adapters::nds::{crc16, LOGO_CRC_EXPECTED};

    let fnt_offset = 0x200usize;
    let fat_offset = 0x220usize;
    let file1_offset = 0x240usize;
    let file2_offset = 0x280usize;

    let file1: Vec<u8> = b"WELCOME TO THE SYNTH DS!\0PRESS START BUTTON\0".to_vec();
    let mut file2: Vec<u8> = Vec::new();
    for text in ["START GAME", "OPTIONS MENU"] {
        for u in text.encode_utf16() {
            file2.extend_from_slice(&u.to_le_bytes());
        }
        file2.extend_from_slice(&[0, 0]);
    }

    let mut rom = vec![0u8; 0x300];
    rom[0..7].copy_from_slice(b"SYNTHDS");
    rom[0x0C..0x10].copy_from_slice(b"ASYP");
    rom[0x10..0x12].copy_from_slice(b"01");

    // FNT: main table da raiz (subtable em +8, first file id 0, 1 diretorio).
    let mut fnt = Vec::new();
    fnt.extend_from_slice(&8u32.to_le_bytes());
    fnt.extend_from_slice(&0u16.to_le_bytes());
    fnt.extend_from_slice(&1u16.to_le_bytes());
    for name in ["intro.txt", "menu.bin"] {
        fnt.push(name.len() as u8);
        fnt.extend_from_slice(name.as_bytes());
    }
    fnt.push(0);

    let mut fat = Vec::new();
    for (start, len) in [(file1_offset, file1.len()), (file2_offset, file2.len())] {
        fat.extend_from_slice(&(start as u32).to_le_bytes());
        fat.extend_from_slice(&((start + len) as u32).to_le_bytes());
    }

    rom[0x40..0x44].copy_from_slice(&(fnt_offset as u32).to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&(fnt.len() as u32).to_le_bytes());
    rom[0x48..0x4C].copy_from_slice(&(fat_offset as u32).to_le_bytes());
    rom[0x4C..0x50].copy_from_slice(&(fat.len() as u32).to_le_bytes());
    rom[0x15C..0x15E].copy_from_slice(&LOGO_CRC_EXPECTED.to_le_bytes());

    rom[fnt_offset..fnt_offset + fnt.len()].copy_from_slice(&fnt);
    rom[fat_offset..fat_offset + fat.len()].copy_from_slice(&fat);
    rom[file1_offset..file1_offset + file1.len()].copy_from_slice(&file1);
    rom[file2_offset..file2_offset + file2.len()].copy_from_slice(&file2);

    // CRC do header por ultimo (cobre 0x000..0x15E, incluindo o campo do logo).
    let crc = crc16(&rom[..0x15E]);
    rom[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());
    rom
}

/// Header sintetico de disco GameCube (magic em 0x1C + game code + titulo).
pub fn make_gc_disc_header() -> Vec<u8> {
    use crate::adapters::gamecube::{MAGIC, MAGIC_OFFSET};
    let mut disc = vec![0u8; 4096];
    disc[0..6].copy_from_slice(b"GSYP01");
    disc[MAGIC_OFFSET..MAGIC_OFFSET + 4].copy_from_slice(&MAGIC);
    let title = b"SYNTHETIC GC ADVENTURE";
    disc[0x20..0x20 + title.len()].copy_from_slice(title);
    disc
}

/// Header sintetico de disco Wii (magic em 0x18 + titulo).
pub fn make_wii_disc_header() -> Vec<u8> {
    use crate::adapters::wii::{MAGIC, MAGIC_OFFSET};
    let mut disc = vec![0u8; 4096];
    disc[0..6].copy_from_slice(b"RSYP01");
    disc[MAGIC_OFFSET..MAGIC_OFFSET + 4].copy_from_slice(&MAGIC);
    let title = b"SYNTHETIC WII QUEST";
    disc[0x20..0x20 + title.len()].copy_from_slice(title);
    disc
}

/// Header sintetico WUX (WudCompress/Cemu): magic dupla + sectorSize 32 KiB +
/// uncompressedSize de uma imagem de disco tipica.
pub fn make_wux_header() -> Vec<u8> {
    use crate::adapters::wiiu::{WUX_MAGIC0, WUX_MAGIC1};
    let mut data = vec![0u8; 4096];
    data[0..4].copy_from_slice(WUX_MAGIC0);
    data[4..8].copy_from_slice(&WUX_MAGIC1.to_le_bytes());
    data[8..12].copy_from_slice(&0x8000u32.to_le_bytes()); // 32 KiB por setor
    data[0x0C..0x14].copy_from_slice(&(23u64 * 1024 * 1024 * 1024).to_le_bytes());
    data
}

/// Header sintetico RPX: ELF 32-bit big-endian PowerPC com OSABI/versao "CAFE".
pub fn make_rpx_header() -> Vec<u8> {
    use crate::adapters::wiiu::{CAFE_ABI_VERSION, CAFE_OSABI};
    let mut data = vec![0u8; 4096];
    data[0..4].copy_from_slice(b"\x7FELF");
    data[4] = 1; // ELFCLASS32
    data[5] = 2; // big-endian
    data[6] = 1; // EV_CURRENT
    data[7] = CAFE_OSABI;
    data[8] = CAFE_ABI_VERSION;
    data[0x10..0x12].copy_from_slice(&0xFE01u16.to_be_bytes()); // e_type Cafe RPL
    data[0x12..0x14].copy_from_slice(&20u16.to_be_bytes()); // EM_PPC
    data
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
