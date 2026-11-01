//! Service registration owns IDs, guest entry pairs and their replacement.

use super::{
    hooks::{Continuations, Handler},
    Backend, Cpu, Dispatch, DispatchEntry, Error, Hook, Result, RuntimeMemory,
};
use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExitValues, HostExitValues};
use std::{collections::HashMap, rc::Rc};

#[derive(Clone)]
pub(super) struct Service {
    address: u32,
    pub handler: Rc<dyn Handler>,
    pub continuations: Option<Continuations>,
}

impl Service {
    fn addresses(&self) -> impl Iterator<Item = u32> {
        std::iter::once(self.address).chain(self.continuations.map(|code| code.resume))
    }
}

/// Address lookup belongs to registration. Execution indexes a stable slot.
#[derive(Default)]
pub(super) struct Services {
    ids: HashMap<u32, u32>,
    slots: Vec<Option<Service>>,
}

impl Services {
    #[inline]
    pub fn get(&self, id: u32) -> &Service {
        self.slots
            .get(id as usize)
            .and_then(Option::as_ref)
            .expect("registered host service")
    }

    /// Replacing either registered position removes the whole entry/resume pair.
    pub fn replaced_addresses(&self, addresses: impl Iterator<Item = u32>) -> Vec<u32> {
        let addresses: Vec<_> = addresses.collect();
        self.slots
            .iter()
            .flatten()
            .filter(|service| {
                service
                    .addresses()
                    .any(|address| addresses.contains(&address))
            })
            .flat_map(Service::addresses)
            .collect()
    }

    pub fn remove(&mut self, addresses: &[u32]) {
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|service| addresses.contains(&service.address))
            {
                *slot = None;
            }
        }
    }

    async fn install(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        dispatch: &mut Dispatch,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let mut ids = self.ids.clone();
        let mut slots = self.slots.clone();
        let mut changed = Vec::new();
        for hook in hooks {
            let id = *ids.entry(hook.address).or_insert_with(|| {
                let id = u32::try_from(slots.len()).expect("host service ID capacity");
                slots.push(None);
                id
            });
            slots[id as usize] = Some(Service {
                address: hook.address,
                handler: hook.handler,
                continuations: hook.continuations,
            });
            changed.push(id);
        }
        // Last registration wins for duplicate entries in the same batch.
        changed.sort_unstable();
        changed.dedup();
        let mut addresses = HashMap::new();
        for service in slots.iter().flatten() {
            for address in service.addresses() {
                if addresses.insert(address, ()).is_some() {
                    return Err(Error::ServiceAddressConflict { address });
                }
            }
        }
        let removed: Vec<_> = changed
            .iter()
            .filter_map(|&id| self.slots.get(id as usize).and_then(Option::as_ref))
            .flat_map(Service::addresses)
            .collect();
        let mut program = Program::new();
        let mut exports = Vec::new();
        for id in changed {
            let service = slots[id as usize].as_ref().expect("planned service");
            for (index, address) in service.addresses().enumerate() {
                let exit = if index == 0 {
                    HostExitValues::call(id)
                } else {
                    HostExitValues::resume()
                };
                let name = format!("service_{id}_{index}");
                let function = program.function::<abi::ExecutionSignature>(move |body, ()| {
                    body.return_(ExecutionExitValues::host(exit))
                });
                program.export(&name, function)?;
                exports.push((address, name));
            }
        }
        let entries: Vec<_> = exports
            .iter()
            .map(|(address, name)| DispatchEntry {
                address: *address,
                export: name,
            })
            .collect();
        let bytes = program.compile()?;
        let module = backend.compiler().compile(&bytes).await?;
        dispatch
            .install(backend, bindings, &module, &entries, &removed)
            .await?;
        // Publish handlers only after linking all entries successfully. Stable IDs
        // survive removal, while replaced captures are released together.
        self.ids = ids;
        self.slots = slots;
        Ok(())
    }
}

impl Cpu {
    /// Installs service entries and their optional resume positions atomically.
    /// The predecessor has published CPU state before the driver polls a handler.
    /// Hooks do not retire guest instructions or replenish the instruction budget.
    /// Ready awaits finish in one poll; pending futures survive waiter replacement.
    ///
    /// Replacing an entry removes its previous resume position. An entry or resume
    /// cannot overlap another live service's registered positions. Completion is
    /// ordinary guest code and is not intercepted.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it before installing.
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        let machine = self.machine_mut();
        machine
            .services
            .install(
                &mut machine.backend,
                &machine.bindings,
                &mut machine.dispatch,
                hooks,
            )
            .await
    }
}
