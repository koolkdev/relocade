//! Only selected bytes reach MMIO; every transfer observes live routing.

use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{Event, MmioUpdate, TestModule},
};
use wasm86_x86::{ExecutionProfile, Segments};

fn mmio(frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for bytes in [8, 16] {
        for (empty, remap) in [(true, false), (false, false), (false, true)] {
            let code = code(bytes);
            let mut mask = [0; 16];
            if !empty {
                for offset in [0, 2, 7, 15] {
                    mask[offset] = 0x80;
                }
            }
            let mut image = image(&code, mask);
            image.cpu.segments = Segments::real_mode();
            image.cpu.registers.edi = 0xabcd_4011;
            image.data(0x8011, &[0x5a; 16]);
            image.data(0x9011, &[0x5a; 16]);
            let mut input = image.input();
            input.mmio_pages = vec![(4, 0x8000)];
            input.observe_mmio = true;
            if remap {
                input.mmio_updates = vec![MmioUpdate {
                    map: vec![(4 * 8, vec![1, 0, 0, 0, 0, 0x90, 0, 0])],
                    ..MmioUpdate::default()
                }];
            }
            let cpu = retired(image.cpu, code.len(), bytes);
            let mut writes = vec![];
            let mut events = vec![];
            if !empty {
                for offset in [0, 2, 7, 15].into_iter().filter(|&offset| offset < bytes) {
                    let frame = if remap && offset != 0 { 0x9000 } else { 0x8000 };
                    writes.push((frame + 0x11 + offset as u32, &DATA[offset..offset + 1]));
                    if !remap || offset == 0 {
                        events.push(Event::MmioWrite {
                            address: 0x4011 + offset as u32,
                            value: vec![DATA[offset]],
                        });
                    }
                }
            }
            let mut wanted = expected(
                &image,
                &[Step {
                    cpu,
                    ram: &writes,
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
            wanted.events.splice(0..0, events);
            let module = match frontend {
                Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
                Frontend::Interpreter => {
                    TestModule::interpreter_with_profile(ExecutionProfile::Real16)
                }
            };
            assert_eq!(
                module.observe(&input, 1),
                wanted,
                "MASKMOV width={bytes}, empty={empty}, remap={remap}"
            );
        }
    }
}

test_frontends!(masked_mmio, mmio);
