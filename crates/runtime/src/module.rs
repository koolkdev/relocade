//! Installation of callable modules and their execution-address bindings.

use crate::{
    backend::{Backend, Instance},
    Devices, Error, Machine, Result,
};
use wasm86_x86::{CompiledModule, ExecutionProfile};

/// Generated code plus the exports made reachable at guest execution addresses.
/// All exports use `() -> i64` and the shared CPU, memory and execution-budget ABI.
/// The embedding must uphold the x86 compiler's profile and snapshot-validity
/// requirements. Compile every execution entry with `with_execution_budget()`
/// so instruction and work limits also apply inside installed code. Registered
/// addresses must be block boundaries; blocks must not cross another entry.
/// Installing code does not start a JIT or track mutable code bytes.
pub struct ExecutableModule {
    pub(crate) code: CompiledModule,
    pub(crate) exports: Vec<String>,
    bindings: Vec<EntryBinding>,
}

struct EntryBinding {
    address: u32,
    export: u32,
}

impl ExecutableModule {
    pub fn new(code: CompiledModule) -> Self {
        Self {
            code,
            exports: Vec::new(),
            bindings: Vec::new(),
        }
    }

    /// Associates an execution address with a module export. Several addresses
    /// can refer to the same function; each unique export occupies one table slot.
    pub fn bind_entry(&mut self, address: u32, export: impl Into<String>) {
        let export = export.into();
        let index = self.retain_export(export);
        self.bindings.push(EntryBinding {
            address,
            export: index,
        });
    }

    pub(crate) fn retain_export(&mut self, export: String) -> u32 {
        if let Some(index) = self.exports.iter().position(|name| name == &export) {
            index as u32
        } else {
            let index = self.exports.len() as u32;
            self.exports.push(export);
            index
        }
    }
}

impl<B: Backend, D: Devices + 'static> Machine<B, D> {
    /// Compiles with this machine's originating backend and installs callable
    /// exports against its existing memories. Address bindings become visible
    /// together after instantiation. Replacing an address affects future visits.
    pub async fn install_module(&mut self, module: ExecutableModule) -> Result<()> {
        if module.code.execution_profile != Some(ExecutionProfile::Real16) {
            return Err(Error::IncompatibleModule);
        }
        let compiled = self.runtime.code.backend.compile(&module.code).await?;
        let first_slot = self.next_slot;
        self.instance
            .install_module(&compiled, first_slot, &module.exports)
            .await?;
        self.next_slot = first_slot
            .checked_add(module.exports.len() as u32)
            .expect("function table fits Wasm index space");
        let mut entries = self.entries.clone();
        for binding in module.bindings {
            entries.insert(binding.address, first_slot + binding.export);
        }
        self.instance.set_entries(&entries)?;
        self.entries = entries;
        Ok(())
    }
}
