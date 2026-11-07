use super::super::video::{video, PROGRAM};
use crate::common;
use relocade_dos::Dos;
use relocade_machine::FarPointer;
use relocade_x86::{CpuState, Segment, SegmentRegister};

const BUFFER: FarPointer = FarPointer {
    segment: 0x5000,
    offset: 0xfffc,
};

fn state(dos: &mut Dos, operation: u8, components: u16, pointer: FarPointer) -> CpuState {
    {
        let context = dos.cpu().context();
        context.cpu.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, pointer.segment),
        );
        context.cpu.set_si(0x1234);
        context.cpu.set_di(0x2345);
        context.cpu.set_bp(0x3456);
    }
    let result = video(
        dos,
        0x4f04,
        pointer.offset,
        components,
        0xab00 | u16::from(operation),
    );
    if operation != 0 || result.registers.eax as u16 != 0x4f {
        assert_eq!(result.registers.ebx as u16, pointer.offset);
    }
    assert_eq!(result.registers.ecx as u16, components);
    assert_eq!(result.registers.edx as u16, 0xab00 | u16::from(operation));
    assert_eq!(result.registers.esi as u16, 0x1234);
    assert_eq!(result.registers.edi as u16, 0x2345);
    assert_eq!(result.registers.ebp as u16, 0x3456);
    assert_eq!(result.segments.es.selector, pointer.segment);
    result
}

fn read<const N: usize>(dos: &mut Dos, pointer: FarPointer) -> [u8; N] {
    let mut bytes = [0; N];
    dos.cpu()
        .context()
        .memory
        .read_wrapping(pointer, &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn component_masks_query_and_save_bounded_wrapping_buffers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        for (index, bytes) in [
            90usize, 74, 148, 789, 863, 847, 921, 34, 108, 92, 166, 807, 881, 865, 939,
        ]
        .into_iter()
        .enumerate()
        {
            let mask = index as u16 + 1;
            let result = state(&mut dos, 0, mask, BUFFER);
            assert_eq!(result.registers.eax as u16, 0x4f);
            assert_eq!(usize::from(result.registers.ebx as u16), bytes.div_ceil(64));
            dos.cpu()
                .context()
                .memory
                .write_wrapping(BUFFER.wrapping_add(0xffff), &[0xa5; 962])
                .unwrap();
            assert_eq!(state(&mut dos, 1, mask, BUFFER).registers.eax as u16, 0x4f);
            assert_eq!(read::<1>(&mut dos, BUFFER.wrapping_add(0xffff)), [0xa5]);
            assert_eq!(read::<1>(&mut dos, BUFFER.wrapping_add(bytes)), [0xa5]);
            assert_eq!(state(&mut dos, 2, mask, BUFFER).registers.eax as u16, 0x4f);
        }
    });
}

#[test]
fn relocated_state_restores_selected_classes_and_preserves_packed_memory() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x4f02, 0x103, 0, 0);
        video(&mut dos, 0x4f06, 0, 1024, 0);
        video(&mut dos, 0x4f07, 0, 24, 600);
        video(&mut dos, 0x4f05, 0, 0, 7);
        {
            let mut context = dos.cpu().context();
            context.memory.write_u8(0xa0010, 0x7b).unwrap();
            context.memory.write_u8(0xaffff, 0xc5).unwrap();
            context.memory.write_u32(0x10c, 0xf000_1234).unwrap();
            context.io.write_port(0x3c6, 1, 0xa5);
            context.io.write_port(0x3c8, 1, 17);
            context.io.write_port(0x3c9, 1, 10); // Leave the DAC transfer at green.
            context.io.write_port(0x1ce, 2, 0x1234);
            context.io.read_port(0x3da, 1);
            context.io.write_port(0x3c0, 1, 0x32); // Leave attribute data pending.
        }
        let core = dos.vga().borrow().core_state();
        let dac = dos.vga().borrow().dac_state();
        let dispi = dos.vga().borrow().dispi_state();
        let firmware = read::<30>(&mut dos, FarPointer::from_packed(0x449));
        assert_eq!(state(&mut dos, 1, 15, BUFFER).registers.eax as u16, 0x4f);
        let saved = read::<939>(&mut dos, BUFFER);
        let relocated = FarPointer {
            segment: 0x4800,
            offset: 0x200,
        };
        dos.cpu()
            .context()
            .memory
            .write_wrapping(relocated, &saved)
            .unwrap();

        for mask in [1, 2, 4, 8, 15] {
            video(&mut dos, 0x4f02, 0x8101, 0, 0);
            {
                let mut context = dos.cpu().context();
                context.io.write_port(0x3c4, 2, 0x2001);
                context.memory.write_u32(0x10c, 0xf000_5678).unwrap();
            }
            let other_core = dos.vga().borrow().core_state();
            let other_dac = dos.vga().borrow().dac_state();
            let other_dispi = dos.vga().borrow().dispi_state();
            let other_firmware = read::<30>(&mut dos, FarPointer::from_packed(0x449));
            assert_eq!(
                state(&mut dos, 2, mask, relocated).registers.eax as u16,
                0x4f
            );
            let device = dos.vga();
            let vga = device.borrow();
            assert_eq!(
                &vga.core_state(),
                if mask & 1 != 0 { &core } else { &other_core }
            );
            assert_eq!(
                &vga.dac_state(),
                if mask & 4 != 0 { &dac } else { &other_dac }
            );
            assert_eq!(
                &vga.dispi_state(),
                if mask & 8 != 0 { &dispi } else { &other_dispi }
            );
            drop(vga);
            assert_eq!(
                read::<30>(&mut dos, FarPointer::from_packed(0x449)),
                if mask & 2 != 0 {
                    firmware
                } else {
                    other_firmware
                }
            );
            assert_eq!(
                dos.cpu().context().memory.read_u32(0x10c).unwrap(),
                if mask & 2 != 0 {
                    0xf000_1234
                } else {
                    0xf000_5678
                }
            );
        }
        // Restoring flags=1 must not replay the VRAM-clearing enable write.
        assert_eq!(video(&mut dos, 0x4f03, 0, 0, 0).registers.ebx as u16, 0x103);
        assert_eq!(video(&mut dos, 0x4f05, 0x100, 0, 0).registers.edx as u16, 7);
        let context = dos.cpu().context();
        assert_eq!(context.memory.read_u8(0xa0010).unwrap(), 0x7b);
        assert_eq!(context.memory.read_u8(0xaffff).unwrap(), 0xc5);
        context.io.write_port(0x3c9, 1, 20);
        context.io.write_port(0x3c9, 1, 30);
        assert_eq!(dos.vga().borrow().snapshot().dac.colors[17], [10, 20, 30]);
    });
}

