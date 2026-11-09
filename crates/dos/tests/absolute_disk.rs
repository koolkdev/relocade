mod common;

use relocade_dos::ProcessStatus;
use relocade_x86::{ExecutionExit, Segment};

#[test]
fn unavailable_absolute_disks_preserve_buffers_and_leave_saved_flags_for_the_caller() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(
            &[
                0xcd, 0x25, 0x9d, 0xeb, 0xfe, // INT 25h; POPF; JMP $.
                0xcd, 0x26, 0x9d, 0xeb, 0xfe, // INT 26h; POPF; JMP $.
            ],
            Default::default(),
        )
        .unwrap();
        let memory = dos.cpu().memory();
        let buffer = [0xa5; 512];
        memory.view().write(0x10300, &buffer).unwrap();
        for ip in [0x100, 0x105] {
            for count in [1, 0xffff] {
                // Traditional and extended packet calls.
                for flags in [0x856, 0xa57] {
                    let state = dos.cpu().cpu_state();
                    state.set_eip(ip);
                    state.set_eax(0xabcd_0002); // Logical drive C:.
                    state.set_bx(0x300);
                    state.set_cx(count);
                    state.set_dx(7);
                    state.set_esi(0x1234_5678);
                    state.set_edi(0x2345_6789);
                    state.set_eflags(flags);
                    let before = state.snapshot();
                    let sp = state.sp();
                    assert_eq!(
                        common::run_until(dos.cpu(), ip + 2).await,
                        ExecutionExit::Interrupted
                    );
                    let mut expected = before.registers;
                    expected.eax = 0xabcd_0201;
                    expected.esp -= 2;
                    assert_eq!(state.snapshot().registers, expected);
                    assert_eq!(state.snapshot().segments, before.segments);
                    assert_eq!(state.eflags(), flags | 0x201); // CF and IF set.
                    let saved_flags =
                        u32::from(state.segment(Segment::Ss).selector) * 16 + u32::from(state.sp());
                    assert_eq!(memory.view().read_u16(saved_flags).unwrap(), flags as u16);
                    let mut after = [0; 512];
                    memory.view().read(0x10300, &mut after).unwrap();
                    assert_eq!(after, buffer);
                    assert_eq!(
                        common::run_until(dos.cpu(), ip + 3).await,
                        ExecutionExit::Interrupted
                    );
                    assert_eq!(state.sp(), sp);
                    assert_eq!(state.eflags(), flags);
                    assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
                }
            }
        }
    });
}
