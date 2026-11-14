use super::super::video::{video, PROGRAM};
use crate::common::{self, VideoInspection};
use relocade_dos::Dos;
use relocade_x86::{CpuState, ExecutionExit};

fn request(dos: &mut Dos, function: u8, bx: u16, dx: u16, status: u16) -> CpuState {
    {
        let context = dos.cpu().context();
        context.cpu.set_si(0x1234);
        context.cpu.set_di(0x5678);
        context.cpu.set_bp(0x9abc);
    }
    let result = video(dos, 0x4f00 | u16::from(function), bx, 0xdef0, dx);
    assert_eq!(result.registers.eax as u16, status);
    assert_eq!(result.registers.ecx as u16, 0xdef0);
    assert_eq!(result.registers.esi as u16, 0x1234);
    assert_eq!(result.registers.edi as u16, 0x5678);
    assert_eq!(result.registers.ebp as u16, 0x9abc);
    if function != 3 || status != 0x4f {
        assert_eq!(result.registers.ebx as u16, bx);
    }
    if function != 5 || bx != 0x100 || status != 0x4f {
        assert_eq!(result.registers.edx as u16, dx);
    }
    result
}

#[test]
fn vbe_window_pointer_switches_the_dispi_bank_through_a_far_call() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        request(&mut dos, 2, 0x101, 0, 0x4f);
        let (status, bytes) = super::information(&mut dos, 1, 0x101, 0x200);
        assert_eq!(status, 0x004f);
        let mut program = vec![0x9a]; // CALL FAR WinFuncPtr; JMP $
        program.extend_from_slice(&bytes[12..16]);
        program.extend_from_slice(&[0xeb, 0xfe]);
        {
            let mut context = dos.cpu().context();
            let code = context.cpu.snapshot().segments.cs.base + 0x110;
            context.memory.write(code, &program).unwrap();
            context.cpu.set_eip(0x110);
            context.cpu.set_ax(0xdead);
            context.cpu.set_bx(0);
            context.cpu.set_dx(1);
            context.cpu.set_eflags(0x643);
        }
        assert_eq!(
            common::run_until(dos.cpu(), 0x115).await,
            ExecutionExit::Interrupted
        );
        let state = dos.cpu().cpu_state();
        assert_eq!(state.sp(), 0xfffe);
        assert_eq!(state.eflags(), 0x643);
        assert_eq!(
            request(&mut dos, 5, 0x100, 0xffff, 0x4f).registers.edx as u16,
            1
        );
        dos.cpu().context().memory.write_u8(0xa0000, 0x5a).unwrap();
        request(&mut dos, 5, 0, 0, 0x4f);
        assert_eq!(dos.cpu().context().memory.read_u8(0xa0000).unwrap(), 0);
        request(&mut dos, 5, 0, 1, 0x4f);
        assert_eq!(dos.cpu().context().memory.read_u8(0xa0000).unwrap(), 0x5a);
    });
}

