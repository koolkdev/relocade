//! Reusable operations with a host call view and one lazily prepared Wasm function.

use crate::io::State;
use crate::{runtime::Backend, IoContext};
use relocade_compiler::abi::FunctionSignature;
use relocade_wasm_runtime::FunctionRef;
use relocade_x86::abi;
use std::{
    cell::OnceCell,
    rc::{Rc, Weak},
};

pub(super) type Read = dyn Fn(IoContext<'_>, u16, u8) -> u32;
pub(super) type Write = dyn Fn(IoContext<'_>, u16, u8, u32);
type Bind<S> = dyn Fn(&mut IoContext<'_>) -> FunctionRef<Backend, S>;

pub(super) struct Handler<S: FunctionSignature, C: ?Sized> {
    pub(super) callback: Rc<C>,
    bind: Box<Bind<S>>,
    function: OnceCell<(Weak<State>, FunctionRef<Backend, S>)>,
}

impl<S: FunctionSignature, C: ?Sized> Handler<S, C> {
    pub(super) fn function(&self, context: &mut IoContext<'_>) -> &FunctionRef<Backend, S> {
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

/// A reusable read operation. Construct mode-specific operations once, then pass
/// references to [`super::DevicePorts::set_read`] at state changes. Creation does
/// not require a CPU. Its Wasm function is prepared when CPU tables initialize,
/// or when installed after initialization. Preparation binds it to that bus.
/// Native stores retain created functions until bus teardown. Captures stay on
/// the execution thread; use weak device references to avoid ownership cycles.
#[derive(Clone)]
pub struct PortRead(pub(super) Rc<Handler<abi::PortReadSignature, Read>>);

impl PortRead {
    /// The callback receives a width of 1, 2 or 4 bytes and owns wide-read
    /// behavior. Callers consume only the low result bytes.
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

/// A reusable write operation, with the same lifetime and binding rules as
/// [`PortRead`]. Values have unused high bits cleared before the callback.
#[derive(Clone)]
pub struct PortWrite(pub(super) Rc<Handler<abi::PortWriteSignature, Write>>);

impl PortWrite {
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
