mod common;

use common::Backend;
use relocade_dos::{Dos, DosState};
use relocade_x86::{CpuState, ExecutionExit, Segment, SegmentRegister};

fn service(
    dos: &mut Dos<Backend<DosState>>,
    function: u8,
    paragraphs: u16,
    segment: u16,
) -> CpuState {
    {
        let mut machine = dos.runtime().machine();
        let mut cpu = machine.cpu();
        cpu.set_eip(0x100);
        cpu.set_ah(function);
        cpu.set_bx(paragraphs);
        cpu.set_segment(
            Segment::Es,
            SegmentRegister::real_mode(Segment::Es, segment),
        );
    }
    assert_eq!(
        dos.runtime().run(10).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    dos.runtime().machine().cpu().snapshot()
}

fn assert_success(cpu: &CpuState) {
    assert_eq!(cpu.flags.eflags() & 1, 0);
}

fn assert_error(cpu: &CpuState, code: u16) {
    assert_eq!(cpu.flags.eflags() & 1, 1);
    assert_eq!(cpu.registers.eax as u16, code);
}

async fn check_memory_services() {
    let mut dos = common::dos().await;
    dos.load_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
        .unwrap(); // INT 21h; JMP $

    // COM and its environment initially own all conventional memory. Keep the
    // program's first 64 KiB so its initial stack remains inside the allocation.
    let cpu = service(&mut dos, 0x48, 1, 0);
    assert_error(&cpu, 8);
    assert_eq!(cpu.registers.ebx as u16, 0);
    assert_success(&service(&mut dos, 0x4a, 0x1000, 0x1000));
    let first = service(&mut dos, 0x48, 0x10, 0);
    assert_success(&first);
    assert_eq!(first.registers.eax as u16, 0x2001);
    let second = service(&mut dos, 0x48, 0x20, 0);
    assert_success(&second);
    assert_eq!(second.registers.eax as u16, 0x2012);

    let cpu = service(&mut dos, 0x4a, 0x40, 0x2001);
    assert_error(&cpu, 8); // Cannot grow through the second allocated block.
    assert_eq!(cpu.registers.ebx as u16, 0x10);
    assert_success(&service(&mut dos, 0x49, 0, 0x2012));
    assert_success(&service(&mut dos, 0x4a, 0x40, 0x2001));
    let mut mcb = [0; 5];
    dos.runtime()
        .machine()
        .memory()
        .read(0x20000, &mut mcb)
        .unwrap();
    assert_eq!(mcb, [b'M', 0, 0x10, 0x40, 0]);
    assert_error(&service(&mut dos, 0x49, 0, 0x2002), 9); // Interior pointer.
    let cpu = service(&mut dos, 0x48, 0xffff, 0);
    assert_error(&cpu, 8);
    assert_eq!(cpu.registers.ebx as u16, 0x7fbc);

    // Failed expansion consumes the adjacent free space and reports the maximum.
    let cpu = service(&mut dos, 0x4a, 0xffff, 0x2001);
    assert_error(&cpu, 8);
    assert_eq!(cpu.registers.ebx as u16, 0x7ffd);
    dos.runtime()
        .machine()
        .memory()
        .read(0x20000, &mut mcb)
        .unwrap();
    assert_eq!(mcb, [b'M', 0, 0x10, 0xfd, 0x7f]);
    assert_success(&service(&mut dos, 0x49, 0, 0x2001));
    assert_error(&service(&mut dos, 0x49, 0, 0x2001), 9);

    // A zero-sized block still has its own MCB and can be freed normally.
    let cpu = service(&mut dos, 0x48, 0, 0);
    assert_success(&cpu);
    assert_eq!(cpu.registers.eax as u16, 0x2001);
    assert_success(&service(&mut dos, 0x49, 0, 0x2001));
    let cpu = service(&mut dos, 0x48, 0x7ffd, 0);
    assert_success(&cpu); // Free blocks coalesced, including their header paragraphs.
    assert_eq!(cpu.registers.eax as u16, 0x2001);

    dos.runtime()
        .machine()
        .memory()
        .write(0xfff0, b"!")
        .unwrap();
    assert_error(&service(&mut dos, 0x48, 1, 0), 7);
    dos.load_com(&[0xc3], Default::default()).unwrap(); // Loading resets the arena; RET terminates.
    assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::HostReturn);
    dos.runtime()
        .machine()
        .memory()
        .read(0xfff0, &mut mcb)
        .unwrap();
    assert_eq!(mcb, [b'Z', 0, 0, 0, 0x90]); // All root-process memory released.
}

#[test]
fn dos_allocates_frees_and_resizes_conventional_memory() {
    pollster::block_on(check_memory_services());
}
