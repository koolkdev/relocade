#![allow(dead_code)]

use relocade_dos::{Dos, DosState};

use relocade_runtime::RuntimeState;
pub type Backend<T> = relocade_runtime_wasmtime::WasmtimeBackend<RuntimeState<T>>;

pub async fn dos() -> Dos<Backend<DosState>> {
    let backend = Backend::with_state(RuntimeState::new(DosState::default())).unwrap();
    Dos::new(backend).await.unwrap()
}

pub fn output(mut stream: impl std::io::Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    bytes
}

use relocade_x86::{CpuState, ExecutionExit};

// INT 21h followed by JMP $ lets each host assertion inspect the IRET result.
pub const SERVICE_PROGRAM: &[u8] = &[0xcd, 0x21, 0xeb, 0xfe];

pub fn service(dos: &mut Dos<Backend<DosState>>, ax: u16, bx: u16, cx: u16, dx: u16) -> CpuState {
    // Earlier async input may have enabled profiling. This synchronous fixture
    // inspects one DOS service without servicing compilation requests.
    dos.runtime().set_hot_threshold(None);
    {
        let mut machine = dos.runtime().machine();
        let mut cpu = machine.cpu();
        cpu.set_eip(0x100);
        cpu.set_ax(ax);
        cpu.set_bx(bx);
        cpu.set_cx(cx);
        cpu.set_dx(dx);
    }
    assert_eq!(
        dos.runtime().run(10).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    dos.runtime().machine().cpu().snapshot()
}

pub fn path_service(dos: &mut Dos<Backend<DosState>>, path: &[u8], ax: u16, cx: u16) -> CpuState {
    let mut terminated = path.to_vec();
    terminated.push(0);
    dos.runtime()
        .machine()
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
