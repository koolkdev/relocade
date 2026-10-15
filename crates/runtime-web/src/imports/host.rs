use super::*;
use crate::memory;
use std::mem::offset_of;
use wasm86_x86::{CpuState, ExecutionContext};

/// A module's direct host imports, retained beside its installed Wasm functions.
pub(crate) struct HostImports {
    pub object: Object,
    _callbacks: Vec<Closure<dyn FnMut() -> u32>>,
}

impl HostImports {
    pub fn new<D: Environment + 'static>(
        common: &Object,
        state: &Rc<RefCell<Imports<D>>>,
        context: &Memory,
        memory: &MemoryAccess,
        callbacks: Vec<wasm86_runtime::backend::HostCallback<D>>,
    ) -> Self {
        let namespace = Object::new();
        let mut closures = Vec::new();
        for (index, callback) in callbacks.into_iter().enumerate() {
            let binding = state.borrow_mut().bind_host_call(callback);
            let mut access = CallbackAccess {
                state: state.clone(),
                memory: memory.clone(),
            };
            let context = context.clone();
            let closure = Closure::new(move || {
                let mut bytes = [0; CpuState::BYTE_LEN];
                memory::read(
                    &context,
                    offset_of!(ExecutionContext, cpu) as u32,
                    &mut bytes,
                );
                let mut state = CpuState::from_bytes(bytes);
                let action =
                    access.access(|imports, memory| imports.host_call(&mut state, memory, binding));
                memory::write(
                    &context,
                    offset_of!(ExecutionContext, cpu) as u32,
                    &state.to_bytes(),
                );
                action
            });
            set_property(&namespace, &format!("entry_{index}"), closure.as_ref());
            closures.push(closure);
        }
        let object = Object::assign(&Object::new(), common);
        set_property(&object, "host", &namespace);
        Self {
            object,
            _callbacks: closures,
        }
    }
}
