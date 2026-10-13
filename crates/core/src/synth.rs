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

/// GBA com tabelas de ponteiros de ROM (0x08000000 + offset, LE):
/// - 0x800: tabela de 3 -> NEW GAME, CONTINUE, OPTIONS;
/// - 0x880: tabela de 2 -> CONTINUE, OPTIONS (string com 2 referencias);
/// - 0x900: ponteiro ISOLADO -> GAME OVER (literal pool: nao e tabela).
pub fn make_gba_rom_with_pointers() -> Vec<u8> {
    let mut rom = make_gba_rom("PTRTEST");
    rom.resize(4096, 0);
    plant(&mut rom, 0x400, b"NEW GAME\0");
    plant(&mut rom, 0x410, b"CONTINUE\0");
    plant(&mut rom, 0x420, b"OPTIONS\0");
    plant(&mut rom, 0x440, b"GAME OVER\0");
    let mut put_ptr = |at: usize, target: u32| {
        rom[at..at + 4].copy_from_slice(&(0x0800_0000 + target).to_le_bytes());
    };
    for (i, target) in [0x400, 0x410, 0x420].into_iter().enumerate() {
        put_ptr(0x800 + i * 4, target);
    }
    put_ptr(0x880, 0x410);
    put_ptr(0x884, 0x420);
    put_ptr(0x900, 0x440);
    rom
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
    build_nds(&base_nds_files())
}

fn base_nds_files() -> Vec<(&'static str, Vec<u8>)> {
    let mut menu: Vec<u8> = Vec::new();
    for text in ["START GAME", "OPTIONS MENU"] {
        for u in text.encode_utf16() {
            menu.extend_from_slice(&u.to_le_bytes());
        }
        menu.extend_from_slice(&[0, 0]);
    }
    vec![
        (
            "intro.txt",
            b"WELCOME TO THE SYNTH DS!\0PRESS START BUTTON\0".to_vec(),
        ),
        ("menu.bin", menu),
    ]
}

/// Arquivo de mensagens: contagem + TABELA de 3 offsets u32 relativos ao
/// arquivo + strings, e depois uma tabela de so 2 (nao pode ser aceita: em
/// offset relativo o run minimo e 3).
fn message_table_file() -> Vec<u8> {
    let mut msg = vec![0u8; 0x38];
    msg[0..4].copy_from_slice(&3u32.to_le_bytes());
    for (i, target) in [0x10u32, 0x19, 0x22].into_iter().enumerate() {
        msg[4 + i * 4..8 + i * 4].copy_from_slice(&target.to_le_bytes());
    }
    plant(&mut msg, 0x10, b"NEW GAME\0");
    plant(&mut msg, 0x19, b"CONTINUE\0");
    plant(&mut msg, 0x22, b"OPTIONS\0");
    msg[0x2C..0x30].copy_from_slice(&0x10u32.to_le_bytes());
    msg[0x30..0x34].copy_from_slice(&0x19u32.to_le_bytes());
    msg
}

/// Mesmo tipo de arquivo com offsets u16: tabela de 4 (conta) e depois uma
/// de 3 (nao conta: em u16 o run minimo e 4).
fn message_table_u16_file() -> Vec<u8> {
    let mut msg = vec![0u8; 0x34];
    msg[0..2].copy_from_slice(&4u16.to_le_bytes());
    for (i, target) in [0x0Cu16, 0x15, 0x1E, 0x26].into_iter().enumerate() {
        msg[2 + i * 2..4 + i * 2].copy_from_slice(&target.to_le_bytes());
    }
    plant(&mut msg, 0x0C, b"NEW GAME\0");
    plant(&mut msg, 0x15, b"CONTINUE\0");
    plant(&mut msg, 0x1E, b"OPTIONS\0");
    plant(&mut msg, 0x26, b"EXIT\0");
    for (i, target) in [0x0Cu16, 0x15, 0x1E].into_iter().enumerate() {
        msg[0x2C + i * 2..0x2E + i * 2].copy_from_slice(&target.to_le_bytes());
    }
    msg
}

