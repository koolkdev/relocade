mod common;

use common::{error, service, SERVICE_PROGRAM};
use relocade_dos::ProcessStatus;
use relocade_x86::{ExecutionExit, HostExit, Segment, SegmentRegister};
use std::io::Write;

#[test]
fn psp_queries_preserve_caller_state_and_pending_control_c() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        service(&mut dos, 0x3301, 0, 0, 1); // BREAK ON.
        dos.stdin().write_all(&[3]).unwrap();
        for ax in [0x51ab, 0x62cd] {
            let state = dos.cpu().cpu_state();
            state.set_ax(ax);
            state.set_ebx(0x89ab_cdef);
            state.set_ecx(0x1234_5678);
            state.set_edx(0x2345_6789);
            state.set_esi(0x3456_789a);
            state.set_edi(0x4567_89ab);
            state.set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x1234));
            state.set_eflags(0x0a57);
            let before = state.snapshot();
            let after = service(&mut dos, ax, 0xcdef, 0x5678, 0x6789);
            let mut registers = before.registers;
            registers.ebx = 0x89ab_1000; // The launched PSP, independent of DS.
            assert_eq!(after.registers, registers);
            assert_eq!(after.segments, before.segments);
            assert_eq!(after.flags.eflags(), 0x0a57);
            assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        }
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, 3);
        assert!(common::output(dos.stdout()).is_empty());
    });
}

async fn check_system_services() {
    let mut dos = common::dos().await;
    dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos.cpu().context().cpu.set_eflags(0x0a56); // OF, IF, ZF, AF and PF.
    error(&service(&mut dos, 0x3e00, 0xffff, 0, 0), 6); // Set carry.
    let cpu = service(&mut dos, 0x3000, 0x1234, 0x5678, 0x9abc);
    assert_eq!(cpu.registers.eax as u16, 5);
    assert_eq!(cpu.registers.ebx as u16, 0xff00);
    assert_eq!(cpu.registers.ecx as u16, 0);
    assert_eq!(cpu.registers.edx as u16, 0x9abc);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // Error sets carry; version preserves flags.
    let cpu = service(&mut dos, 0x3001, 0xffff, 0xffff, 0);
    assert_eq!(cpu.registers.eax as u16, 5);
    assert_eq!(cpu.registers.ebx as u16, 0);

    // A non-normalized pointer survives a set/get round trip exactly.
    dos.cpu()
        .context()
        .cpu
        .set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x1234));
    let cpu = service(&mut dos, 0x25ff, 0, 0, 0xfedc);
    assert_eq!(cpu.flags.eflags() & 1, 1);
    let mut vector = [0; 4];
    dos.cpu().context().memory.read(0x3fc, &mut vector).unwrap();
    assert_eq!(vector, [0xdc, 0xfe, 0x34, 0x12]);
    let cpu = service(&mut dos, 0x35ff, 0, 0, 0);
    assert_eq!(cpu.registers.ebx as u16, 0xfedc);
    assert_eq!(cpu.segments[Segment::Es].selector, 0x1234);
    assert_eq!(cpu.flags.eflags() & 1, 1);

    // Querying DOS's vector returns the address used by normal guest dispatch.
    let cpu = service(&mut dos, 0x3521, 0, 0, 0);
    assert_eq!(cpu.segments[Segment::Es].selector, 0xf000);
    assert_eq!(cpu.registers.ebx as u16, 0x110);
    error(&service(&mut dos, 0x71a0, 0, 0, 0), 0x7100);
    error(&service(&mut dos, 0x714e, 0, 0, 0), 0x7100);
    // A successful zero-byte stdout write clears only the saved carry flag.
    let cpu = service(&mut dos, 0x4000, 1, 0, 0);
    assert_eq!(cpu.registers.eax as u16, 0);
    assert_eq!(cpu.flags.eflags(), 0x0a56);
    // Unsupported LFN calls return to the guest.
    assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
}

async fn check_guest_interrupt_vector() {
    let mut dos = common::dos().await;
    let mut program = vec![
        0xb8, 0x60, 0x25, // MOV AX,2560: install INT 60h
        0xba, 0x20, 1, // MOV DX,0120: handler within CS=DS
        0xcd, 0x21, // Set the vector through DOS.
        0x31, 0xc0, // XOR AX,AX
        0xcd, 0x60, // Invoke the guest handler through the IVT.
        0xa3, 0, 3, // MOV [0300],AX
        0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
    ];
    program.resize(0x20, 0);
    program.extend_from_slice(&[0xb8, 0x34, 0x12, 0xcf]); // MOV AX,1234; IRET
    dos.launch_com(&program, Default::default()).unwrap();
    assert_eq!(
        pollster::block_on(dos.cpu().run()),
        ExecutionExit::Host(HostExit::Return)
    );
    assert_eq!(
        dos.process().unwrap().status(),
        ProcessStatus::Exited(Ok(0))
    );
    let mut result = [0; 2];
    dos.cpu()
        .context()
        .memory
        .read(0x10300, &mut result)
        .unwrap();
    assert_eq!(result, [0x34, 0x12]);
}

