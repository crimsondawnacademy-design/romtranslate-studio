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

/// Disco GameCube sintetico: header (magic 0x1C, fst_offset/size BE em
/// 0x424/0x428) + FST com raiz, um arquivo solto, um subdiretorio "data/" e
/// strings ASCII plantadas nos arquivos.
pub fn make_gc_disc() -> Vec<u8> {
    use crate::adapters::gamecube::{FST_OFFSET_FIELD, FST_SIZE_FIELD, MAGIC, MAGIC_OFFSET};

    let fst_offset = 0x1000usize;
    let file1_offset = 0x2000usize;
    let file2_offset = 0x2400usize;
    let file1: Vec<u8> = b"WELCOME TO GAMECUBE ISLAND!\0PRESS THE A BUTTON\0".to_vec();
    let mut file2: Vec<u8> = vec![0xFF; 8];
    file2.extend_from_slice(b"SOUND OPTIONS\0");

    let mut disc = vec![0u8; 0x3000];
    disc[0..6].copy_from_slice(b"GSYP01");
    disc[MAGIC_OFFSET..MAGIC_OFFSET + 4].copy_from_slice(&MAGIC);
    let title = b"SYNTHETIC GC ADVENTURE";
    disc[0x20..0x20 + title.len()].copy_from_slice(title);

    // String table: offsets 0="opening.txt", 12="data", 17="config.bin".
    let names = b"opening.txt\0data\0config.bin\0";
    // Entries (3x u32 BE): raiz + arquivo + dir "data" (filhos ate 4) + arquivo.
    let entries: [(u32, u32, u32); 4] = [
        (0x0100_0000, 0, 4),                           // raiz: total=4
        (0, file1_offset as u32, file1.len() as u32),  // opening.txt
        (0x0100_0000 | 12, 0, 4),                      // dir data/, next=4
        (17, file2_offset as u32, file2.len() as u32), // data/config.bin
    ];
    let mut fst = Vec::new();
    for (a, b, c) in entries {
        fst.extend_from_slice(&a.to_be_bytes());
        fst.extend_from_slice(&b.to_be_bytes());
        fst.extend_from_slice(&c.to_be_bytes());
    }
    fst.extend_from_slice(names);

    disc[FST_OFFSET_FIELD..FST_OFFSET_FIELD + 4]
        .copy_from_slice(&(fst_offset as u32).to_be_bytes());
    disc[FST_SIZE_FIELD..FST_SIZE_FIELD + 4].copy_from_slice(&(fst.len() as u32).to_be_bytes());
    disc[fst_offset..fst_offset + fst.len()].copy_from_slice(&fst);
    disc[file1_offset..file1_offset + file1.len()].copy_from_slice(&file1);
    disc[file2_offset..file2_offset + file2.len()].copy_from_slice(&file2);
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

/// Builder de imagem ISO 9660 sintetica (2048/setor): PVD no setor 16, root
/// no 18, um nivel opcional de subdiretorio ("DIR/ARQUIVO"). Suficiente para
/// as fixtures de PS1/PS2/PSP — nenhum byte de disco real.
fn build_iso9660(system_id: &str, volume_id: &str, files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    const S: usize = 2048;
    fn record(name: &[u8], extent: u32, size: u32, is_dir: bool) -> Vec<u8> {
        let mut r = vec![0u8; 33 + name.len()];
        if !r.len().is_multiple_of(2) {
            r.push(0); // records tem comprimento par (ECMA-119)
        }
        r[0] = r.len() as u8;
        r[2..6].copy_from_slice(&extent.to_le_bytes());
        r[6..10].copy_from_slice(&extent.to_be_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[14..18].copy_from_slice(&size.to_be_bytes());
        r[25] = if is_dir { 0x02 } else { 0x00 };
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    }

    // Layout: subdirs ganham 1 setor cada a partir do 19; arquivos depois.
    let mut subdirs: Vec<&str> = Vec::new();
    for (path, _) in files {
        if let Some((dir, _)) = path.split_once('/') {
            if !subdirs.contains(&dir) {
                subdirs.push(dir);
            }
        }
    }
    let root_lba = 18u32;
    let first_file_lba = 19 + subdirs.len() as u32;
    let mut file_lbas: Vec<u32> = Vec::new();
    let mut next = first_file_lba;
    for (_, content) in files {
        file_lbas.push(next);
        next += content.len().div_ceil(S).max(1) as u32;
    }
    let total_sectors = next as usize;
    let mut image = vec![0u8; total_sectors * S];

    // PVD (setor 16) + set terminator (17).
    let pvd = &mut image[16 * S..17 * S];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    let sys = system_id.as_bytes();
    pvd[8..8 + sys.len().min(32)].copy_from_slice(&sys[..sys.len().min(32)]);
    for b in pvd[8 + sys.len().min(32)..40].iter_mut() {
        *b = b' ';
    }
    let vol = volume_id.as_bytes();
    pvd[40..40 + vol.len().min(32)].copy_from_slice(&vol[..vol.len().min(32)]);
    let root_rec = record(&[0], root_lba, S as u32, true);
    pvd[156..156 + root_rec.len()].copy_from_slice(&root_rec);
    image[17 * S] = 255;
    image[17 * S + 1..17 * S + 6].copy_from_slice(b"CD001");

    // Diretorios: raiz + um setor por subdir.
    let mut root_records: Vec<u8> = Vec::new();
    root_records.extend(record(&[0], root_lba, S as u32, true));
    root_records.extend(record(&[1], root_lba, S as u32, true));
    let mut subdir_records: Vec<Vec<u8>> = subdirs
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let lba = 19 + i as u32;
            let mut recs = Vec::new();
            recs.extend(record(&[0], lba, S as u32, true));
            recs.extend(record(&[1], root_lba, S as u32, true));
            recs
        })
        .collect();
    for (i, dir) in subdirs.iter().enumerate() {
        root_records.extend(record(dir.as_bytes(), 19 + i as u32, S as u32, true));
    }
    for ((path, content), lba) in files.iter().zip(&file_lbas) {
        let (target, name) = match path.split_once('/') {
            Some((dir, name)) => (subdirs.iter().position(|d| d == &dir), name),
            None => (None, *path),
        };
        let iso_name = format!("{name};1");
        let rec = record(iso_name.as_bytes(), *lba, content.len() as u32, false);
        match target {
            Some(i) => subdir_records[i].extend(rec),
            None => root_records.extend(rec),
        }
    }
    image[root_lba as usize * S..root_lba as usize * S + root_records.len()]
        .copy_from_slice(&root_records);
    for (i, recs) in subdir_records.iter().enumerate() {
        let base = (19 + i) * S;
        image[base..base + recs.len()].copy_from_slice(recs);
    }
    for ((_, content), lba) in files.iter().zip(&file_lbas) {
        let base = *lba as usize * S;
        image[base..base + content.len()].copy_from_slice(content);
    }
    image
}