/// String no offset 0 do arquivo + padding de zeros: sem a regra "alvo 0 nao
/// conta", cada palavra zerada viraria ponteiro pra ela.
fn zero_padded_title_file() -> Vec<u8> {
    let mut title = b"TITLE SCREEN\0".to_vec();
    title.resize(32, 0);
    title
}

/// NDS com tabelas de offsets relativos alem dos arquivos base: `msg.bin`
/// (u32), `msg16.bin` (u16) e `title.bin` (string no offset 0 + zeros).
pub fn make_nds_rom_with_message_table() -> Vec<u8> {
    let mut files = base_nds_files();
    files.push(("msg.bin", message_table_file()));
    files.push(("msg16.bin", message_table_u16_file()));
    files.push(("title.bin", zero_padded_title_file()));
    build_nds(&files)
}

/// NDS minimo: header + FNT (so a raiz) + FAT + arquivos alinhados em 0x40.
fn build_nds(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use crate::adapters::nds::{crc16, LOGO_CRC_EXPECTED};

    // FNT: main table da raiz (subtable em +8, first file id 0, 1 diretorio).
    let mut fnt = Vec::new();
    fnt.extend_from_slice(&8u32.to_le_bytes());
    fnt.extend_from_slice(&0u16.to_le_bytes());
    fnt.extend_from_slice(&1u16.to_le_bytes());
    for (name, _) in files {
        fnt.push(name.len() as u8);
        fnt.extend_from_slice(name.as_bytes());
    }
    fnt.push(0);

    let fnt_offset = 0x200usize;
    let fat_offset = (fnt_offset + fnt.len()).next_multiple_of(0x20);
    let mut fat = Vec::new();
    let mut starts = Vec::new();
    let mut cursor = (fat_offset + files.len() * 8).next_multiple_of(0x40);
    for (_, bytes) in files {
        starts.push(cursor);
        fat.extend_from_slice(&(cursor as u32).to_le_bytes());
        fat.extend_from_slice(&((cursor + bytes.len()) as u32).to_le_bytes());
        cursor = (cursor + bytes.len()).next_multiple_of(0x40);
    }

    let mut rom = vec![0u8; cursor.next_multiple_of(0x100)];
    rom[0..7].copy_from_slice(b"SYNTHDS");
    rom[0x0C..0x10].copy_from_slice(b"ASYP");
    rom[0x10..0x12].copy_from_slice(b"01");
    rom[0x40..0x44].copy_from_slice(&(fnt_offset as u32).to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&(fnt.len() as u32).to_le_bytes());
    rom[0x48..0x4C].copy_from_slice(&(fat_offset as u32).to_le_bytes());
    rom[0x4C..0x50].copy_from_slice(&(fat.len() as u32).to_le_bytes());
    rom[0x15C..0x15E].copy_from_slice(&LOGO_CRC_EXPECTED.to_le_bytes());

    rom[fnt_offset..fnt_offset + fnt.len()].copy_from_slice(&fnt);
    rom[fat_offset..fat_offset + fat.len()].copy_from_slice(&fat);
    for ((_, bytes), start) in files.iter().zip(starts) {
        rom[start..start + bytes.len()].copy_from_slice(bytes);
    }

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
    build_iso9660_at(system_id, volume_id, files, 18)
}

/// Setor do diretorio raiz dos discos "de DVD" sinteticos: depois da janela
/// de probe (128 KiB = setor 64), como num DVD de verdade, onde a ponte UDF
/// ocupa os setores ate a ancora dela, no 256.
pub const FAR_ROOT_LBA: u32 = 262;

