//! Selected-card firmware exercised against the connected ET4000 hardware.
use relocade_bios::video::VideoFirmware;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{IoBus, MachineConfig, Memory};
use relocade_vga::{Et4000Ax, FramebufferAccess, FramebufferGeometry};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{cell::RefCell, rc::Rc};

#[path = "../../machine/tests/support/execution.rs"]
mod execution;
#[path = "et4000/tables.rs"]
mod tables;
#[path = "et4000/vbe.rs"]
mod vbe;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Firmware {
    memory: Memory,
    io: IoBus,
    card: Rc<RefCell<Et4000Ax>>,
    firmware: VideoFirmware,
}

impl Firmware {
    fn new() -> Self {
        let map = PhysicalMemoryMap::new([
            (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
            (relocade_vga::MEMORY_RANGE, PhysicalMapping::MMIO),
            (
                0xf0000..=0xfffff,
                PhysicalMapping::Rom {
                    backing_offset: 0xf0000,
                },
            ),
        ])
        .unwrap();
        let memory = Memory::new(MachineConfig::new(0x100000, map).unwrap()).unwrap();
        let card = Rc::new(RefCell::new(Et4000Ax::with_clock(
            ManualClock::default().clock(),
        )));
        let io = IoBus::new(card.clone(), |io| Et4000Ax::configure_io(io, &card));
        let firmware = VideoFirmware::Et4000Ax(card.clone());
        relocade_bios::initialize(
            &mut memory.view().with_io(&io),
            &io,
            relocade_bios::Configuration {
                conventional_memory_kib: 640,
                math_coprocessor: true,
                pointing_device: false,
            },
            &firmware,
        );
        Self {
            memory,
            io,
            card,
            firmware,
        }
    }

    fn set_mode(&self, request: u8) -> Option<u8> {
        self.firmware
            .set_mode(&mut self.memory.view().with_io(&self.io), &self.io, request)
    }

    fn geometry(&self) -> FramebufferGeometry {
        self.card.borrow_mut().framebuffer().geometry().unwrap()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn indexed_modes_clear_or_preserve_banked_memory_and_return_to_text() {
    let machine = Firmware::new();
    let font16 = machine.memory.view().read_u32(0x43 * 4).unwrap();
    assert_eq!(
        machine.geometry(),
        FramebufferGeometry::Text {
            columns: 80,
            rows: 25
        }
    );
    for (number, width, height) in [(0x2e, 640, 480), (0x30, 800, 600), (0x38, 1024, 768)] {
        assert!(machine.set_mode(0x13).is_some());
        assert_ne!(machine.memory.view().read_u32(0x43 * 4).unwrap(), font16);
        assert!(machine.set_mode(number).is_some());
        assert_eq!(machine.memory.view().read_u32(0x43 * 4).unwrap(), font16);
        assert_eq!(machine.memory.view().read_u16(0x485).unwrap(), 16);
        assert_eq!(
            machine.geometry(),
            FramebufferGeometry::Graphics { width, height }
        );
        assert_eq!(machine.memory.view().read_u8(0x449).unwrap(), number);
        // Bank one is beyond the first 64 KiB; it must survive preserve-mode entry.
        machine.io.write_port(0x3cd, 1, 0x11);
        machine.io.write_mmio(0xa0000, 1, 0x5a);
        assert!(machine.set_mode(number | 0x80).is_some());
        assert_eq!(machine.io.read_port(0x3cd, 1), 0);
        machine.io.write_port(0x3cd, 1, 0x11);
        assert_eq!(machine.io.read_mmio(0xa0000, 1), 0x5a);
        assert!(machine.set_mode(number).is_some());
        machine.io.write_port(0x3cd, 1, 0x11);
        assert_eq!(machine.io.read_mmio(0xa0000, 1), 0);
    }
    assert!(machine.set_mode(3).is_some());
    assert_eq!(
        machine.geometry(),
        FramebufferGeometry::Text {
            columns: 80,
            rows: 25
        }
    );
    machine.io.write_mmio(0xb8000, 2, 0x0741);
    assert_eq!(
        machine
            .card
            .borrow_mut()
            .framebuffer()
            .text_cell(0, 0)
            .unwrap()
            .value(),
        0x0741
    );
    assert_eq!(machine.io.read_port(0x1ce, 2), 0xffff);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unsupported_modes_leave_the_active_card_and_bios_state_unchanged() {
    let machine = Firmware::new();
    assert!(machine.set_mode(0x38).is_some());
    machine.io.write_port(0x3cd, 1, 0x21);
    let before = machine.card.borrow().core_state();
    assert!(machine.set_mode(0x39).is_none());
    assert_eq!(machine.card.borrow().core_state(), before);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x21);
    assert_eq!(machine.memory.view().read_u8(0x449).unwrap(), 0x38);
}