/// Converte uma imagem 2048/setor em raw 2352 (Mode 2 Form 1: sync + header +
/// subheader duplicado + EDC/ECC reais, como num BIN dumpado de verdade).
fn wrap_raw_2352(plain: &[u8]) -> Vec<u8> {
    const SYNC: [u8; 12] = [
        0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
    ];
    let mut out = Vec::with_capacity(plain.len() / 2048 * 2352);
    for chunk in plain.chunks(2048) {
        let mut sector = vec![0u8; 2352];
        sector[..12].copy_from_slice(&SYNC);
        sector[15] = 2; // Mode 2
                        // Subheader XA duplicado (4+4): submode 0x08 = data, Form 1.
        sector[18] = 0x08;
        sector[22] = 0x08;
        sector[24..24 + chunk.len()].copy_from_slice(chunk);
        crate::adapters::cdrom::regenerate_sector(&mut sector)
            .expect("synth: setor sintetico sempre regeneravel");
        out.extend_from_slice(&sector);
    }
    out
}

/// PARAM.SFO minimo (psdevwiki): magic \\0PSF + index de entries de 16 bytes.
fn build_sfo(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut keys = Vec::new();
    let mut data = Vec::new();
    let mut index = Vec::new();
    for (key, value) in pairs {
        let key_off = keys.len() as u16;
        let data_off = data.len() as u32;
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        let len = bytes.len() as u32;
        data.extend_from_slice(&bytes);
        index.extend_from_slice(&key_off.to_le_bytes());
        index.extend_from_slice(&0x0204u16.to_le_bytes()); // utf8
        index.extend_from_slice(&len.to_le_bytes());
        index.extend_from_slice(&len.to_le_bytes());
        index.extend_from_slice(&data_off.to_le_bytes());
    }
    let key_table_start = 0x14 + index.len() as u32;
    let data_table_start = key_table_start + keys.len() as u32;
    let mut sfo = Vec::new();
    sfo.extend_from_slice(&[0x00, b'P', b'S', b'F']);
    sfo.extend_from_slice(&0x0101u32.to_le_bytes());
    sfo.extend_from_slice(&key_table_start.to_le_bytes());
    sfo.extend_from_slice(&data_table_start.to_le_bytes());
    sfo.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
    sfo.extend_from_slice(&index);
    sfo.extend_from_slice(&keys);
    sfo.extend_from_slice(&data);
    sfo
}