/// O mesmo ISO, com o diretorio raiz em `root_lba`; subdiretorios e arquivos
/// vem logo depois dele.
fn build_iso9660_at(
    system_id: &str,
    volume_id: &str,
    files: &[(&str, Vec<u8>)],
    root_lba: u32,
) -> Vec<u8> {
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

    // Layout: subdirs ganham 1 setor cada depois da raiz; arquivos depois.
    let mut subdirs: Vec<&str> = Vec::new();
    for (path, _) in files {
        if let Some((dir, _)) = path.split_once('/') {
            if !subdirs.contains(&dir) {
                subdirs.push(dir);
            }
        }
    }
    let first_dir_lba = root_lba + 1;
    let first_file_lba = first_dir_lba + subdirs.len() as u32;
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
            let lba = first_dir_lba + i as u32;
            let mut recs = Vec::new();
            recs.extend(record(&[0], lba, S as u32, true));
            recs.extend(record(&[1], root_lba, S as u32, true));
            recs
        })
        .collect();
    for (i, dir) in subdirs.iter().enumerate() {
        root_records.extend(record(
            dir.as_bytes(),
            first_dir_lba + i as u32,
            S as u32,
            true,
        ));
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
        let base = (first_dir_lba as usize + i) * S;
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

/// PS1 com `MSG.DAT` (tabela u32 de offsets relativos, sobra no setor final),
/// `FULL.DAT` (mesma estrutura com 2048 bytes exatos: nenhuma sobra),
/// `MSG16.DAT` (tabela u16) e `TITLE.DAT` (string no offset 0 + zeros).
pub fn make_ps1_bin_with_message_table() -> Vec<u8> {
    let mut full = message_table_file();
    full.resize(2048, 0);
    let plain = build_iso9660(
        "PLAYSTATION",
        "SYNTH_PS1",
        &[
            (
                "SYSTEM.CNF",
                b"BOOT = cdrom:\\SLUS_012.34;1\r\nTCB = 4\r\n".to_vec(),
            ),
            ("MSG.DAT", message_table_file()),
            ("FULL.DAT", full),
            ("MSG16.DAT", message_table_u16_file()),
            ("TITLE.DAT", zero_padded_title_file()),
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
    ps2_iso(18)
}

/// O mesmo disco de PS2 com o layout de um DVD: diretorio raiz e SYSTEM.CNF
/// depois da janela de probe (`FAR_ROOT_LBA`).
pub fn make_ps2_dvd_iso() -> Vec<u8> {
    ps2_iso(FAR_ROOT_LBA)
}

fn ps2_iso(root_lba: u32) -> Vec<u8> {
    let mut pak = vec![0u8; 1024];
    plant(&mut pak, 0x20, b"PRESS X TO JUMP\0");
    plant(&mut pak, 0x40, b"SAVE PROGRESS?\0");
    build_iso9660_at(
        "PLAYSTATION",
        "SYNTH_PS2",
        &[
            (
                "SYSTEM.CNF",
                b"BOOT2 = cdrom0:\\SLUS_205.67;1\r\nVER = 1.00\r\nVMODE = NTSC\r\n".to_vec(),
            ),
            ("DATA/TEXT.PAK", pak),
        ],
        root_lba,
    )
}

/// Palavras de menu do `DATA/MENU.BIN` de `make_ps2_iso_with_noise`.
pub const NOISE_ISO_MENU: [&str; 3] = ["SAVE", "LOAD", "EXIT"];

/// PS2 como um disco de verdade costuma ser: 1 MiB de dado aleatorio
/// (`DATA/MOVIE.BIN`, o que video, audio e arquivo comprimido parecem pro
/// scanner) ao lado do texto de `make_ps2_iso` (`DATA/TEXT.PAK`) e de um
/// arquivo de 1 KiB so com palavras de menu (`DATA/MENU.BIN`).
pub fn make_ps2_iso_with_noise() -> Vec<u8> {
    let mut pak = vec![0u8; 1024];
    plant(&mut pak, 0x20, b"PRESS X TO JUMP\0");
    plant(&mut pak, 0x40, b"SAVE PROGRESS?\0");
    let mut menu = vec![0u8; 1024];
    for (i, word) in NOISE_ISO_MENU.iter().enumerate() {
        plant(&mut menu, 0x10 + i * 8, word.as_bytes());
    }
    build_iso9660(
        "PLAYSTATION",
        "SYNTH_PS2_BIG",
        &[
            (
                "SYSTEM.CNF",
                b"BOOT2 = cdrom0:\\SLUS_205.67;1\r\nVER = 1.00\r\nVMODE = NTSC\r\n".to_vec(),
            ),
            ("DATA/MOVIE.BIN", make_random(1024 * 1024, 77)),
            ("DATA/TEXT.PAK", pak),
            ("DATA/MENU.BIN", menu),
        ],
    )
}

/// PSP: ISO 2048 de UMD com UMD_DATA.BIN e PSP_GAME/PARAM.SFO validos.
pub fn make_psp_iso() -> Vec<u8> {
    psp_iso(18)
}

/// O mesmo UMD com o diretorio raiz depois da janela de probe.
pub fn make_psp_iso_with_far_root() -> Vec<u8> {
    psp_iso(FAR_ROOT_LBA)
}

fn psp_iso(root_lba: u32) -> Vec<u8> {
    let mut data = vec![0u8; 1024];
    plant(&mut data, 0x10, b"NEW GAME\0");
    plant(&mut data, 0x20, b"CONTINUE ADVENTURE\0");
    build_iso9660_at(
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
        root_lba,
    )
}

/// Tabela de caracteres inventada pro cartucho de `make_custom_table_rom`:
/// nenhuma letra cai onde o ASCII a poria.
pub const CUSTOM_UPPER: u8 = 0x80;
pub const CUSTOM_LOWER: u8 = 0xA0;
pub const CUSTOM_SPACE: u8 = 0x40;

/// Texto (so letras e espaco) na tabela inventada acima.
pub fn encode_custom(text: &str) -> Vec<u8> {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' => CUSTOM_UPPER + (b - b'A'),
            b'a'..=b'z' => CUSTOM_LOWER + (b - b'a'),
            b' ' => CUSTOM_SPACE,
            other => panic!("encode_custom: {other:#04X} fora da tabela sintetica"),
        })
        .collect()
}

/// As frases de `make_custom_table_rom`, na ordem em que estao no cartucho.
pub const CUSTOM_LINES: [&str; 7] = [
    "START GAME",
    "PRESS START",
    "CONTINUE",
    "Press Start to begin your quest",
    "The hero went north to the old tower",
    "the village is quiet tonight",
    "Start over from the last save point",
];

/// NES (iNES) cujo texto esta numa tabela propria, como em cartucho de
/// verdade: o scanner ASCII nao acha nada aqui. Frases a partir de 0x100,
/// uma a cada 0x40 bytes, terminadas em 0x00.
pub fn make_custom_table_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = 1;
    rom[5] = 1;
    for (i, line) in CUSTOM_LINES.iter().enumerate() {
        plant(&mut rom, 0x100 + i * 0x40, &encode_custom(line));
    }
    rom
}

