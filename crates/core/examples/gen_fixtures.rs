//! Gera fixtures sinteticas em `fixtures/generated/` para teste manual da GUI.
//! Uso: `cargo run -p romtranslate-core --example gen_fixtures`

use std::fs;
use std::path::PathBuf;

use romtranslate_core::synth;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/generated");
    fs::create_dir_all(&dir)?;

    let files: [(&str, Vec<u8>); 11] = [
        ("synthetic.wux", synth::make_wux_header()),
        ("synthetic.rpx", synth::make_rpx_header()),
        ("synthetic.nds", synth::make_nds_rom()),
        ("synthetic_gc.iso", synth::make_gc_disc_header()),
        ("synthetic_wii.iso", synth::make_wii_disc_header()),
        ("synthetic.gba", synth::make_gba_rom("SYNTHRPG")),
        ("synthetic.nes", synth::make_nes_rom()),
        (
            "synthetic_lorom.sfc",
            synth::make_snes_lorom("SYNTHETIC QUEST"),
        ),
        (
            "synthetic_headered.smc",
            synth::make_snes_headered("SYNTHETIC QUEST"),
        ),
        ("not_a_rom.bin", synth::make_random(4096, 2026)),
        ("synthetic.rtsf", synth::make_rtsf_fixture()),
    ];

    for (name, bytes) in files {
        let path = dir.join(name);
        fs::write(&path, &bytes)?;
        println!("{} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}
