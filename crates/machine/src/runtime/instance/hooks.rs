//! Builds hook modules and owns their Rust service IDs and handler captures.

use super::DispatchEntry;
use crate::runtime::{
    hooks::{Handler, Implementation},
    Hook, Result,
};
use relocade_compiler::{abi::Function, Program};
use relocade_x86::{abi, ExecutionExitValues, HostExitValues};
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

#[derive(Default)]
pub(super) struct Hooks {
    ids: HashMap<u32, u32>,
    handlers: Vec<Option<Rc<dyn Handler>>>,
}

/// Generated Wasm and captured handlers awaiting installation together.
pub(super) struct HookModule {
    pub(super) bytes: Vec<u8>,
    exports: Vec<(u32, String)>,
    handlers: Vec<PreparedHandler>,
}

struct PreparedHandler {
    key: u32,
    id: u32,
    handler: Rc<dyn Handler>,
}

impl HookModule {
    pub(super) fn entries(&self) -> Vec<DispatchEntry<'_>> {
        self.exports
            .iter()
            .map(|(key, export)| DispatchEntry { key: *key, export })
            .collect()
    }
}

impl Hooks {
    #[inline]
    pub(super) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.handlers
            .get(id as usize)
            .and_then(Option::as_ref)
            .expect("registered host service")
    }

    pub(super) fn compile(&self, hooks: impl IntoIterator<Item = Hook>) -> Result<HookModule> {
        // The last hook at a key wins, before any block builders run.
        let hooks: BTreeMap<_, _> = hooks
            .into_iter()
            .map(|hook| (hook.key, hook.implementation))
            .collect();
        let mut program = Program::new();
        let mut exports = Vec::new();
        let mut handlers = Vec::new();
        let mut next_id = self.handlers.len();
        for (key, implementation) in hooks {
            let function = match implementation {
                Implementation::Host(handler) => {
                    let id = self.ids.get(&key).copied().unwrap_or_else(|| {
                        let id = u32::try_from(next_id).expect("host service ID capacity");
                        next_id += 1;
                        id
                    });
                    handlers.push(PreparedHandler { key, id, handler });
                    program
                        .function::<abi::ExecutionSignature>(move |body, ()| {
                            body.return_(ExecutionExitValues::host(HostExitValues::call(id)))
                        })
                        .into()
                }
                Implementation::Block(build) => build(&mut program),
            };
            let name = format!("entry_{}", exports.len());
            Function::<abi::ExecutionSignature>::new("", &name).export(&mut program, function)?;
            exports.push((key, name));
        }
        Ok(HookModule {
            bytes: program.compile()?,
            exports,
            handlers,
        })
    }

    /// Installs prepared handlers after their Wasm entries are installed.
    pub(super) fn install(&mut self, module: HookModule) {
        for PreparedHandler { key, id, handler } in module.handlers {
            self.ids.insert(key, id);
            if self.handlers.len() <= id as usize {
                self.handlers.resize_with(id as usize + 1, || None);
            }
            self.handlers[id as usize] = Some(handler);
        }
    }

    /// Releases overwritten captures, retaining their IDs for reinstallation.
    pub(super) fn remove(&mut self, keys: impl IntoIterator<Item = u32>) {
        for key in keys {
            if let Some(&id) = self.ids.get(&key) {
                self.handlers[id as usize] = None;
            }
        }
    }
}
