//! Reusable operations with a host call view and one lazily prepared Wasm function.

use crate::io::State;
use crate::IoContext;
use relocade_compiler::abi::FunctionSignature;
use relocade_wasm_runtime::{FunctionRef, WasmRuntime};
use relocade_x86::abi;
use std::{
    cell::OnceCell,
    rc::{Rc, Weak},
};

pub(super) type Read = dyn Fn(IoContext<'_>, u16, u8) -> u32;
pub(super) type Write = dyn Fn(IoContext<'_>, u16, u8, u32);
type Bind<S> = dyn Fn(&mut IoContext<'_>) -> FunctionRef<WasmRuntime, S>;

pub(super) struct Handler<S: FunctionSignature, C: ?Sized> {
    pub(super) callback: Rc<C>,
    bind: Box<Bind<S>>,
    function: OnceCell<(Weak<State>, FunctionRef<WasmRuntime, S>)>,
}

impl<S: FunctionSignature, C: ?Sized> Handler<S, C> {
    pub(super) fn function(&self, context: &mut IoContext<'_>) -> &FunctionRef<WasmRuntime, S> {
        self.check_owner(context);
        let (_, function) = self
            .function
            .get_or_init(|| (context.bus.clone(), (self.bind)(context)));
        function
    }

    pub(super) fn check_owner(&self, context: &IoContext<'_>) {
        if let Some((owner, _)) = self.function.get() {
            assert!(
                Weak::ptr_eq(owner, context.bus),
                "port handler is bound to another I/O bus"
            );
        }
    }
}

/// A reusable read operation, bound to one bus on first installation.
/// Keep mode-specific operations in the device and reuse them at state changes.
/// Native runtimes retain prepared functions until bus teardown; captures should
/// use weak device references when the device itself retains this handler.
#[derive(Clone)]
pub struct PortRead(pub(super) Rc<Handler<abi::PortReadSignature, Read>>);

impl PortRead {
    /// Delivers the original starting address and complete width of 1, 2 or 4
    /// bytes to the callback, which defines the transfer behavior. Callers
    /// consume only the low result bytes.
    pub fn new(read: impl Fn(IoContext<'_>, u16, u8) -> u32 + 'static) -> Self {
        let read = Rc::new(read);
        let captured = read.clone();
        Self(Rc::new(Handler {
            callback: read,
            // Keep the concrete capture in the Wasm function. This erased
            // factory runs only when preparing, never on an ordinary port read.
            bind: Box::new(move |context| {
                let read = captured.clone();
                let bus = context.bus.clone();
                FunctionRef::new(&mut context.runtime, move |runtime, (port, bytes)| {
                    read(IoContext::new(runtime, &bus), port as u16, bytes as u8) as i32
                })
            }),
            function: OnceCell::new(),
        }))
    }
}

/// A reusable write operation with the same binding and lifetime rules as
/// [`PortRead`]. Callbacks receive values with unused upper bits cleared.
///
/// For a word write of 0F02 at an index port, the starting operation determines
/// whether and how the following port is accessed:
///
/// | Constructor | Behavior |
/// |---|---|
/// | [`byte`](Self::byte) | Rejects the write without invoking the callback. |
/// | [`low_byte`](Self::low_byte) | Invokes the callback once with 02; the next port is untouched. |
/// | [`byte_sequence`](Self::byte_sequence) | Invokes the first callback with 02, then resolves the next port for 0F if inside the window. |
/// | [`word`](Self::word) | Invokes one callback with the complete word 0F02. |
/// | [`new`](Self::new) | Delivers the original address, width and value to the callback. |
#[derive(Clone)]
pub struct PortWrite(pub(super) Rc<Handler<abi::PortWriteSignature, Write>>);

impl PortWrite {
    /// Delivers the original starting address, complete width of 1, 2 or 4 bytes,
    /// and value to the callback, which defines the transfer behavior.
    pub fn new(write: impl Fn(IoContext<'_>, u16, u8, u32) + 'static) -> Self {
        let write = Rc::new(write);
        let captured = write.clone();
        Self(Rc::new(Handler {
            callback: write,
            bind: Box::new(move |context| {
                let write = captured.clone();
                let bus = context.bus.clone();
                FunctionRef::new(
                    &mut context.runtime,
                    move |runtime, (port, bytes, value)| {
                        write(
                            IoContext::new(runtime, &bus),
                            port as u16,
                            bytes as u8,
                            value as u32,
                        )
                    },
                )
            }),
            function: OnceCell::new(),
        }))
    }
}
