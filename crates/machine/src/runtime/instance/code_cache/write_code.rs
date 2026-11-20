//! Routes writes to compiled instruction bytes while Wasm holds the CPU.

use super::{installed::InstalledModules, CodeWake};
use crate::{runtime::WasmRuntime, Memory};
use relocade_wasm_runtime::{Context, FunctionRef, Import, WasmBackend};
use relocade_x86::abi;
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// Wasm can access installed modules only while the code cache lends them to this callback.
struct ActiveModules {
    memory: Memory,
    wake: Arc<CodeWake>,
    installed: RefCell<Option<Box<InstalledModules>>>,
}

impl ActiveModules {
    fn retire_modified(&self, context: &mut Context<'_>) {
        let modified = self.wake.take_modified_modules();
        if !modified.is_empty() {
            self.installed
                .borrow_mut()
                .as_mut()
                .expect("active guest execution")
                .retire(context, modified)
                .expect("retire modified code");
        }
    }
}

pub(in crate::runtime::instance) struct WriteCode {
    callback: FunctionRef<WasmRuntime, abi::CodeWriteSignature>,
    active: Rc<ActiveModules>,
}

impl WriteCode {
    pub(in crate::runtime::instance) fn new(
        wasm: &mut WasmRuntime,
        memory: Memory,
        wake: Arc<CodeWake>,
    ) -> Self {
        let active = Rc::new(ActiveModules {
            memory,
            wake,
            installed: RefCell::default(),
        });
        let executing = Rc::downgrade(&active);
        let callback = FunctionRef::new(
            &mut wasm.context(),
            move |mut context, (backing, bytes, value): (u32, u32, u64)| {
                let active = executing.upgrade().expect("live code cache");
                active
                    .memory
                    .view()
                    .write_backing(backing, &value.to_le_bytes()[..bytes as usize])
                    .expect("tracked RAM fits backing");
                active.retire_modified(&mut context);
            },
        );
        Self { callback, active }
    }

    pub(in crate::runtime::instance) fn import(&self) -> Import<'static, WasmRuntime> {
        self.callback.import(abi::WRITE_CODE)
    }

    /// Lend the installed modules while guest code or a Wasm start function can call writeCode.
    pub(super) fn bind<'a>(
        &'a self,
        stopped: &'a mut Option<Box<InstalledModules>>,
    ) -> WriteCallbackScope<'a> {
        let modules = stopped.take().expect("stopped code cache");
        self.active.installed.replace(Some(modules));
        WriteCallbackScope {
            stopped,
            active: &self.active,
        }
    }
}

#[must_use]
pub(super) struct WriteCallbackScope<'a> {
    stopped: &'a mut Option<Box<InstalledModules>>,
    active: &'a ActiveModules,
}

impl Drop for WriteCallbackScope<'_> {
    fn drop(&mut self) {
        *self.stopped = self.active.installed.take();
    }
}
