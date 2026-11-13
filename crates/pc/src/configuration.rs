use crate::CONVENTIONAL_MEMORY_SIZE;
use relocade_dma::Dma;
use relocade_machine::{CpuConfig, MachineConfig, Runner};
use relocade_pic::Pic;
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};
use std::{cell::RefCell, rc::Rc};

/// Hardware installed before guest execution, independently of host playback.
#[derive(Clone, Copy, Debug, Default)]
pub struct Configuration {
    pub sound_card: SoundCard,
}

/// The sound card to initialize. The PC speaker is available with every choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoundCard {
    None,
    #[default]
    AdLib,
    SoundBlaster2,
}

impl SoundCard {
    pub(crate) async fn initialize(
        self,
        runner: &Runner,
        dma: &Rc<RefCell<Dma>>,
        pic: &Pic,
    ) -> Option<Box<dyn crate::sound_card::SoundCard>> {
        match self {
            Self::None => None,
            Self::AdLib => Some(Box::new(relocade_adlib::Adlib::initialize(runner).await)),
            Self::SoundBlaster2 => Some(Box::new(
                relocade_sound_blaster::SoundBlaster::initialize(runner, dma.clone(), pic).await,
            )),
        }
    }
}

pub fn memory() -> MachineConfig {
    const MEMORY_SIZE: u32 = 1024 * 1024;
    const BIOS_ROM_START: u32 = 0xf0000;
    let map = PhysicalMemoryMap::new([
        (
            0..=CONVENTIONAL_MEMORY_SIZE - 1,
            PhysicalMapping::Ram { backing_offset: 0 },
        ),
        (relocade_vga::MEMORY_RANGE, PhysicalMapping::MMIO),
        (
            BIOS_ROM_START..=MEMORY_SIZE - 1,
            PhysicalMapping::Rom {
                backing_offset: BIOS_ROM_START,
            },
        ),
    ])
    .expect("fixed PC memory map");
    MachineConfig::new(u64::from(MEMORY_SIZE), map).expect("fixed PC memory map")
}

pub fn cpu() -> CpuConfig {
    CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState::real_mode(),
    )
    .expect("fixed real-mode PC CPU")
}