#[test]
fn mode_and_window_requests_preserve_memory_only_when_requested() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        assert_eq!(
            request(&mut dos, 3, 0x9876, 0x1234, 0x4f).registers.ebx as u16,
            3
        );
        request(&mut dos, 5, 0, 1, 0x14f); // No banked window in text mode.
        request(&mut dos, 2, 0x101, 0x1234, 0x4f);
        for (bank, value) in [(0, 0x37), (1, 0x6e)] {
            request(&mut dos, 5, 0, bank, 0x4f);
            dos.cpu().context().memory.write_u8(0xa0000, value).unwrap();
        }
        let registers = dos.vga().borrow().registers().clone();
        let mut bda = [0; 0x50];
        dos.cpu().context().memory.read(0x449, &mut bda).unwrap();
        for mode in [0x107, 0x4101, 0xc101, 0x8107, 0x83, 0x7f] {
            request(&mut dos, 2, mode, 0x1234, 0x14f);
        }
        for (operation, bank) in [(1, 2), (0x101, 2), (0x200, 2), (0, 64), (0, 0xffff)] {
            request(&mut dos, 5, operation, bank, 0x14f);
        }
        let mut after = [0; 0x50];
        dos.cpu().context().memory.read(0x449, &mut after).unwrap();
        assert_eq!(after, bda);
        assert_eq!(dos.vga().borrow().registers(), &registers);
        assert_eq!(request(&mut dos, 3, 0, 0, 0x4f).registers.ebx as u16, 0x101);
        assert_eq!(
            request(&mut dos, 5, 0x100, 0xffff, 0x4f).registers.edx as u16,
            1
        );
        assert_eq!(dos.cpu().context().memory.read_u8(0xa0000).unwrap(), 0x6e);

        request(&mut dos, 2, 0x8101, 0, 0x4f);
        assert_eq!(request(&mut dos, 3, 0, 0, 0x4f).registers.ebx as u16, 0x101);
        assert_eq!(
            request(&mut dos, 5, 0x100, 0xffff, 0x4f).registers.edx as u16,
            0
        );
        assert_eq!(video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u8, 0xff);
        for (bank, value) in [(0, 0x37), (1, 0x6e)] {
            request(&mut dos, 5, 0, bank, 0x4f);
            assert_eq!(dos.cpu().context().memory.read_u8(0xa0000).unwrap(), value);
        }
        request(&mut dos, 2, 0x101, 0, 0x4f);
        assert_eq!(
            request(&mut dos, 5, 0x100, 0xffff, 0x4f).registers.edx as u16,
            0
        );
        for bank in [0, 1] {
            request(&mut dos, 5, 0, bank, 0x4f);
            assert_eq!(dos.cpu().context().memory.read_u8(0xa0000).unwrap(), 0);
        }
    });
}

#[test]
fn packed_mode_rejects_legacy_output_and_restores_usable_text() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(PROGRAM, Default::default()).unwrap();
        request(&mut dos, 2, 0x101, 0, 0x4f);
        request(&mut dos, 5, 0, 1, 0x4f);
        dos.cpu()
            .context()
            .memory
            .write(0xa0000, &[0xa5; 65536])
            .unwrap();
        assert_eq!(
            video(&mut dos, 0x0601, 0x0700, 0, 0x0101).registers.eax as u16,
            0x0601 // Legacy scrolling has no error status; unsupported modes do nothing.
        );
        for ax in [
            0x0800, 0x0958, 0x0a58, 0x0c01, 0x0d00, 0x0e58, 0x1104, 0x1300,
        ] {
            assert_eq!(video(&mut dos, ax, 7, 2, 0).registers.eax as u16 >> 8, 0x86);
        }
        let mut bytes = [0; 65536];
        dos.cpu()
            .context()
            .memory
            .read(0xa0000, &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0xa5; 65536]);
        assert_eq!(request(&mut dos, 5, 0x100, 0, 0x4f).registers.edx as u16, 1);

        video(&mut dos, 3, 0, 0, 0);
        assert_eq!(request(&mut dos, 3, 0, 0, 0x4f).registers.ebx as u16, 3);
        request(&mut dos, 5, 0x100, 0x1234, 0x14f);
        video(&mut dos, 0x0e5a, 7, 0, 0);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb8000).unwrap(),
            0x075a
        );
        // VBE can also set legacy modes, translating its preserve bit to AH=00h's bit 7.
        request(&mut dos, 2, 0x8003, 0, 0x4f);
        assert_eq!(
            dos.cpu().context().memory.read_u16(0xb8000).unwrap(),
            0x075a
        );
        assert_eq!(request(&mut dos, 3, 0, 0, 0x4f).registers.ebx as u16, 3);
        assert_eq!(video(&mut dos, 0x0f00, 0, 0, 0).registers.eax as u8, 0x83);
    });
}
