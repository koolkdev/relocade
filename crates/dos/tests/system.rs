mod common;

use common::{error, service, SERVICE_PROGRAM};
use relocade_dos::ProcessStatus;
use relocade_x86::{ExecutionExit, HostExit, Segment, SegmentRegister};

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
        pollster::block_on(dos.cpu().run(100)),
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
