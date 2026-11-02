#[path = "state/save.rs"]
mod save;

use super::video::video;
use relocade_dos::Dos;
use relocade_x86::{CpuState, Segment, SegmentRegister};

fn state(dos: &mut Dos, operation: u8, components: u16, segment: u16, offset: u16) -> CpuState {
    {
        let context = dos.cpu().context();
        context.cpu.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, segment),
        );
        context.cpu.set_si(0x1234);
        context.cpu.set_di(0x5678);
        context.cpu.set_bp(0x9abc);
    }
    let result = video(
        dos,
        0x1c00 | u16::from(operation),
        offset,
        components,
        0xdef0,
    );
    assert_eq!(result.registers.ecx as u16, components);
    assert_eq!(result.registers.edx as u16, 0xdef0);
    assert_eq!(result.registers.esi as u16, 0x1234);
    assert_eq!(result.registers.edi as u16, 0x5678);
    assert_eq!(result.registers.ebp as u16, 0x9abc);
    assert_eq!(result.segments.es.selector, segment);
    if operation != 0 {
        assert_eq!(result.registers.ebx as u16, offset);
    }
    result
}

fn buffer<const N: usize>(dos: &mut Dos, segment: u16, offset: u16) -> [u8; N] {
    let context = dos.cpu().context();
    std::array::from_fn(|index| {
        let address = (u32::from(segment) << 4) + u32::from(offset.wrapping_add(index as u16));
        context.memory.read_u8(address).unwrap()
    })
}
