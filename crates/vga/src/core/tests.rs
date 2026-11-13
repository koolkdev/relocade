use super::VgaCore;
use crate::{
    memory::VideoMemory,
    registers::{graphics, sequencer},
    MEMORY_RANGE,
};
use relocade_machine::{Device, IoBus, IoContext};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct VgaOnly {
    core: VgaCore,
    memory: VideoMemory,
}

impl Device for VgaOnly {
    fn read_mmio(&mut self, _: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        self.core.read_memory(&self.memory, address, bytes)
    }
    fn write_mmio(&mut self, _: IoContext<'_>, address: u32, bytes: u8, value: u64) {
        self.core
            .write_memory(&mut self.memory, address, bytes, value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn vga_only_composition_uses_the_core_latches_and_access_protocols() {
    let mut card = VgaOnly::default();
    // The card receives raw indices before the core applies the VGA index mask.
    card.core.registers.sequencer.select(0xfa);
    assert_eq!(card.core.registers.sequencer.index(), sequencer::MAP_MASK);
    card.core.write_sequencer(0xff);
    card.core.registers.sequencer.select(sequencer::MEMORY_MODE);
    card.core.write_sequencer(6);
    card.core.registers.graphics.select(graphics::MISCELLANEOUS);
    card.core.write_graphics(5);
    card.core.registers.graphics.select(graphics::MODE);
    card.core.write_graphics(0);
    for (address, value) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
        card.memory.write_physical(address as u32, value);
    }
    let device = Rc::new(RefCell::new(card));
    let io = IoBus::new(device.clone(), |io| io.map_mmio([MEMORY_RANGE]));
    assert_eq!(io.read_mmio(0xa0000, 1), 0x12);
    let saved = device.borrow().core.core_state();
    assert_eq!(&saved.to_bytes()[11..15], &[0x12, 0x34, 0x56, 0x78]);
    {
        let mut card = device.borrow_mut();
        card.memory.clear_vram();
        card.core.write_graphics(1); // Write mode one copies the core's latches.
    }
    io.write_mmio(0xa0001, 1, 0xff);
    let card = device.borrow();
    let copied =
        std::array::from_fn::<_, 4, _>(|plane| card.memory.read_physical(4 + plane as u32));
    assert_eq!(copied, [0x12, 0x34, 0x56, 0x78]);
}
