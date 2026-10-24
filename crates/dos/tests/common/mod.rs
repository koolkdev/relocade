#![allow(dead_code)]

use relocade_dos::Dos;

pub async fn dos() -> Dos {
    Dos::new().await.unwrap()
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
    {
        let mut machine = dos.cpu().context();
        let mut cpu = machine.cpu();
        cpu.set_eip(0x100);
        cpu.set_ax(ax);
        cpu.set_bx(bx);
        cpu.set_cx(cx);
        cpu.set_dx(dx);
    }
    assert_eq!(
        ready(dos.cpu().run(10)).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    dos.cpu().context().cpu().snapshot()
}

pub fn path_service(dos: &mut Dos, path: &[u8], ax: u16, cx: u16) -> CpuState {
    let mut terminated = path.to_vec();
    terminated.push(0);
    dos.cpu()
        .context()
        .memory()
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

/// Poll a run once so tests can observe suspension without keeping its waiter.
pub fn poll_once<T>(future: impl std::future::Future<Output = T>) -> std::task::Poll<T> {
    let mut future = std::pin::pin!(future);
    future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

/// Ready-input fixtures must finish without an executor turn or a blocking wait.
pub fn ready<T>(future: impl std::future::Future<Output = T>) -> T {
    match poll_once(future) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("expected the run to complete in one poll"),
    }
}
