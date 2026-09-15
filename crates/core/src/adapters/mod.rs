pub mod gamecube;
pub mod gba;
pub(crate) mod inplace;
pub mod nds;
pub mod nes;
pub mod rtsf;
pub mod snes;
pub mod wii;
pub mod wiiu;

use crate::adapter::GameAdapter;

/// Registry dos adapters compilados no workspace (spec: sem plugin loading
/// dinamico no MVP). Ordem: magics fortes primeiro, heuristicos por ultimo.
pub fn all() -> Vec<Box<dyn GameAdapter>> {
    vec![
        Box::new(rtsf::RtsfAdapter),
        Box::new(nds::NdsAdapter),
        Box::new(gamecube::GameCubeAdapter),
        Box::new(wii::WiiAdapter),
        Box::new(wiiu::WiiUAdapter),
        Box::new(nes::NesAdapter),
        Box::new(gba::GbaAdapter),
        Box::new(snes::SnesAdapter),
    ]
}

pub fn find(id: &str) -> Option<Box<dyn GameAdapter>> {
    all().into_iter().find(|a| a.id() == id)
}