/// As falas comprimidas dentro de `make_gba_rom_with_lz77`.
pub const LZ77_SCRIPT: [&str; 4] = [
    "Welcome to the village, traveler.",
    "The old bridge to the north is broken.",
    "Come back when you have the silver key.",
    "Welcome to the village, traveler. The inn is open.",
];

/// GBA de 64 KiB com texto que so existe comprimido: um bloco LZ77 (tipo
/// 0x10) em 0x1000 com as falas de `LZ77_SCRIPT` separadas por 0x00, 16 KiB
/// de ruido em 0x4000 (o que um dado cifrado pareceria) e zeros no resto.
/// Devolve o ROM e a posicao do bloco.
pub fn make_gba_rom_with_lz77() -> (Vec<u8>, usize) {
    let mut rom = make_gba_rom("LZTEST");
    rom.resize(64 * 1024, 0);
    let script: Vec<u8> = LZ77_SCRIPT
        .iter()
        .flat_map(|line| line.bytes().chain([0]))
        .collect();
    let block = crate::lz77::pack(&script).expect("script cabe em 24 bits");
    plant(&mut rom, 0x1000, &block);
    plant(&mut rom, 0x4000, &make_random(0x4000, 99));
    (rom, 0x1000)
}

/// Falas do bloco comprimido em 0x1000 de `make_gba_rom_with_lz77_table`:
/// quase iguais entre si, pra comprimir muito e crescer quando traduzidas.
pub const LZ77_TABLE_LINES: [&str; 3] = [
    "Welcome to the village, traveler.",
    "Welcome to the village, stranger.",
    "Welcome to the village, my friend.",
];
/// As do bloco em 0x2000 (o outro alvo da tabela).
pub const LZ77_TABLE_OTHER: [&str; 2] = [
    "The shop opens at dawn every day.",
    "The shop closes at dusk every day.",
];
/// As do bloco em 0x3000, que nenhum ponteiro alcanca.
pub const LZ77_LOOSE_LINES: [&str; 3] = [
    "The gate is closed until morning.",
    "The gate is closed until evening.",
    "The gate is closed until the bell.",
];
/// A unica frase do bloco de "grafico" em 0x5000.
pub const LZ77_TILE_PHRASE: &str = "PRESS START TO PLAY";

