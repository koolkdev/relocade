//! Standard VGA registers, access protocols and CPU read latches.
//! A card owns wiring and display effects. Its callbacks receive raw index/data
//! bytes before deciding whether to apply the standard controller masks.
mod dac;
mod memory;
mod state;
#[cfg(test)]
mod tests;

use crate::{
    registers::{attribute, setup::Setup},
    Registers,
};
pub use dac::DacState;
pub(crate) use memory::MemoryWrite;
pub use state::VgaCoreState;

pub struct VgaCore {
    pub(crate) registers: Registers,
    pub(crate) setup: Setup,
    latches: [u8; 4],
    memory_controls: memory::MemoryControls,
}

impl Default for VgaCore {
    fn default() -> Self {
        let registers = Registers::default();
        Self {
            memory_controls: memory::MemoryControls::new(&registers),
            registers,
            setup: Setup::default(),
            latches: [0; 4],
        }
    }
}

impl VgaCore {
    pub fn registers(&self) -> &Registers {
        &self.registers
    }
    pub fn core_state(&self) -> VgaCoreState {
        VgaCoreState::capture(self)
    }
    pub fn dac_state(&self) -> DacState {
        DacState {
            dac: self.registers.dac.clone(),
            color_select: self.registers.attribute.values[attribute::COLOR_SELECT as usize],
        }
    }
    pub(crate) fn restore_dac(&mut self, state: DacState) {
        self.registers.dac = state.dac;
        self.registers.attribute.values[attribute::COLOR_SELECT as usize] = state.color_select;
    }
}
