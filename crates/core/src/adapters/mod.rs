pub mod gba;
pub mod nes;
pub mod snes;

use crate::adapter::GameAdapter;

/// Registry dos adapters compilados no workspace (spec: sem plugin loading dinamico no MVP).
pub fn all() -> Vec<Box<dyn GameAdapter>> {
    vec![
        Box::new(nes::NesAdapter),
        Box::new(gba::GbaAdapter),
        Box::new(snes::SnesAdapter),
    ]
}
