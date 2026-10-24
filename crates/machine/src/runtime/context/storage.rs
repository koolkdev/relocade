use super::MachineCore;
use relocade_wasm_runtime::{
    MemoryAccess, SharedMemoryStorage, WasmMemoryAccess, WasmSharedMemory,
};
use std::rc::{Rc, Weak};

#[derive(Clone, Copy)]
pub(super) enum Region {
    Cpu,
    Guest,
}

pub(super) struct ContextMemory<T> {
    core: Weak<MachineCore<T>>,
    invocation: Option<u64>,
    region: Region,
}

impl<T> Clone for ContextMemory<T> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            invocation: self.invocation,
            region: self.region,
        }
    }
}

impl<T> ContextMemory<T> {
    pub fn new(core: &Rc<MachineCore<T>>, invocation: Option<u64>, region: Region) -> Self {
        Self {
            core: Rc::downgrade(core),
            invocation,
            region,
        }
    }

    pub fn core(&self) -> Rc<MachineCore<T>> {
        let core = self.core.upgrade().expect("CPU has been dropped");
        core.validate(self.invocation);
        core
    }

    #[inline]
    fn access<R>(&self, operation: impl FnOnce(MemoryAccess<'_>) -> R) -> R {
        let core = self.core();
        match self.region {
            Region::Cpu => operation(core.backend.borrow_mut().memory(&core.bindings.context)),
            Region::Guest => operation(core.bindings.shared_guest.view()),
        }
    }
}

macro_rules! scalar {
    ($load:ident, $store:ident, $type:ty) => {
        #[inline]
        fn $load(&self, offset: u32) -> $type {
            self.access(|memory| memory.load(offset))
        }
        #[inline]
        fn $store(&self, offset: u32, value: $type) {
            self.access(|mut memory| memory.store(offset, value));
        }
    };
}

impl<T> SharedMemoryStorage for ContextMemory<T> {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.access(|memory| memory.read(offset, bytes));
    }
    fn write(&self, offset: u32, bytes: &[u8]) {
        self.access(|mut memory| memory.write(offset, bytes));
    }
    // Preserve the bound per-width backend path through the owned handle.
    scalar!(load_u8, store_u8, u8);
    scalar!(load_u16, store_u16, u16);
    scalar!(load_u32, store_u32, u32);
    scalar!(load_u64, store_u64, u64);
}
