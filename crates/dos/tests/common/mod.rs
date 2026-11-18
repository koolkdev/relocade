#![allow(dead_code)]

use relocade_dos::{Dos, ProcessStatus};
mod input;
pub mod mouse;
mod runner;
pub use runner::runner;
#[path = "../../../machine/tests/support/execution.rs"]
mod execution;
#[allow(unused_imports)]
pub use execution::{checkpoint, run_until, steps};

mod fixture;
#[allow(unused_imports)]
pub use fixture::{dos, next_capture, DosFixture};

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
    service_with_input(dos, ax, bx, cx, dx, &[])
}

/// Invoke a DOS service while supplying a script at the BIOS buffer's capacity.
pub fn service_with_input(
    dos: &mut Dos,
    ax: u16,
    bx: u16,
    cx: u16,
    dx: u16,
    bytes: &[u8],
) -> CpuState {
    {
        let mut machine = dos.cpu().context();
        let cpu = &mut machine.cpu;
        cpu.set_eip(0x100);
        cpu.set_ax(ax);
        cpu.set_bx(bx);
        cpu.set_cx(cx);
        cpu.set_dx(dx);
    }
    assert_eq!(
        pollster::block_on(input::feed_input(
            dos.stdin(),
            bytes,
            crate::common::run_until(dos.cpu(), 0x102)
        )),
        ExecutionExit::Interrupted
    );
    dos.cpu().context().cpu.snapshot()
}

/// Terminates a process running SERVICE_PROGRAM through its DOS interrupt.
pub fn terminate(dos: &mut Dos) {
    {
        let execution = dos.cpu().context();
        execution.cpu.set_eip(0x100);
        execution.cpu.set_ax(0x4c00);
    }
    assert_eq!(
        pollster::block_on(dos.cpu().run()),
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
        .context()
        .memory
        .write(0x10300, &terminated)
        .unwrap();
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

#[path = "../../../vga/tests/support/mod.rs"]
pub mod vga_support;
pub use vga_support::inspection;

/// Register-level inspection for fixtures using the default DISPI card.
pub trait VideoInspection {
    fn vga(&self) -> std::rc::Rc<std::cell::RefCell<relocade_vga::DispiAdapter>>;
}

impl VideoInspection for Dos {
    fn vga(&self) -> std::rc::Rc<std::cell::RefCell<relocade_vga::DispiAdapter>> {
        match self.video_card() {
            relocade_dos::video_card::VideoCard::Dispi(card) => card.clone(),
            relocade_dos::video_card::VideoCard::Et4000Ax(_) => panic!("fixture requires DISPI"),
        }
    }
}

pub fn screen(dos: &relocade_dos::Dos) -> inspection::Inspection {
    inspection::inspect(&dos.vga().borrow().snapshot())
}