/// GBA de 64 KiB com quatro blocos LZ77: 0x1000 e 0x2000 (texto, alvos de uma
/// tabela de 2 ponteiros de ROM em 0x800), 0x3000 (texto, sem ponteiro) e
/// 0x5000 (2 KiB de padrao de tile com uma frase so no meio — nao e texto).
pub fn make_gba_rom_with_lz77_table() -> Vec<u8> {
    let mut rom = make_gba_rom("LZTABLE");
    rom.resize(64 * 1024, 0);
    let script = |lines: &[&str]| -> Vec<u8> {
        let text: Vec<u8> = lines
            .iter()
            .flat_map(|line| line.bytes().chain([0]))
            .collect();
        crate::lz77::pack(&text).expect("script cabe em 24 bits")
    };
    plant(&mut rom, 0x1000, &script(&LZ77_TABLE_LINES));
    plant(&mut rom, 0x2000, &script(&LZ77_TABLE_OTHER));
    plant(&mut rom, 0x3000, &script(&LZ77_LOOSE_LINES));
    let mut tiles: Vec<u8> = (0..2048usize)
        .map(|i| 0x80 | ((i % 64) * 7 % 16) as u8 | ((((i % 64) * 3 % 8) as u8) << 4))
        .collect();
    tiles[1024..1024 + LZ77_TILE_PHRASE.len()].copy_from_slice(LZ77_TILE_PHRASE.as_bytes());
    plant(
        &mut rom,
        0x5000,
        &crate::lz77::pack(&tiles).expect("tiles cabem em 24 bits"),
    );
    for (i, target) in [0x1000u32, 0x2000].into_iter().enumerate() {
        rom[0x800 + i * 4..0x804 + i * 4].copy_from_slice(&(0x0800_0000 + target).to_le_bytes());
    }
    rom
}

/// Falas em japones (inventadas) de `make_gba_rom_japanese`, gravadas em
/// Shift-JIS. A saudacao nao tem ponteiro; "sim", "nao" e o aviso do item
/// estao numa tabela de ponteiros de ROM.
pub const JP_GREETING: &str = "むらへ ようこそ、たびびとさん。";
pub const JP_YES: &str = "はい";
pub const JP_NO: &str = "いいえ";
pub const JP_FOUND: &str = "やくそうを てにいれた！";
/// Japones com ASCII no meio, como um aviso de batalha.
pub const JP_MIXED: &str = "HPが 30 かいふくした！";
/// Palavra so de kanji: sem kana, a regra nao a separa de dado binario.
pub const JP_KANJI_ONLY: &str = "装備";

fn plant_sjis(rom: &mut [u8], offset: usize, text: &str) {
    let mut bytes = crate::sjis::encode(text).expect("texto da fixture cabe em Shift-JIS");
    bytes.push(0);
    plant(rom, offset, &bytes);
}