/// PS1: BIN raw 2352 com SYSTEM.CNF (BOOT=) e strings num arquivo de dados
/// que atravessa fronteira de setor.
pub fn make_ps1_bin() -> Vec<u8> {
    let mut game = vec![0u8; 3000];
    plant(&mut game, 0x40, b"INSERT COIN TO CONTINUE\0");
    plant(&mut game, 0x820, b"MEMORY CARD NOT FOUND\0"); // 2o setor do arquivo
    let plain = build_iso9660(
        "PLAYSTATION",
        "SYNTH_PS1",
        &[
            (
                "SYSTEM.CNF",
                b"BOOT = cdrom:\\SLUS_012.34;1\r\nTCB = 4\r\nEVENT = 10\r\nSTACK = 801fff00\r\n"
                    .to_vec(),
            ),
            ("GAME.DAT", game),
        ],
    );
    wrap_raw_2352(&plain)
}

/// Dump single-file multi-track: o BIN de PS1 com setores de "audio" (PCM
/// cru, sem sync/header/EDC) anexados — layout de cue com FILE unico.
pub fn make_ps1_bin_with_audio() -> (Vec<u8>, usize) {
    let mut bin = make_ps1_bin();
    let audio_sectors = 3usize;
    for s in 0..audio_sectors {
        bin.extend((0..2352).map(|i| ((i * 31 + s * 17 + 7) & 0xFF) as u8));
    }
    (bin, audio_sectors)
}

/// PS2: ISO 2048 com SYSTEM.CNF (BOOT2=) e strings num subdiretorio.
pub fn make_ps2_iso() -> Vec<u8> {
    let mut pak = vec![0u8; 1024];
    plant(&mut pak, 0x20, b"PRESS X TO JUMP\0");
    plant(&mut pak, 0x40, b"SAVE PROGRESS?\0");
    build_iso9660(
        "PLAYSTATION",
        "SYNTH_PS2",
        &[
            (
                "SYSTEM.CNF",
                b"BOOT2 = cdrom0:\\SLUS_205.67;1\r\nVER = 1.00\r\nVMODE = NTSC\r\n".to_vec(),
            ),
            ("DATA/TEXT.PAK", pak),
        ],
    )
}

/// PSP: ISO 2048 de UMD com UMD_DATA.BIN e PSP_GAME/PARAM.SFO validos.
pub fn make_psp_iso() -> Vec<u8> {
    let mut data = vec![0u8; 1024];
    plant(&mut data, 0x10, b"NEW GAME\0");
    plant(&mut data, 0x20, b"CONTINUE ADVENTURE\0");
    build_iso9660(
        "PSP GAME",
        "SYNTH_PSP",
        &[
            (
                "UMD_DATA.BIN",
                b"ULUS-01234|1234567890ABCDEF|0001|G\0".to_vec(),
            ),
            (
                "PSP_GAME/PARAM.SFO",
                build_sfo(&[("DISC_ID", "ULUS01234"), ("TITLE", "SYNTHETIC PSP QUEST")]),
            ),
            ("PSP_GAME/DATA.BIN", data),
        ],
    )
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
