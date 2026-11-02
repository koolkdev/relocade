//! Device callbacks publish host memory changes before returning to guest code.

use super::{dispatch::code::CodeRegistry, Backend};
use relocade_wasm_runtime::{Import, WasmBackend, WasmContext};
use relocade_x86::abi;
use std::sync::{Arc, Mutex};

pub(crate) fn imports(
    backend: &mut Backend,
    code: Arc<Mutex<CodeRegistry>>,
) -> Vec<Import<'static, Backend>> {
    let read_port = code.clone();
    let write_port = code.clone();
    let read_mmio = code.clone();
    let write_mmio = code.clone();
    vec![
        backend.bind_host(
            abi::READ_PORT,
            Box::new(move |mut context, port, bytes| {
                let value = context.state().read_port(port as u16, bytes as u8) as i32;
                read_port.lock().unwrap().sync(&mut context);
                value
            }),
        ),
        backend.bind_host(
            abi::WRITE_PORT,
            Box::new(move |mut context, port, bytes, value| {
                context
                    .state()
                    .write_port(port as u16, bytes as u8, value as u32);
                write_port.lock().unwrap().sync(&mut context);
            }),
        ),
        backend.bind_host(
            abi::READ_MMIO,
            Box::new(move |mut context, address, bytes| {
                let value = context.state().read_mmio(address as u32, bytes as u8) as i64;
                read_mmio.lock().unwrap().sync(&mut context);
                value
            }),
        ),
        backend.bind_host(
            abi::WRITE_MMIO,
            Box::new(move |mut context, address, bytes, value| {
                context
                    .state()
                    .write_mmio(address as u32, bytes as u8, value as u64);
                write_mmio.lock().unwrap().sync(&mut context);
            }),
        ),
        backend.bind_host(
            abi::INVALIDATE_CODE,
            Box::new(move |mut context, address, bytes| {
                code.lock()
                    .unwrap()
                    .write(&mut context, address as u32, bytes as u32);
            }),
        ),
    ]
}