#[test]
fn invalid_state_and_requests_fail_before_restoring_any_class() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        video(&mut dos, 0x4f02, 0x103, 0, 0);
        assert_eq!(state(&mut dos, 1, 15, BUFFER).registers.eax as u16, 0x4f);
        let saved = read::<939>(&mut dos, BUFFER);
        video(&mut dos, 0x4f02, 3, 0, 0);
        let core = dos.vga().borrow().core_state();
        let dac = dos.vga().borrow().dac_state();
        let dispi = dos.vga().borrow().dispi_state();
        let firmware = read::<30>(&mut dos, FarPointer::from_packed(0x449));
        // Signature, version, mask, offsets, attribute phase, DAC phase and DISPI flags.
        for (offset, value) in [(0, 0), (4, 2), (6, 16), (8, 0), (24, 2), (150, 3), (929, 2)] {
            let mut corrupt = saved;
            corrupt[offset] = value;
            dos.cpu()
                .context()
                .memory
                .write_wrapping(BUFFER, &corrupt)
                .unwrap();
            assert_eq!(state(&mut dos, 2, 15, BUFFER).registers.eax as u16, 0x14f);
            assert_eq!(dos.vga().borrow().core_state(), core);
            assert_eq!(dos.vga().borrow().dac_state(), dac);
            assert_eq!(dos.vga().borrow().dispi_state(), dispi);
            assert_eq!(
                read::<30>(&mut dos, FarPointer::from_packed(0x449)),
                firmware
            );
        }
        dos.cpu()
            .context()
            .memory
            .write_wrapping(BUFFER, &saved)
            .unwrap();
        for (operation, mask) in [(0, 0), (0, 16), (1, 0), (1, 16), (2, 0), (2, 16), (3, 15)] {
            assert_eq!(
                state(&mut dos, operation, mask, BUFFER).registers.eax as u16,
                0x14f
            );
            assert_eq!(read::<939>(&mut dos, BUFFER), saved);
        }
        assert_eq!(state(&mut dos, 1, 1, BUFFER).registers.eax as u16, 0x4f);
        assert_eq!(state(&mut dos, 2, 15, BUFFER).registers.eax as u16, 0x14f);
        dos.cpu().context().io.write_port(0x46e8, 1, 0);
        for operation in 0..=2 {
            assert_eq!(
                state(&mut dos, operation, 1, BUFFER).registers.eax as u16,
                0x14f
            );
        }
    });
}

#[test]
fn firmware_state_can_overlap_its_own_source_and_destination() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        let pointer = FarPointer::from_packed(0x449);
        let original = read::<30>(&mut dos, pointer);
        assert_eq!(state(&mut dos, 1, 2, pointer).registers.eax as u16, 0x4f);
        assert_ne!(read::<30>(&mut dos, pointer), original);
        assert_eq!(state(&mut dos, 2, 2, pointer).registers.eax as u16, 0x4f);
        assert_eq!(read::<30>(&mut dos, pointer), original);
    });
}
