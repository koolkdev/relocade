//! Lends the CPU's installed modules to writeCode while Wasm is executing.

use super::Modules;
use crate::{runtime::WasmRuntime, Memory, Result};
use relocade_wasm_runtime::{Context, FunctionRef, Import, Module, WasmBackend};
use relocade_x86::abi;
use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
};

type Installed = Box<Modules<WasmRuntime>>;

/// The installed modules are in this slot only while Wasm can call back.
struct ExecutingModules {
    memory: Memory,
    modules: RefCell<Option<Installed>>,
}

impl ExecutingModules {
    fn retire_modified(&self, context: &mut Context<'_>) {
        let modified = self.memory.take_modified_modules();
        if !modified.is_empty() {
            self.modules
                .borrow_mut()
                .as_mut()
                .expect("active guest execution")
                .retire(context, modified)
                .expect("retire modified code");
        }
    }

    fn write_code(&self, mut context: Context<'_>, (backing, bytes, value): (u32, u32, u64)) {
        self.memory
            .view()
            .write_backing(backing, &value.to_le_bytes()[..bytes as usize])
            .expect("tracked RAM fits backing");
        self.retire_modified(&mut context);
    }
}

/// Stopped callers can borrow installed code. Wasm callbacks can withdraw it
/// while the CPU is executing.
pub(in crate::runtime::instance) struct CpuModules {
    stopped: Option<Installed>,
    executing: Rc<ExecutingModules>,
    pub(in crate::runtime::instance) write_code: FunctionRef<WasmRuntime, abi::CodeWriteSignature>,
}

impl CpuModules {
    pub(in crate::runtime::instance) fn new(wasm: &mut WasmRuntime, guest: Memory) -> Result<Self> {
        let executing = Rc::new(ExecutingModules {
            memory: guest,
            modules: RefCell::default(),
        });
        let active = executing.clone();
        Ok(Self {
            stopped: Some(Box::new(Modules::new(wasm)?)),
            executing,
            write_code: FunctionRef::new(&mut wasm.context(), move |context, store| {
                active.write_code(context, store)
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

    /// Apply stopped-host writes before any guest entry can follow a cached link.
    pub(in crate::runtime::instance) fn enter_guest(
        &mut self,
        context: &mut Context<'_>,
    ) -> WriteCallbackScope<'_> {
        let callback = self.enable_write_callback();
        callback.executing.retire_modified(context);
        callback
    }

    fn enable_write_callback(&mut self) -> WriteCallbackScope<'_> {
        self.executing.modules.replace(self.stopped.take());
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
    executing: &'a ExecutingModules,
}

impl Drop for WriteCallbackScope<'_> {
    fn drop(&mut self) {
        *self.stopped = self.executing.modules.take();
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