#[test]
fn dos_reports_version_vectors_and_absent_lfn_services() {
    pollster::block_on(check_system_services());
}

#[test]
fn dos_installs_a_guest_interrupt_handler() {
    pollster::block_on(check_guest_interrupt_vector());
}

#[test]
fn unclaimed_dos_vectors_and_driver_probes_preserve_registers_and_flags() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let program: Vec<_> = (0x2a..=0x3f)
            .flat_map(|vector| [0xcd, vector, 0xeb, 0xfe])
            .collect();
        dos.launch_com(&program, Default::default()).unwrap();
        // The resident mouse driver replaces INT 33h during DOS installation.
        let unclaimed = (0x2au8..=0x3f)
            .filter(|&vector| vector != 0x33)
            .map(|vector| (vector, 0xcafe, 0x1234));
        for (vector, ax, bx) in unclaimed.chain([
            (0x2f, 0x4300, 0x1234), // XMS: AL must not become 80h.
            (0x2f, 0x1687, 0x1234), // DPMI: AX must stay nonzero.
            (0x2f, 0x1500, 0),      // MSCDEX: caller initializes the drive count.
            (0x2f, 0x1a00, 0x1234), // ANSI.
            (0x2f, 0x1000, 0x1234), // SHARE.
            (0x2f, 0x1600, 0x1234), // Windows.
            (0x2f, 0x7a00, 0x1234), // IPX.
            (0x2d, 0x0000, 0x1234), // AMIS AL=0 installation checks.
            (0x2d, 0x8000, 0x1234),
            (0x2d, 0xff00, 0x1234),
        ]) {
            let ip = 0x100 + u32::from(vector - 0x2a) * 4;
            for flags in [0x856, 0xa57] {
                let state = dos.cpu().cpu_state();
                state.set_eip(ip);
                state.set_ax(ax);
                state.set_bx(bx);
                state.set_ecx(0x2345_6789);
                state.set_edx(0x3456_789a);
                state.set_esi(0x4567_89ab);
                state.set_edi(0x5678_9abc);
                state.set_eflags(flags);
                let before = state.snapshot();
                assert_eq!(
                    common::run_until(dos.cpu(), ip + 2).await,
                    ExecutionExit::Interrupted
                );
                assert_eq!(state.snapshot().registers, before.registers);
                assert_eq!(state.snapshot().segments, before.segments);
                assert_eq!(state.eflags(), flags);
                assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
            }
        }
    });
}

#[test]
fn guest_multiplex_driver_chains_unclaimed_probes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut program = vec![
            0xb8, 0x2f, 0x35, 0xcd, 0x21, // Get original INT 2Fh.
            0x89, 0x1e, 0, 3, // MOV [0300],BX.
            0x8c, 0x06, 2, 3, // MOV [0302],ES.
            0xba, 0x40, 1, 0xb8, 0x2f, 0x25, 0xcd, 0x21, // Install DS:0140.
            0xb8, 0xfe, 0xca, 0xcd, 0x2f, 0xa3, 4, 3, // Claimed function.
            0xb8, 0, 0x43, 0xcd, 0x2f, 0xa3, 6, 3, // XMS probe chains.
            0xb8, 0, 0x4c, 0xcd, 0x21,
        ];
        program.resize(0x40, 0);
        program.extend_from_slice(&[
            0x3d, 0xfe, 0xca, // CMP AX,CAFE.
            0x75, 4, // JNE unclaimed.
            0xb8, 0xef, 0xbe, 0xcf, // MOV AX,BEEF; IRET.
            0x2e, 0xff, 0x2e, 0, 3, // JMP FAR CS:[0300].
        ]);
        dos.launch_com(&program, Default::default()).unwrap();
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(dos.cpu().memory().view().read_u16(0x10304).unwrap(), 0xbeef);
        assert_eq!(dos.cpu().memory().view().read_u16(0x10306).unwrap(), 0x4300);
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(0))
        );
    });
}

#[test]
fn null_and_unknown_dos_functions_clear_only_al() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for function in [
            0x18, 0x1d, 0x1e, 0x20, 0x61, 0x6b, 0x6d, 0x70, 0x72, 0xfe, 0xff,
        ] {
            for flags in [0x856, 0xa57] {
                let state = dos.cpu().cpu_state();
                state.set_eip(0x100);
                state.set_eax(0xabcd_00ef | (function << 8));
                state.set_ebx(0x1234_5678);
                state.set_ecx(0x2345_6789);
                state.set_edx(0x3456_789a);
                state.set_esi(0x4567_89ab);
                state.set_edi(0x5678_9abc);
                state.set_eflags(flags);
                let mut expected = state.snapshot();
                expected.registers.eax &= !0xff;
                assert_eq!(
                    common::run_until(dos.cpu(), 0x102).await,
                    ExecutionExit::Interrupted
                );
                assert_eq!(state.snapshot().registers, expected.registers);
                assert_eq!(state.snapshot().segments, expected.segments);
                assert_eq!(state.eflags(), flags);
                assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
            }
        }
    });
}
