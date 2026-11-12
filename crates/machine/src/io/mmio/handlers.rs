use crate::{io::handlers::Handler, IoContext};
use relocade_wasm_runtime::FunctionRef;
use relocade_x86::abi;
use std::{cell::OnceCell, rc::Rc};

type Read = dyn Fn(IoContext<'_>, u32, u8) -> u64;
type Write = dyn Fn(IoContext<'_>, u32, u8, u64);

/// A reusable read operation for physical memory transfers of one through eight
/// bytes. The bus splits at mapping boundaries; the callback owns each resulting
/// transfer, including partial widths. It receives the address after A20.
#[derive(Clone)]
pub struct MmioRead(pub(super) Rc<Handler<abi::MmioReadSignature, Read>>);

impl MmioRead {
    /// Prepares a callback once. Unused result bits are discarded by the caller.
    /// Captures should use weak references if the device retains this operation.
    pub fn new(read: impl Fn(IoContext<'_>, u32, u8) -> u64 + 'static) -> Self {
        let read = Rc::new(read);
        let captured = read.clone();
        Self(Rc::new(Handler {
            callback: read,
            bind: Box::new(move |context| {
                let read = captured.clone();
                let bus = context.bus.clone();
                FunctionRef::new(&mut context.runtime, move |runtime, (address, bytes)| {
                    read(IoContext::new(runtime, &bus), address as u32, bytes as u8) as i64
                })
            }),
            function: OnceCell::new(),
        }))
    }
}

/// A reusable write operation with the same address and lifetime contract as
/// [`MmioRead`]. Values are little endian with unused upper bits cleared.
#[derive(Clone)]
pub struct MmioWrite(pub(super) Rc<Handler<abi::MmioWriteSignature, Write>>);

impl MmioWrite {
    pub fn new(write: impl Fn(IoContext<'_>, u32, u8, u64) + 'static) -> Self {
        let write = Rc::new(write);
        let captured = write.clone();
        Self(Rc::new(Handler {
            callback: write,
            bind: Box::new(move |context| {
                let write = captured.clone();
                let bus = context.bus.clone();
                FunctionRef::new(
                    &mut context.runtime,
                    move |runtime, (address, bytes, value)| {
                        write(
                            IoContext::new(runtime, &bus),
                            address as u32,
                            bytes as u8,
                            value as u64,
                        )
                    },
                )
            }),
            function: OnceCell::new(),
        }))
    }
}
