#![allow(dead_code)]

use relocade_dos::{Dos, ProcessStatus};

// Service tests step the CPU directly; worker tests drive registered devices.
pub async fn dos() -> Dos {
    let (clock, timers) = relocade_machine::timer_service();
    std::thread::spawn(|| pollster::block_on(timers.run()));
    Dos::new(&relocade_machine::Runner::with_clock(clock))
        .await
        .unwrap()
}

/// Poll once and release the waiter while the CPU retains the pending service.
pub fn assert_pending(future: impl std::future::Future) {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
}

pub fn output(mut stream: impl std::io::Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    bytes
}

use relocade_x86::{CpuState, ExecutionExit};

// INT 21h followed by JMP $ lets each host assertion inspect the IRET result.
pub const SERVICE_PROGRAM: &[u8] = &[0xcd, 0x21, 0xeb, 0xfe];

pub fn service(dos: &mut Dos, ax: u16, bx: u16, cx: u16, dx: u16) -> CpuState {
    dos.cpu().with(|machine| {
        let cpu = &mut machine.cpu;
        cpu.set_eip(0x100);
        cpu.set_ax(ax);
        cpu.set_bx(bx);
        cpu.set_cx(cx);
        cpu.set_dx(dx);
    });
    assert_eq!(
        pollster::block_on(dos.cpu().run(10)),
        ExecutionExit::BudgetExhausted
    );
    dos.cpu().with(|call| call.cpu.snapshot())
}

/// Terminates a process running SERVICE_PROGRAM through its DOS interrupt.
pub fn terminate(dos: &mut Dos) {
    dos.cpu().with(|execution| {
        execution.cpu.set_eip(0x100);
        execution.cpu.set_ax(0x4c00);
    });
    assert_eq!(
        pollster::block_on(dos.cpu().run(10)),
        ExecutionExit::Host(relocade_x86::HostExit::Return)
    );
    assert_eq!(
        dos.process().unwrap().status(),
        ProcessStatus::Exited(Ok(0))
    );
}

pub fn path_service(dos: &mut Dos, path: &[u8], ax: u16, cx: u16) -> CpuState {
    let mut terminated = path.to_vec();
    terminated.push(0);
    dos.cpu()
        .with(|call| call.memory.write(0x10300, &terminated).unwrap());
    service(dos, ax, 0, cx, 0x300)
}

pub fn success(cpu: &CpuState) -> u16 {
    assert_eq!(cpu.flags.eflags() & 1, 0);
    cpu.registers.eax as u16
}

pub fn error(cpu: &CpuState, code: u16) {
    assert_eq!(cpu.flags.eflags() & 1, 1);
    assert_eq!(cpu.registers.eax as u16, code);
}

pub fn mz_image(code: &[u8]) -> Vec<u8> {
    let size = 0x20 + code.len();
    let mut image = vec![0; 0x20];
    image[..2].copy_from_slice(b"MZ");
    for (offset, value) in [
        (0x02, size as u16), // Bytes in the final file page.
        (0x04, 1),           // One file page.
        (0x08, 2),           // Two header paragraphs.
        (0x0a, 0x10),        // Extra paragraphs for the stack.
        (0x0c, 0x10),
        (0x10, 0x100), // SP within the load segment; CS, SS and IP are zero.
    ] {
        image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(code);
    image
}
