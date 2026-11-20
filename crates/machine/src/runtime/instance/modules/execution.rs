//! Lends the CPU's installed modules to writeCode while Wasm is executing.

use super::Modules;
use crate::{runtime::WasmRuntime, Memory, Result};
use relocade_wasm_runtime::{Context, FunctionRef, Import, Module, WasmBackend};
use relocade_x86::abi;
use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::{Rc, Weak},
};

type Installed = Box<Modules<WasmRuntime>>;

fn write_code(
    guest: &Memory,
    active: &Weak<RefCell<Option<Installed>>>,
    mut context: Context<'_>,
    (backing, bytes, value): (u32, u32, u64),
) {
    guest
        .view()
        .write_backing(backing, &value.to_le_bytes()[..bytes as usize])
        .expect("tracked RAM fits backing");
    if guest.take_retirement_request() {
        let executing = active.upgrade().expect("live CPU");
        let mut modules = executing.borrow_mut();
        modules
            .as_mut()
            .expect("active write callback")
            .retire_modified(&mut context)
            .expect("retire modified code");
    }
}

/// Stopped callers can borrow installed code. Wasm callbacks can withdraw it
/// while the CPU is executing.
pub(in crate::runtime::instance) struct CpuModules {
    stopped: Option<Installed>,
    executing: Rc<RefCell<Option<Installed>>>,
    pub(in crate::runtime::instance) write_code: FunctionRef<WasmRuntime, abi::CodeWriteSignature>,
}

impl CpuModules {
    pub(in crate::runtime::instance) fn new(wasm: &mut WasmRuntime, guest: Memory) -> Result<Self> {
        let executing: Rc<RefCell<Option<Installed>>> = Rc::default();
        let active = Rc::downgrade(&executing);
        Ok(Self {
            stopped: Some(Box::new(Modules::new(wasm)?)),
            executing,
            write_code: FunctionRef::new(&mut wasm.context(), move |context, store| {
                write_code(&guest, &active, context, store)
            }),
        })
    }

    pub(in crate::runtime::instance) fn instantiate(
        &mut self,
        wasm: &mut WasmRuntime,
        module: &Module,
        imports: Vec<Import<'_, WasmRuntime>>,
    ) -> relocade_wasm_runtime::Result<<WasmRuntime as WasmBackend>::Instance> {
        // Keep the imports available to a start function that retires guest code.
        let (lookup, functions) = self.table.resources();
        let mut imports = imports;
        imports.extend([
            self.write_code.import(abi::WRITE_CODE),
            wasm.bind(abi::EXECUTION_ENTRIES, &lookup),
            wasm.bind(abi::EXECUTION_FUNCTIONS, &functions),
        ]);
        let _callback = self.enable_write_callback();
        wasm.instantiate(module, &imports)
    }

    pub(in crate::runtime::instance) fn enable_write_callback(&mut self) -> WriteCallbackScope<'_> {
        self.executing.replace(self.stopped.take());
        WriteCallbackScope {
            stopped: &mut self.stopped,
            executing: &self.executing,
        }
    }
}

/// Lends the installed modules to `writeCode` until Wasm returns to its caller.
#[must_use]
pub(in crate::runtime::instance) struct WriteCallbackScope<'a> {
    stopped: &'a mut Option<Installed>,
    executing: &'a RefCell<Option<Installed>>,
}

impl Drop for WriteCallbackScope<'_> {
    fn drop(&mut self) {
        *self.stopped = self.executing.take();
    }
}

impl Deref for CpuModules {
    type Target = Modules<WasmRuntime>;
    fn deref(&self) -> &Self::Target {
        self.stopped.as_deref().expect("stopped CPU")
    }
}

impl DerefMut for CpuModules {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.stopped.as_deref_mut().expect("stopped CPU")
    }
}
