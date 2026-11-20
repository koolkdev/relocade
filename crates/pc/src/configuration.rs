use crate::CONVENTIONAL_MEMORY_SIZE;
use relocade_dma::Dma;
use relocade_machine::{CpuConfig, MachineConfig, Runner};
use relocade_pic::Pic;
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, MemoryBackend, PhysicalMapping, PhysicalMemoryMap,
};
use std::{cell::RefCell, rc::Rc};

/// Hardware installed before guest execution, independently of host playback.
#[derive(Clone, Copy, Debug, Default)]
pub struct Configuration {
    pub sound_card: SoundCard,
    pub video_card: VideoCard,
    pub parallel_port: Option<ParallelPort>,
}

impl Configuration {
    /// Checks parallel-port address bounds, IRQ selection and the Sound Blaster
    /// IRQ conflict before connecting hardware. PC construction also calls this.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(port) = self.parallel_port {
            if port.base == 0 || port.base > u16::MAX - 2 {
                return Err(format!(
                    "parallel-port base {:#06x} must be nonzero and leave room for three registers",
                    port.base
                ));
            }
            if !matches!(port.irq, 5 | 7) {
                return Err(format!(
                    "parallel port does not support IRQ{}; choose IRQ5 or IRQ7",
                    port.irq
                ));
            }
            if port.irq == 7 && self.sound_card == SoundCard::SoundBlaster2 {
                return Err(
                    "parallel port and Sound Blaster both use IRQ7; choose IRQ5 for the parallel port"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

/// One SPP adapter, assigned the first BIOS LPT slot regardless of its address.
/// This installs the port with an unconnected connector. IRQ5 and IRQ7 are
/// supported; IRQ7 cannot be shared with the current Sound Blaster model.
/// The base must be nonzero and leave room for three consecutive registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParallelPort {
    pub base: u16,
    pub irq: u8,
}

impl Default for ParallelPort {
    fn default() -> Self {
        Self {
            base: 0x378,
            irq: 7,
        }
    }
}

/// The video card and matching firmware to initialize before guest execution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoCard {
    #[default]
    Dispi,
    Et4000Ax,
}

impl VideoCard {
    pub(crate) async fn initialize(self, runner: &Runner) -> crate::video_card::VideoCard {
        match self {
            Self::Dispi => crate::video_card::VideoCard::Dispi(
                relocade_vga::DispiAdapter::initialize(runner).await,
            ),
            Self::Et4000Ax => crate::video_card::VideoCard::Et4000Ax(
                relocade_vga::Et4000Ax::initialize(runner).await,
            ),
        }
    }
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
            Self::AdLib => Some(Box::new(relocade_adlib::Adlib::initialize(runner))),
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
        Compiler::new(ExecutionProfile::Real16)
            .with_memory_backend(MemoryBackend::PhysicalBus)
            .target(),
        CpuState::real_mode(),
    )
    .expect("fixed real-mode PC CPU")
}
