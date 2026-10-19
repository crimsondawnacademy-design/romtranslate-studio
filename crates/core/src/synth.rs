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

/// Sequencias com cara de texto plantadas em cada arquivo binario do
/// `make_nds_rom_with_binary_files`.
pub const NDS_BINARY_JUNK: [&str; 2] = ["FOREST THEME LOOP", "Castle Hall Ambience"];

/// NDS com os arquivos base mais tres de formato binario reconhecido pelo
/// cabecalho — som (`SDAT`), modelo 3D (`BMD0`) e video (`MODS`) —, cada um
/// com `NDS_BINARY_JUNK` dentro, e `fala.sdat`: nome de som, conteudo de
/// texto (o nome nao decide).
pub fn make_nds_rom_with_binary_files() -> Vec<u8> {
    let binary = |magic: &[u8]| {
        let mut file = magic.to_vec();
        file.resize(0x20, 0x01);
        for text in NDS_BINARY_JUNK {
            file.extend_from_slice(text.as_bytes());
            file.extend_from_slice(&[0, 0x81, 0x02]);
        }
        file
    };
    let mut files = base_nds_files();
    files.push(("bgm.sdat", binary(b"SDAT\xFF\xFE\x00\x01")));
    files.push(("hero.nsbmd", binary(b"BMD0\xFF\xFE\x02\x00")));
    files.push(("intro.mods", binary(b"MODSN3\x0C\x00")));
    files.push(("fala.sdat", b"THE BRIDGE IS OUT TONIGHT\0".to_vec()));
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

/// Quem faz e o que faz, nas mensagens (inventadas) de `make_gba_rom_jis16`:
/// cada mensagem e um sujeito, uma quebra de linha e um predicado.
pub const JIS16_SUBJECTS: [&str; 12] = [
    "むらの ひとは",
    "たびびとは",
    "こどもたちは",
    "としよりは",
    "みせの おやじは",
    "となりの ねこは",
    "ふねの せんちょうは",
    "もりの きこりは",
    "やまの かりうどは",
    "まちの パンやは",
    "みなとの りょうしは",
    "おかの ひつじかいは",
];
pub const JIS16_PREDICATES: [&str; 10] = [
    "きょうも げんきに はたらいている。",
    "あさから うたを うたっている。",
    "まだ ねむそうに している。",
    "あたらしい はしを つくっている。",
    "とおくの うみを ながめている。",
    "おおきな こえで わらっている。",
    "しずかに ほんを よんでいる。",
    "たのしそうに おどっている。",
    "ゆっくり さんぽを している。",
    "おいしい スープを つくっている。",
];
/// Nomes curtos da tabela de itens de `make_gba_rom_jis16`.
pub const JIS16_NAMES: [&str; 4] = ["リンゴ", "パン", "くすり", "ランプ"];
/// Um nome com um desenho proprio do jogo no meio (codigo `JIS16_GLYPH`, que
/// a tabela padrao deixa vazio), como a extracao o le.
pub const JIS16_GLYPH_NAME: &str = "ハ[03FE]ミツ";
pub const JIS16_GLYPH: u16 = 0x03FE;
/// O codigo que o cartucho usa como espaco entre palavras (fora do JIS).
pub const JIS16_SPACE: u16 = 0x00AC;
/// Onde comeca o roteiro de `make_gba_rom_jis16`.
pub const JIS16_SCRIPT_AT: usize = 0x2000;

/// Como a mensagem `i` de `make_gba_rom_jis16` e gravada, pelo sujeito dela
/// (de 6 em 6): as do sujeito 1 abrem com o comando `FF08` e o parametro
/// dele; as do 2, com um numero de cabecalho antes do losango; as do 3 tem o
/// comando `FF23` com parametro no meio; o resto e so texto.
fn jis16_pattern(i: usize) -> usize {
    (i / JIS16_PREDICATES.len()) % 6
}

/// A mensagem `i` de `make_gba_rom_jis16`, como a extracao a le: o losango,
/// o sujeito, a quebra de linha (`[FF01]`) e o predicado. O comando e o
/// numero que vem ANTES do losango nao fazem parte dela.
pub fn jis16_message(i: usize) -> String {
    format!(
        "◆{}{}[FF01]{}",
        JIS16_SUBJECTS[i / JIS16_PREDICATES.len()],
        if jis16_pattern(i) == 3 {
            "[FF23:0001]"
        } else {
            ""
        },
        JIS16_PREDICATES[i % JIS16_PREDICATES.len()]
    )
}

/// GBA de 64 KiB com o texto em codigos de 16 bits na ordem do JIS
/// (`crate::kuten`): em 0x1000 os nomes de `JIS16_NAMES` e `JIS16_GLYPH_NAME`,
/// cada um fechado por 0xFFFF; em `JIS16_SCRIPT_AT` as 120 mensagens de
/// `jis16_message`, cada uma seguida de 0xFF03 0xFF00 (espera o botao) e
/// 0xFFFF, nos quatro jeitos de `jis16_pattern`. O espaco entre palavras e
/// `JIS16_SPACE`. O ASCII de `make_gba_rom` continua la.
pub fn make_gba_rom_jis16() -> Vec<u8> {
    let mut rom = make_gba_rom("JIS16");
    rom.resize(64 * 1024, 0);
    let units = |text: &str| -> Vec<u16> {
        text.chars()
            .map(|c| match c {
                ' ' => JIS16_SPACE,
                c => crate::kuten::encode(c).expect("caractere da fixture existe no JIS"),
            })
            .collect()
    };
    let bytes = |codes: &[u16]| -> Vec<u8> { codes.iter().flat_map(|c| c.to_le_bytes()).collect() };

    let mut names = vec![0xFFFF];
    for name in JIS16_NAMES {
        names.extend(units(name));
        names.push(0xFFFF);
    }
    names.extend(units("ハ"));
    names.push(JIS16_GLYPH);
    names.extend(units("ミツ"));
    names.push(0xFFFF);
    plant(&mut rom, 0x1000, &bytes(&names));

    let mut script = Vec::new();
    for (s, subject) in JIS16_SUBJECTS.iter().enumerate() {
        for (p, predicate) in JIS16_PREDICATES.iter().enumerate() {
            let pattern = s % 6;
            match pattern {
                1 => script.extend([0xFF08, 0x0003]),
                2 => script.push(0x0005 + p as u16),
                _ => {}
            }
            script.push(crate::kuten::encode('◆').expect("losango"));
            script.extend(units(subject));
            if pattern == 3 {
                script.extend([0xFF23, 0x0001]);
            }
            script.push(0xFF01);
            script.extend(units(predicate));
            script.extend([0xFF03, 0xFF00, 0xFFFF]);
        }
    }
    plant(&mut rom, JIS16_SCRIPT_AT, &bytes(&script));
    rom
}

/// A fala com kanji de `make_gba_rom_jis16_font`, como a extracao a le.
pub const JIS16_FONT_LINE: &str = "ちちの 父は いま 茶を のんでいる。";
/// As linhas vazias do JIS que a fonte de `make_gba_rom_jis16_font` pula,
/// como a do jogo medido: todo kanji fica 6 * 94 posicoes antes da tabela
/// padrao.
pub const JIS16_FONT_SKIPPED: std::ops::RangeInclusive<usize> = 9..=14;

/// O `make_gba_rom_jis16` em 128 KiB com uma FONTE guardada como a do jogo
/// medido em 16/10/2026: em 0x10000, um desenho a cada 4 bytes, cada um com
/// o codigo Shift-JIS dele na frente, na ordem JIS das linhas 1 a 48 sem as
/// de `JIS16_FONT_SKIPPED`; em 0x18000, `JIS16_FONT_LINE` escrita com o
/// codigo do texto = posicao do desenho na fonte.
pub fn make_gba_rom_jis16_font() -> Vec<u8> {
    let mut rom = make_gba_rom_jis16();
    rom.resize(128 * 1024, 0);
    let mut font = Vec::new();
    for (row, col) in jis16_font_cells() {
        font.extend(sjis_of(row, col));
        font.extend([0, 0]);
    }
    plant(&mut rom, 0x10000, &font);
    // Kana pela tabela padrao (e igual na fonte); kanji pela posicao na fonte.
    let skipped = JIS16_FONT_SKIPPED.clone().count() * 94;
    let mut line: Vec<u16> = JIS16_FONT_LINE
        .chars()
        .map(|c| {
            if c == ' ' {
                return JIS16_SPACE;
            }
            let standard = crate::kuten::encode(c).expect("caractere existe no JIS") as usize;
            let row = standard / 94 + 1;
            let shift = if row > *JIS16_FONT_SKIPPED.end() {
                skipped
            } else {
                0
            };
            (standard - shift) as u16
        })
        .collect();
    line.push(0xFFFF);
    let bytes: Vec<u8> = line.iter().flat_map(|c| c.to_le_bytes()).collect();
    plant(&mut rom, 0x18000, &bytes);
    rom
}

/// As celulas do JIS (linha, coluna) na ordem das fontes de
/// `make_gba_rom_jis16_font` e `make_gba_rom_jis16_drawn_font`: as linhas 1 a
/// 48 sem as de `JIS16_FONT_SKIPPED`.
fn jis16_font_cells() -> impl Iterator<Item = (usize, usize)> {
    (1..=48usize)
        .filter(|row| !JIS16_FONT_SKIPPED.contains(row))
        .flat_map(|row| (1..=94usize).map(move |col| (row, col)))
}

/// O codigo Shift-JIS da celula (linha, coluna) do JIS, pelos bytes JIS (0x20
/// + linha, 0x20 + coluna).
fn sjis_of(row: usize, col: usize) -> [u8; 2] {
    let (j1, j2) = (0x20 + row, 0x20 + col);
    let lead = ((j1 + 1) >> 1) + 0x70;
    let lead = if lead > 0x9F { lead + 0x40 } else { lead };
    let trail = match j1 % 2 {
        1 if j2 + 0x1F >= 0x7F => j2 + 0x20,
        1 => j2 + 0x1F,
        _ => j2 + 0x7E,
    };
    [lead as u8, trail as u8]
}

/// Onde comecam as duas fontes de `make_gba_rom_jis16_drawn_font` e quantos
/// bytes tem cada desenho dela (o codigo Shift-JIS e os pontos).
pub const JIS16_DRAWN_FONTS: [(usize, usize); 2] = [(0x20000, 22), (0x40000, 10)];

/// O `make_gba_rom_jis16` em 512 KiB com DUAS fontes inventadas, desenhadas,
/// na ordem da fonte do jogo medido (`jis16_font_cells`, cada desenho com o
/// codigo Shift-JIS dele na frente), como as que o jogo medido em 18/10/2026
/// guarda: em `JIS16_DRAWN_FONTS[0]`, desenhos de 16x10 pontos, 2 bytes por
/// linha e o bit mais alto a esquerda; em `[1]`, de 8x8, 1 byte por linha e
/// o bit mais BAIXO a esquerda. As letras e sinais de largura inteira sao os
/// da fonte do app (`images::font`), a partir da coluna 1; o resto do que a
/// tabela padrao tem e um quadrado; codigo vazio na tabela tem desenho vazio.
pub fn make_gba_rom_jis16_drawn_font() -> Vec<u8> {
    let mut rom = make_gba_rom_jis16();
    rom.resize(512 * 1024, 0);
    for (n, (at, stride)) in JIS16_DRAWN_FONTS.into_iter().enumerate() {
        let (row_bytes, msb) = if n == 0 { (2, true) } else { (1, false) };
        let rows = (stride - 2) / row_bytes;
        let mut font = Vec::new();
        for (row, col) in jis16_font_cells() {
            let standard = ((row - 1) * 94 + col - 1) as u16;
            let ink: Vec<u32> = match crate::kuten::decode(standard) {
                None => Vec::new(),
                Some(c) => {
                    let ascii = char::from_u32((c as u32).wrapping_sub(0xFEE0))
                        .filter(|_| ('\u{FF01}'..='\u{FF5E}').contains(&c));
                    match ascii.and_then(crate::images::font::glyph) {
                        // A partir da linha da maiuscula, uma coluna pra dentro.
                        Some(glyph) => (0..rows)
                            .map(|y| u32::from(glyph.rows.get(y + 2).copied().unwrap_or(0)) << 1)
                            .collect(),
                        None => (0..rows - 1)
                            .map(|y| if y == 0 || y == rows - 2 { 0x7E } else { 0x42 })
                            .collect(),
                    }
                }
            };
            let mut glyph = vec![0u8; rows * row_bytes];
            for (y, bits) in ink.iter().enumerate() {
                for x in (0..row_bytes * 8).filter(|x| bits >> x & 1 == 1) {
                    glyph[y * row_bytes + x / 8] |=
                        if msb { 0x80 >> (x % 8) } else { 1 << (x % 8) };
                }
            }
            font.extend(sjis_of(row, col));
            font.extend(glyph);
        }
        plant(&mut rom, at, &font);
    }
    rom
}

/// Onde comeca o banco de mensagens de `make_gba_rom_jis16_bank`.
pub const JIS16_BANK_AT: usize = 0x8000;
/// Os pares do diretorio que tem bloco (os outros ficam vazios, como no
/// jogo), na ordem das salas: a sala `r` guarda as mensagens `20 * r` a
/// `20 * r + 19` de `jis16_message`.
pub const JIS16_BANK_PAIRS: [usize; 6] = [0, 1, 2, 4, 5, 7];
/// O preenchimento (0xFF) de `make_gba_rom_jis16_bank`: o espaco livre.
pub const JIS16_BANK_PADDING: std::ops::Range<usize> = 0x20000..0x38000;

/// GBA de 256 KiB com as 120 mensagens de `jis16_message` num BANCO em
/// blocos (`adapters::bank`), a estrutura do roteiro do jogo medido: em
/// `JIS16_BANK_AT`, um u32 com 16 e 16 offsets u32 relativos ao banco (zero
/// nos pares vazios), aos pares (tabela, texto); cada tabela tem um offset
/// u16 por mensagem (relativo ao bloco de texto), mais um 0 e um 0xFFFF no
/// fim; cada bloco de texto abre com 0xFFFF e o numero da sala, e cada
/// mensagem fecha com 0xFF03 0xFF00 0xFFFF. Com `padding`, 96 KiB de 0xFF em
/// `JIS16_BANK_PADDING`, e dado depois.
pub fn make_gba_rom_jis16_bank(padding: bool) -> Vec<u8> {
    jis16_bank(padding, false, false)
}

/// O banco de `make_gba_rom_jis16_bank(false)` (sem espaco livre nenhum) com
/// as salas 3, 4 e 5 repetindo as mensagens das salas 0, 1 e 2, como a mesma
/// sala de um jogo em capitulos diferentes: metade do texto esta guardada
/// duas vezes.
pub fn make_gba_rom_jis16_bank_repeated() -> Vec<u8> {
    jis16_bank(false, false, true)
}

/// A terceira linha das mensagens compridas de
/// `make_gba_rom_jis16_bank_scrolling`.
pub const JIS16_THIRD_LINE: &str = "それから ひとやすみ する。";

/// A mensagem `i` de `make_gba_rom_jis16_bank_scrolling`, como a extracao a
/// le: a de `jis16_message` com um espaco abrindo a linha de baixo e, uma em
/// cada dez, uma terceira linha.
pub fn jis16_scrolling_message(i: usize) -> String {
    let third = if i % JIS16_PREDICATES.len() == 9 {
        format!("[FF01] {JIS16_THIRD_LINE}")
    } else {
        String::new()
    };
    jis16_message(i).replace("[FF01]", "[FF01] ") + &third
}

/// O banco de `make_gba_rom_jis16_bank(true)` num jogo cuja caixa de texto
/// ROLA, como a do jogo medido: a linha de baixo abre com um espaco (o texto
/// fica alinhado depois do losango) e uma mensagem em cada dez tem tres
/// linhas emendadas pela quebra — mais do que a caixa mostra de uma vez.
pub fn make_gba_rom_jis16_bank_scrolling() -> Vec<u8> {
    jis16_bank(true, true, false)
}

fn jis16_bank(padding: bool, scrolls: bool, repeated: bool) -> Vec<u8> {
    let mut rom = make_gba_rom("JISBANK");
    rom.resize(256 * 1024, 0);
    let units = |text: &str| -> Vec<u16> {
        text.chars()
            .map(|c| match c {
                ' ' => JIS16_SPACE,
                c => crate::kuten::encode(c).expect("caractere da fixture existe no JIS"),
            })
            .collect()
    };
    let per_room = 20;
    let dir_entries = 16usize;
    let mut blocks: Vec<u8> = Vec::new();
    let mut dir = vec![0u32; dir_entries];
    let first = (4 + 4 * dir_entries).next_multiple_of(8);
    for (room, &pair) in JIS16_BANK_PAIRS.iter().enumerate() {
        let mut text: Vec<u16> = vec![0xFFFF, room as u16];
        let mut table: Vec<u16> = Vec::new();
        for i in room * per_room..(room + 1) * per_room {
            let i = if repeated { i % (3 * per_room) } else { i };
            table.push(2 * text.len() as u16);
            let (s, p) = (i / JIS16_PREDICATES.len(), i % JIS16_PREDICATES.len());
            match jis16_pattern(i) {
                1 => text.extend([0xFF08, 0x0003]),
                2 => text.push(0x0005 + p as u16),
                _ => {}
            }
            text.push(crate::kuten::encode('◆').expect("losango"));
            text.extend(units(JIS16_SUBJECTS[s]));
            if jis16_pattern(i) == 3 {
                text.extend([0xFF23, 0x0001]);
            }
            text.push(0xFF01);
            if scrolls {
                text.push(JIS16_SPACE);
            }
            text.extend(units(JIS16_PREDICATES[p]));
            if scrolls && p == 9 {
                text.extend([0xFF01, JIS16_SPACE]);
                text.extend(units(JIS16_THIRD_LINE));
            }
            text.extend([0xFF03, 0xFF00, 0xFFFF]);
        }
        table.extend([0x0000, 0xFFFF]);
        if text.len() % 2 == 1 {
            text.push(0xFFFF);
        }
        dir[2 * pair] = (first + blocks.len()) as u32;
        blocks.extend(table.iter().flat_map(|c| c.to_le_bytes()));
        dir[2 * pair + 1] = (first + blocks.len()) as u32;
        blocks.extend(text.iter().flat_map(|c| c.to_le_bytes()));
    }
    let mut bank = (dir_entries as u32).to_le_bytes().to_vec();
    bank.extend(dir.iter().flat_map(|v| v.to_le_bytes()));
    bank.resize(first, 0);
    bank.extend(blocks);
    plant(&mut rom, JIS16_BANK_AT, &bank);
    if padding {
        rom[JIS16_BANK_PADDING].fill(0xFF);
        let tail = make_random(rom.len() - JIS16_BANK_PADDING.end, 23);
        plant(&mut rom, JIS16_BANK_PADDING.end, &tail);
    }
    rom
}

/// Onde comeca o diretorio misto de `make_gba_rom_jis16_mixed`.
pub const JIS16_MIXED_AT: usize = 0x8000;
/// As entradas do diretorio misto com a tabela de uma sala (o texto e a
/// entrada seguinte); as outras apontam pra dado.
pub const JIS16_MIXED_ROOMS: [usize; 2] = [1, 5];
/// O preenchimento (0xFF) de `make_gba_rom_jis16_mixed`: o espaco livre.
pub const JIS16_MIXED_PADDING: std::ops::Range<usize> = 0x20000..0x38000;

/// GBA de 256 KiB com um diretorio MISTO, como os do jogo medido em
/// 16/10/2026 (descricoes de itens, dicas): em `JIS16_MIXED_AT`, um u32 com 8
/// e 8 offsets u32 relativos ao diretorio — dado, tabela, texto, dado, dado,
/// tabela, texto, dado. A sala `r` guarda as mensagens `60 * r` a `60 * r +
/// 59` de `jis16_message`: tabela com um offset u16 por mensagem (o primeiro
/// e 4) e bloco de texto que abre com 0xFFFF e a quantidade de mensagens. O
/// dado e ruido; 96 KiB de 0xFF em `JIS16_MIXED_PADDING`.
pub fn make_gba_rom_jis16_mixed() -> Vec<u8> {
    let mut rom = make_gba_rom("JISMIXED");
    rom.resize(256 * 1024, 0);
    let units = |text: &str| -> Vec<u16> {
        text.chars()
            .map(|c| match c {
                ' ' => JIS16_SPACE,
                c => crate::kuten::encode(c).expect("caractere da fixture existe no JIS"),
            })
            .collect()
    };
    let per_room = 60;
    let entries = 8usize;
    let first = (4 + 4 * entries).next_multiple_of(8);
    let mut blocks: Vec<u8> = Vec::new();
    let mut dir = vec![0u32; entries];
    let (mut i, mut room) = (0, 0);
    while i < entries {
        dir[i] = (first + blocks.len()) as u32;
        if !JIS16_MIXED_ROOMS.contains(&i) {
            blocks.extend(make_random(0x100, 0x51 + i as u64));
            i += 1;
            continue;
        }
        let mut text: Vec<u16> = vec![0xFFFF, per_room as u16];
        let mut table: Vec<u16> = Vec::new();
        for k in room * per_room..(room + 1) * per_room {
            table.push(2 * text.len() as u16);
            let (s, p) = (k / JIS16_PREDICATES.len(), k % JIS16_PREDICATES.len());
            match jis16_pattern(k) {
                1 => text.extend([0xFF08, 0x0003]),
                2 => text.push(0x0005 + p as u16),
                _ => {}
            }
            text.push(crate::kuten::encode('◆').expect("losango"));
            text.extend(units(JIS16_SUBJECTS[s]));
            if jis16_pattern(k) == 3 {
                text.extend([0xFF23, 0x0001]);
            }
            text.push(0xFF01);
            text.extend(units(JIS16_PREDICATES[p]));
            text.extend([0xFF03, 0xFF00, 0xFFFF]);
        }
        if text.len() % 2 == 1 {
            text.push(0xFFFF);
        }
        blocks.extend(table.iter().flat_map(|c| c.to_le_bytes()));
        dir[i + 1] = (first + blocks.len()) as u32;
        blocks.extend(text.iter().flat_map(|c| c.to_le_bytes()));
        (i, room) = (i + 2, room + 1);
    }
    let mut bank = (entries as u32).to_le_bytes().to_vec();
    bank.extend(dir.iter().flat_map(|v| v.to_le_bytes()));
    bank.resize(first, 0);
    bank.extend(blocks);
    plant(&mut rom, JIS16_MIXED_AT, &bank);
    rom[JIS16_MIXED_PADDING].fill(0xFF);
    let tail = make_random(rom.len() - JIS16_MIXED_PADDING.end, 29);
    plant(&mut rom, JIS16_MIXED_PADDING.end, &tail);
    rom
}

/// Onde comeca o arquivo de `make_gba_rom_jis16_archive`.
pub const JIS16_ARCHIVE_AT: usize = 0x8000;
/// A entrada do arquivo que e a sala com a tabela embutida (as outras sao
/// dado).
pub const JIS16_ARCHIVE_ROOM: usize = 2;
/// O preenchimento (0xFF) de `make_gba_rom_jis16_archive`: o espaco livre.
pub const JIS16_ARCHIVE_PADDING: std::ops::Range<usize> = 0x20000..0x38000;
/// A sala embutida solta de `make_gba_rom_jis16_archive` e o ponteiro de ROM
/// que leva a ela.
pub const JIS16_LOOSE_AT: usize = 0x3C000;
pub const JIS16_LOOSE_POINTER: usize = 0x7000;
/// Os titulos da sala solta.
pub const JIS16_TITLES: [&str; 5] = [
    "リンゴ",
    "パン",
    "くすり",
    "ランプ",
    "ふしぎな まほうの ランプ",
];

/// A mensagem `i` (0 a 119) da sala embutida do arquivo, como a extracao a
/// le: sujeito e predicado, sem o comando do nome que abre a mensagem no
/// cartucho.
pub fn jis16_battle_message(i: usize) -> String {
    format!(
        "{} {}",
        JIS16_SUBJECTS[i / JIS16_PREDICATES.len()],
        JIS16_PREDICATES[i % JIS16_PREDICATES.len()]
    )
}

/// Uma sala com a tabela embutida, como as do jogo medido: etiqueta, versao
/// 1, quantidade, um offset u16 por mensagem relativo ao comeco do bloco, as
/// mensagens (o espaco entre palavras e o codigo 0, o espaco de largura
/// inteira do JIS) e a marca de fim, "~" e a etiqueta.
fn inline_block(tag: &[u8; 4], messages: &[Vec<u16>]) -> Vec<u8> {
    let first = (12 + 2 * messages.len()).next_multiple_of(4);
    let mut table = Vec::new();
    let mut text: Vec<u8> = Vec::new();
    for message in messages {
        table.extend(((first + text.len()) as u16).to_le_bytes());
        text.extend(message.iter().flat_map(|c| c.to_le_bytes()));
        text.extend(0xFFFFu16.to_le_bytes());
    }
    let mut block = tag.to_vec();
    block.extend(1u32.to_le_bytes());
    block.extend((messages.len() as u32).to_le_bytes());
    block.extend(table);
    block.resize(first, 0);
    block.extend(text);
    block.extend([b'~', tag[0], tag[1], tag[2]]);
    block.resize(block.len().next_multiple_of(4), 0);
    block
}

/// GBA de 256 KiB com as duas formas de sala com a tabela embutida do jogo
/// medido em 16/10/2026: em `JIS16_ARCHIVE_AT`, um ARQUIVO (etiqueta, u32 com
/// 8 e 8 pares u32 (offset, tamanho) relativos a ele) cuja entrada
/// `JIS16_ARCHIVE_ROOM` e a sala das 120 mensagens de `jis16_battle_message`,
/// cada uma aberta pelo comando `FF11` (o nome de quem age); as outras
/// entradas sao ruido. Em `JIS16_LOOSE_AT`, uma sala solta com os
/// `JIS16_TITLES`, achada pelo ponteiro de ROM em `JIS16_LOOSE_POINTER`. 96
/// KiB de 0xFF em `JIS16_ARCHIVE_PADDING`.
pub fn make_gba_rom_jis16_archive() -> Vec<u8> {
    let mut rom = make_gba_rom("JISARCH");
    rom.resize(256 * 1024, 0);
    let units = |text: &str| -> Vec<u16> {
        text.chars()
            .map(|c| match c {
                ' ' => 0,
                c => crate::kuten::encode(c).expect("caractere da fixture existe no JIS"),
            })
            .collect()
    };
    let messages: Vec<Vec<u16>> = (0..JIS16_SUBJECTS.len() * JIS16_PREDICATES.len())
        .map(|i| {
            let mut message = vec![0xFF11];
            message.extend(units(&jis16_battle_message(i)));
            message
        })
        .collect();
    let entries = 8usize;
    let mut blocks: Vec<u8> = Vec::new();
    let mut pairs = Vec::new();
    for i in 0..entries {
        let block = if i == JIS16_ARCHIVE_ROOM {
            inline_block(b"msg ", &messages)
        } else {
            make_random(0x100, 0x61 + i as u64)
        };
        pairs.push(((8 + 8 * entries + blocks.len()) as u32, block.len() as u32));
        blocks.extend(block);
    }
    let mut archive = b"pak ".to_vec();
    archive.extend((entries as u32).to_le_bytes());
    for (offset, size) in pairs {
        archive.extend(offset.to_le_bytes());
        archive.extend(size.to_le_bytes());
    }
    archive.extend(blocks);
    plant(&mut rom, JIS16_ARCHIVE_AT, &archive);
    rom[JIS16_ARCHIVE_PADDING].fill(0xFF);
    let tail = make_random(rom.len() - JIS16_ARCHIVE_PADDING.end, 31);
    plant(&mut rom, JIS16_ARCHIVE_PADDING.end, &tail);
    let titles: Vec<Vec<u16>> = JIS16_TITLES.iter().map(|title| units(title)).collect();
    plant(&mut rom, JIS16_LOOSE_AT, &inline_block(b"ttl ", &titles));
    plant(
        &mut rom,
        JIS16_LOOSE_POINTER,
        &(0x0800_0000 + JIS16_LOOSE_AT as u32).to_le_bytes(),
    );
    rom
}

/// As frases do banco de `make_profile_bank`, na ordem do banco. A quarta
/// repete a primeira; `[E6]` quebra a linha e `[E3:10]` e um codigo com um
/// byte de parametro.
pub const PROFILE_LINES: [&str; 5] = [
    "Bat",
    "[E3:10]The castle is quiet tonight.[E6]Too quiet.",
    "Restores 100 HP.",
    "Bat",
    "Skeleton",
];
/// Onde o "modulo" do banco e carregado na memoria, e onde ele comeca no
/// arquivo (`make_profile_bank`).
pub const PROFILE_MEMORY: u32 = 0x0210_0000;
pub const PROFILE_ORIGIN: usize = 0x1000;
/// Faixa do banco de frases e a posicao da tabela de ponteiros.
pub const PROFILE_BANK: std::ops::Range<usize> = 0x2000..0x2100;
pub const PROFILE_TABLE: usize = 0x1100;

/// Uma letra do banco do `make_profile_bank`: ASCII com 0x20 a menos.
pub fn profile_encode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(code) = rest.strip_prefix('[') {
            let close = code.find(']').expect("codigo fechado");
            for part in code[..close].split(':') {
                for i in (0..part.len()).step_by(2) {
                    out.push(u8::from_str_radix(&part[i..i + 2], 16).expect("hex"));
                }
            }
            rest = &code[close + 1..];
        } else {
            let c = rest.chars().next().expect("letra");
            out.push(c as u8 - 0x20);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// GBA de 16 KiB com um banco de frases no formato que so um perfil descreve
/// (`profile`): cada frase abre com `01 00`, as letras sao ASCII com 0x20 a
/// menos, `EA` fecha, alinhada em 4. Um "modulo" carregado em
/// `PROFILE_MEMORY` comeca em `PROFILE_ORIGIN`; a tabela em `PROFILE_TABLE`
/// guarda o endereco de memoria de cada frase de `PROFILE_LINES` e, por
/// ultimo, o de um dado que tambem abre com `01 00` mas nao e texto.
pub fn make_profile_bank() -> Vec<u8> {
    let mut rom = make_gba_rom("PERFIL");
    rom.resize(16 * 1024, 0);
    let mut at = PROFILE_BANK.start;
    let put = |rom: &mut Vec<u8>, at: &mut usize, body: &[u8], slot: usize| {
        let start = *at;
        rom[start..start + 2].copy_from_slice(&[1, 0]);
        rom[start + 2..start + 2 + body.len()].copy_from_slice(body);
        rom[start + 2 + body.len()] = 0xEA;
        *at = (start + 3 + body.len()).next_multiple_of(4);
        let address = PROFILE_MEMORY + (start - PROFILE_ORIGIN) as u32;
        let k = PROFILE_TABLE + 4 * slot;
        rom[k..k + 4].copy_from_slice(&address.to_le_bytes());
    };
    for (slot, line) in PROFILE_LINES.iter().enumerate() {
        put(&mut rom, &mut at, &profile_encode(line), slot);
    }
    put(
        &mut rom,
        &mut at,
        &[0xF1, 0xFF, 0x13, 0xFF, 0x02, 0x9C, 0xC0],
        PROFILE_LINES.len(),
    );
    rom
}

/// A tabela de letras do `make_profile_bank`, no formato `.tbl`.
pub fn profile_table() -> String {
    (0..0x5Fu8)
        .map(|b| format!("{b:02X}={}\n", (b + 0x20) as char))
        .collect()
}

/// O perfil do `make_profile_bank`.
pub fn profile_toml() -> String {
    format!(
        "[[texto]]\nnome = \"modulo\"\ntabela = \"letras\"\ninicio = {}\nfim = {}\n\
         cabecalho = \"0100\"\nterminador = \"EA\"\nalinhamento = 4\nquebra = \"E6\"\n\
         parametros = {{ E3 = 1 }}\n\n[texto.ponteiros]\nmemoria = {}\norigem = {}\n\
         procurar = [[{}, {}]]\n",
        PROFILE_BANK.start,
        PROFILE_BANK.end,
        PROFILE_MEMORY,
        PROFILE_ORIGIN,
        PROFILE_TABLE,
        PROFILE_TABLE + 64
    )
}

/// O banco de `PROFILE_LINES` como um modulo carregado em `memory`: tabela de
/// ponteiros no comeco (o endereco de memoria de cada frase e, por ultimo, o
/// de um dado que abre igual e nao e texto), frases a partir de 0x40, no
/// formato do `make_profile_bank`.
fn profile_module(memory: u32) -> Vec<u8> {
    let mut module = vec![0u8; 0x200];
    let mut at = 0x40;
    let bodies = PROFILE_LINES
        .iter()
        .map(|line| profile_encode(line))
        .chain([vec![0xF1, 0xFF, 0x13, 0xFF, 0x02, 0x9C, 0xC0]]);
    for (slot, body) in bodies.enumerate() {
        module[at..at + 2].copy_from_slice(&[1, 0]);
        module[at + 2..at + 2 + body.len()].copy_from_slice(&body);
        module[at + 2 + body.len()] = 0xEA;
        let address = memory + at as u32;
        module[slot * 4..slot * 4 + 4].copy_from_slice(&address.to_le_bytes());
        at = (at + 3 + body.len()).next_multiple_of(4);
    }
    module
}

/// Onde o overlay do `make_nds_profile_bank` e carregado.
pub const NDS_OVERLAY_MEMORY: u32 = 0x0219_0000;
/// Onde o `make_gba_profile_bank` poe o banco no cartucho.
pub const GBA_PROFILE_AT: usize = 0x2000;

/// DS com o banco do perfil num overlay (`ov0.bin`, file id 0) que a tabela
/// de overlays do ARM9 (header 0x50) diz carregar em `NDS_OVERLAY_MEMORY`.
pub fn make_nds_profile_bank() -> Vec<u8> {
    use crate::adapter::GameAdapter;
    use crate::adapters::nds::{crc16, NdsAdapter};

    let module = profile_module(NDS_OVERLAY_MEMORY);
    let mut y9 = vec![0u8; 32];
    y9[4..8].copy_from_slice(&NDS_OVERLAY_MEMORY.to_le_bytes());
    y9[8..12].copy_from_slice(&(module.len() as u32).to_le_bytes());
    let mut rom = build_nds(&[("ov0.bin", module), ("y9.bin", y9)]);
    let table = NdsAdapter
        .list_resources(&rom)
        .expect("filesystem sintetico")
        .into_iter()
        .find(|f| f.path == "y9.bin")
        .expect("y9.bin")
        .offset as u32;
    rom[0x50..0x54].copy_from_slice(&table.to_le_bytes());
    rom[0x54..0x58].copy_from_slice(&32u32.to_le_bytes());
    let crc = crc16(&rom[..0x15E]);
    rom[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());
    rom
}

/// GBA de 16 KiB com o banco do perfil em `GBA_PROFILE_AT`, os ponteiros com
/// o endereco do cartucho (`0x08000000` + posicao).
pub fn make_gba_profile_bank() -> Vec<u8> {
    let mut rom = make_gba_rom("PERFIL GBA");
    rom.resize(16 * 1024, 0);
    let module = profile_module(0x0800_0000 + GBA_PROFILE_AT as u32);
    plant(&mut rom, GBA_PROFILE_AT, &module);
    rom
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

/// Onde moram as partes de `make_gba_rom_with_tilemap`: o bloco de quadros, o
/// mapa do painel (fora do multiplo de 64, como no jogo medido) e o de outra
/// tela.
pub const TILEMAP_TILES_AT: usize = 0x4000;
pub const TILEMAP_MAP_AT: usize = 0xA03C;
pub const TILEMAP_OTHER_MAP_AT: usize = 0xC024;
/// A celula do painel (linha, coluna) onde a lista comeca, e a distancia em
/// pixels de uma linha da lista a seguinte.
pub const TILEMAP_LIST_CELL: (usize, usize) = (2, 3);
pub const TILEMAP_PITCH: usize = 13;
/// Celula do painel, fora da lista, que repete o 2o quadro da metade de baixo
/// da linha 2.
pub const TILEMAP_REPEAT_CELL: (usize, usize) = (18, 20);

/// GBA com um painel montado por mapa de quadros, como o painel de cores do
/// RPG japones medido: uma lista de 9 linhas de "letras" (riscos de 5x7, cor 5
/// no fundo 1) a 13 pixels uma da outra, guardada em tiras de 2 linhas de
/// quadros lado a lado — na ordem guardada, a linha que cai entre duas tiras
/// sai partida. Um mapa cru (32 entradas por linha) monta a tela: moldura com
/// os cantos e a borda de baixo espelhados, fundo, a lista, e uma celula do
/// painel que repete um quadro da linha 2. Outro mapa (outra tela) mostra
/// duas tiras e o comeco da linha 2. O bloco de quadros e cru, com zeros em
/// volta.
pub fn make_gba_rom_with_tilemap() -> Vec<u8> {
    const COLS: usize = 32;
    let mut rom = make_gba_rom("TILEMAP");
    rom.resize(0x10000, 0);
    let mut block = vec![[0u8; 64]; COLS * 5];
    // 0: vazio (cor 0); 1: fundo; 2: borda de cima; 3: borda da esquerda;
    // 4: canto de cima a esquerda.
    block[1] = [1; 64];
    for y in 0..8 {
        for x in 0..8 {
            let line = |a: usize, b: usize| match a {
                0..=2 => 0,
                3 => 2,
                _ => match b {
                    0..=2 => 0,
                    3 => 2,
                    _ => 1,
                },
            };
            block[2][y * 8 + x] = match y {
                0..=2 => 0,
                3 => 2,
                _ => 1,
            };
            block[3][y * 8 + x] = match x {
                0..=2 => 0,
                3 => 2,
                _ => 1,
            };
            block[4][y * 8 + x] = line(y.min(x), y.max(x));
        }
    }
    // A lista: 8 quadros de largura, 16 linhas de quadros.
    const W: usize = 64;
    let mut list = vec![1u8; W * 128];
    for line in 0..9 {
        let top = TILEMAP_PITCH * line + 3;
        for letter in 0..6 + line % 3 {
            let left = 2 + 6 * letter;
            for dy in 0..7 {
                for dx in 0..5 {
                    let ink = match (line + letter) % 4 {
                        0 => dx == 0 || dx == 4 || dy == 3,
                        1 => dy == 0 || dy == 6 || dx == 0,
                        2 => dx == dy.min(4) || dx + dy == 6,
                        _ => {
                            ((dy == 0 || dy == 6) && (1..4).contains(&dx))
                                || ((dx == 0 || dx == 4) && (1..6).contains(&dy))
                        }
                    };
                    if ink {
                        list[(top + dy) * W + left + dx] = 5;
                    }
                }
            }
        }
    }
    // Tira k = linhas 2k e 2k+1 da lista, guardada nas linhas 1 + 2*(k/4) do
    // bloco, a partir da coluna 8*(k%4).
    let stored = |row: usize, col: usize| {
        let k = row / 2;
        (1 + 2 * (k / 4) + row % 2) * COLS + 8 * (k % 4) + col
    };
    for row in 0..16 {
        for col in 0..8 {
            let tile = &mut block[stored(row, col)];
            for y in 0..8 {
                for x in 0..8 {
                    tile[y * 8 + x] = list[(row * 8 + y) * W + col * 8 + x];
                }
            }
        }
    }
    for (t, px) in block.iter().enumerate() {
        for (i, pair) in px.chunks(2).enumerate() {
            rom[TILEMAP_TILES_AT + t * 32 + i] = pair[0] | pair[1] << 4;
        }
    }
    // Moldura de (r0, c0) a (r1, c1), fundo dentro.
    let frame = |map: &mut [u16], (r0, c0): (usize, usize), (r1, c1): (usize, usize)| {
        for r in r0..=r1 {
            for c in c0..=c1 {
                let (h, v) = (u16::from(c == c1) << 10, u16::from(r == r1) << 11);
                map[r * COLS + c] = match (r == r0 || r == r1, c == c0 || c == c1) {
                    (true, true) => 4 | h | v,
                    (true, false) => 2 | v,
                    (false, true) => 3 | h,
                    (false, false) => 1,
                };
            }
        }
    };
    let strip = |map: &mut [u16], k: usize, (r, c): (usize, usize)| {
        for row in 0..2 {
            for col in 0..8 {
                map[(r + row) * COLS + c + col] = stored(2 * k + row, col) as u16;
            }
        }
    };
    let mut map = vec![0u16; COLS * COLS];
    frame(&mut map, (0, 1), (19, 28));
    let (lr, lc) = TILEMAP_LIST_CELL;
    for k in 0..8 {
        strip(&mut map, k, (lr + 2 * k, lc));
    }
    map[TILEMAP_REPEAT_CELL.0 * COLS + TILEMAP_REPEAT_CELL.1] = stored(4, 1) as u16;
    // A outra tela: as tiras 0 e 4 (cortes entre linhas da lista) e o comeco
    // da linha 2 (3 quadros das linhas 3 e 4 da lista).
    let mut other = vec![0u16; COLS * COLS];
    frame(&mut other, (0, 4), (9, 25));
    strip(&mut other, 0, (2, 6));
    strip(&mut other, 4, (2, 15));
    for row in 0..2 {
        for col in 0..3 {
            other[(5 + row) * COLS + 6 + col] = stored(3 + row, col) as u16;
        }
    }
    for (at, map) in [(TILEMAP_MAP_AT, map), (TILEMAP_OTHER_MAP_AT, other)] {
        for (i, entry) in map.iter().enumerate() {
            rom[at + 2 * i..at + 2 * i + 2].copy_from_slice(&entry.to_le_bytes());
        }
    }
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