/// GBA de 4 KiB com texto em Shift-JIS (alem do ASCII de `make_gba_rom`):
/// `JP_GREETING` em 0x400, `JP_YES` em 0x440, `JP_NO` em 0x448, `JP_FOUND` em
/// 0x460, `JP_KANJI_ONLY` em 0x4A0, "NEW GAME" em 0x4C0, `JP_MIXED` em 0x4E0 e
/// a tabela de 3 ponteiros de ROM em 0x800 (sim, nao, aviso do item).
pub fn make_gba_rom_japanese() -> Vec<u8> {
    let mut rom = make_gba_rom("JPTEST");
    rom.resize(4096, 0);
    plant_sjis(&mut rom, 0x400, JP_GREETING);
    plant_sjis(&mut rom, 0x440, JP_YES);
    plant_sjis(&mut rom, 0x448, JP_NO);
    plant_sjis(&mut rom, 0x460, JP_FOUND);
    plant_sjis(&mut rom, 0x4A0, JP_KANJI_ONLY);
    plant(&mut rom, 0x4C0, b"NEW GAME\0");
    plant_sjis(&mut rom, 0x4E0, JP_MIXED);
    for (i, target) in [0x440u32, 0x448, 0x460].into_iter().enumerate() {
        rom[0x800 + i * 4..0x804 + i * 4].copy_from_slice(&(0x0800_0000 + target).to_le_bytes());
    }
    rom
}

/// PS2 com um arquivo de mensagens em Shift-JIS (`DATA/MSG.BIN`:
/// `JP_GREETING` em 0x10, `JP_YES` em 0x40, "CONTINUE" em 0x60).
pub fn make_ps2_iso_japanese() -> Vec<u8> {
    let mut msg = vec![0u8; 512];
    plant_sjis(&mut msg, 0x10, JP_GREETING);
    plant_sjis(&mut msg, 0x40, JP_YES);
    plant(&mut msg, 0x60, b"CONTINUE\0");
    build_iso9660(
        "PLAYSTATION",
        "SYNTH_PS2_JP",
        &[
            (
                "SYSTEM.CNF",
                b"BOOT2 = cdrom0:\\SLPS_250.01;1\r\nVER = 1.00\r\nVMODE = NTSC\r\n".to_vec(),
            ),
            ("DATA/MSG.BIN", msg),
        ],
    )
}

/// Texto de verdade do `make_gba_rom_with_junk`, na ordem em que aparece.
pub const JUNK_ROM_PHRASES: [&str; 3] = [
    "The old bridge is out. Take the forest road.",
    "You found a rusty key!",
    "Press START to begin",
];
/// Palavras de menu de 4 letras: o minimo que o scanner le por padrao.
pub const JUNK_ROM_MENU: [&str; 4] = ["SAVE", "ITEM", "Exit", "Load"];
/// Mais curtas ainda: so aparecem com `min_chars` menor.
pub const JUNK_ROM_SHORT: [&str; 3] = ["Yes", "No", "HP"];
/// Rotulos que nao parecem texto. Os dois primeiros tem ponteiro em tabela.
pub const JUNK_ROM_LABELS: [&str; 3] = ["LV01", "ST02", "LV03"];
/// Onde o ruido do `make_gba_rom_with_junk` comeca e acaba.
pub const JUNK_ROM_NOISE: std::ops::Range<u64> = 0x400..0x7FF0;

