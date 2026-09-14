//! Gera fixtures sinteticas em `fixtures/generated/` para teste manual da GUI.
//! Uso: `cargo run -p romtranslate-core --example gen_fixtures`

use std::fs;
use std::path::PathBuf;

use romtranslate_core::synth;

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/generated");
    fs::create_dir_all(&dir)?;

    let files: [(&str, Vec<u8>); 5] = [
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
    ];

    for (name, bytes) in files {
        let path = dir.join(name);
        fs::write(&path, &bytes)?;
        println!("{} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}
