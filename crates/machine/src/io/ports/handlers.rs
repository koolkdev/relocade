//! Reusable operations with a host call view and one lazily prepared Wasm function.

use crate::io::State;
use crate::{Device, IoContext};
use relocade_compiler::abi::FunctionSignature;
use relocade_wasm_runtime::{FunctionRef, WasmRuntime};
use relocade_x86::abi;
use std::{
    cell::{OnceCell, RefCell},
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
    /// Adapts the device's generic [`Device::read_port`] method. This is an
    /// ordinary selectable operation, with no override or restore state.
    /// The device must remain connected while the operation is used.
    pub fn for_device<D: Device + ?Sized + 'static>(device: &Rc<RefCell<D>>) -> Self {
        let device = Rc::downgrade(device);
        Self::new(move |context, port, bytes| {
            device
                .upgrade()
                .expect("connected device")
                .borrow_mut()
                .read_port(context, port, bytes)
        })
    }

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

/// A reusable write operation with the same binding and lifetime rules as
/// [`PortRead`]. Callbacks receive values with unused upper bits cleared.
#[derive(Clone)]
pub struct PortWrite(pub(super) Rc<Handler<abi::PortWriteSignature, Write>>);

impl PortWrite {
    /// Adapts [`Device::write_port`] with a weak device reference, as in
    /// [`PortRead::for_device`].
    pub fn for_device<D: Device + ?Sized + 'static>(device: &Rc<RefCell<D>>) -> Self {
        let device = Rc::downgrade(device);
        Self::new(move |context, port, bytes, value| {
            device
                .upgrade()
                .expect("connected device")
                .borrow_mut()
                .write_port(context, port, bytes, value);
        })
    }

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