/// GBA de 64 KiB pro filtro de "parece texto". O lixo vem ANTES do texto,
/// como num cartucho de verdade (codigo e graficos primeiro):
/// - `JUNK_ROM_NOISE`: ruido pseudoaleatorio — centenas de sequencias
///   imprimiveis que nao sao texto;
/// - 0x8000: `JUNK_ROM_PHRASES`; 0x8100: `JUNK_ROM_MENU`; 0x8200:
///   `JUNK_ROM_SHORT`; 0x8300: `JUNK_ROM_LABELS`;
/// - 0x8400: tabela de ponteiros -> "LV01", "ST02" e "SAVE". "LV03" fica sem
///   ponteiro: mesma cara, e so ele cai no filtro.
pub fn make_gba_rom_with_junk() -> Vec<u8> {
    let mut rom = make_gba_rom("JUNKTEST");
    rom.resize(64 * 1024, 0);
    let noise = JUNK_ROM_NOISE.start as usize..JUNK_ROM_NOISE.end as usize;
    let len = noise.len();
    rom[noise].copy_from_slice(&make_random(len, 0x4A55));

    let mut at = 0x8000;
    for text in JUNK_ROM_PHRASES {
        plant(&mut rom, at, text.as_bytes());
        at += text.len().next_multiple_of(4) + 4;
    }
    for (base, group) in [
        (0x8100, &JUNK_ROM_MENU[..]),
        (0x8200, &JUNK_ROM_SHORT[..]),
        (0x8300, &JUNK_ROM_LABELS[..]),
    ] {
        for (i, text) in group.iter().enumerate() {
            plant(&mut rom, base + i * 8, text.as_bytes());
        }
    }
    for (i, target) in [0x8300u32, 0x8308, 0x8100].into_iter().enumerate() {
        plant(
            &mut rom,
            0x8400 + i * 4,
            &(0x0800_0000 + target).to_le_bytes(),
        );
    }
    rom
}

/// Falas ASCII de `make_gba_rom_with_names`, na ordem do cartucho.
pub const NAMES_ROM_LINES: [&str; 4] = [
    "Come back in the morning!",
    "\"No!\" he said, and ran off.",
    "The old bridge is out again.",
    "You came in first place!",
];
/// Texto de menu em maiusculas: frase com espaco e palavra unica.
pub const NAMES_ROM_MENU: [&str; 3] = ["NEW GAME", "START", "OPTIONS"];
/// Nomes internos (sala, script), todos com ponteiro em tabela.
pub const NAMES_ROM_NAMES: [&str; 4] = ["FASE4_31_01", "VILA_SET_1", "SALA12", "TORRE_0d_04"];
/// Onde ficam os nomes internos e a tabela de ponteiros deles.
pub const NAMES_ROM_INTERNAL: std::ops::Range<usize> = 0x700..0x800;
/// Onde esta, em UTF-16LE, a mesma frase de `NAMES_ROM_LINES[0]`.
pub const NAMES_ROM_WIDE: std::ops::Range<u64> = 0x800..0x840;

/// GBA de 4 KiB com o que um jogo de texto ASCII mistura:
/// - 0x400: `NAMES_ROM_LINES`, uma a cada 0x40 bytes;
/// - 0x600: `NAMES_ROM_MENU`, uma a cada 0x10;
/// - 0x700: `NAMES_ROM_NAMES`, uma a cada 0x10, e em 0x780 a tabela de
///   ponteiros pros quatro — nome interno, nao texto;
/// - 0x800: a primeira fala de novo, em UTF-16LE (texto que aceita acento).
pub fn make_gba_rom_with_names() -> Vec<u8> {
    let mut rom = make_gba_rom("NAMETEST");
    rom.resize(4096, 0);
    for (base, step, group) in [
        (0x400, 0x40, &NAMES_ROM_LINES[..]),
        (0x600, 0x10, &NAMES_ROM_MENU[..]),
        (NAMES_ROM_INTERNAL.start, 0x10, &NAMES_ROM_NAMES[..]),
    ] {
        for (i, text) in group.iter().enumerate() {
            plant(&mut rom, base + i * step, text.as_bytes());
        }
    }
    for i in 0..NAMES_ROM_NAMES.len() {
        let target = 0x0800_0000 + (NAMES_ROM_INTERNAL.start + i * 0x10) as u32;
        plant(&mut rom, 0x780 + i * 4, &target.to_le_bytes());
    }
    let wide: Vec<u8> = NAMES_ROM_LINES[0]
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    plant(&mut rom, NAMES_ROM_WIDE.start as usize, &wide);
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
