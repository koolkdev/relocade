//! Shared holds on the physical pages whose writes must reach the host.

use super::RuntimeMemory;
use crate::{runtime::WasmRuntimeHandle, MachineConfig};
use relocade_wasm_runtime::{Memory as WasmMemory, WasmMemoryAccess};
use relocade_x86::PhysicalMemoryMap;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};

/// Acquiring the first hold and dropping the last both access the stopped CPU's map.
pub(in crate::runtime) struct HostWritePages {
    runtime: WasmRuntimeHandle,
    config: MachineConfig,
    mapping: WasmMemory,
    // Only callers holding a page keep its host-write route active.
    held: RefCell<BTreeMap<u32, Weak<HostWritePage>>>,
}

impl HostWritePages {
    pub(in crate::runtime) fn new(runtime: &WasmRuntimeHandle, memory: &RuntimeMemory) -> Rc<Self> {
        Rc::new(Self {
            runtime: runtime.clone(),
            config: memory.guest.configuration().clone(),
            mapping: memory.mapping.clone(),
            held: RefCell::default(),
        })
    }

    /// Hold the host-write route for the RAM or ROM page containing this bus address.
    pub(in crate::runtime) fn hold(self: &Rc<Self>, physical: u32) -> Rc<HostWritePage> {
        let physical_page = physical & !(PhysicalMemoryMap::PAGE_BYTES - 1);
        let mut held = self.held.borrow_mut();
        if let Some(page) = held.get(&physical_page) {
            return page.upgrade().expect("live host-write page");
        }
        self.runtime
            .with_runtime(|wasm| self.route_writes(wasm, physical_page, true));
        let page = Rc::new(HostWritePage {
            physical_page,
            owner: Rc::downgrade(self),
        });
        held.insert(physical_page, Rc::downgrade(&page));
        page
    }

    fn route_writes(
        &self,
        wasm: &mut impl WasmMemoryAccess<Memory = WasmMemory>,
        physical_page: u32,
        via_host: bool,
    ) {
        let (offset, kind) = self
            .config
            .memory_map()
            .host_write_entry(physical_page, via_host)
            .expect("host writes require a RAM or ROM page");
        wasm.memory(&self.mapping).store(offset, kind);
    }

    fn release(&self, wasm: &mut impl WasmMemoryAccess<Memory = WasmMemory>, physical_page: u32) {
        self.held.borrow_mut().remove(&physical_page);
        self.route_writes(wasm, physical_page, false);
    }
}

/// The last holder restores the page's configured route while the CPU's map is live.
pub(in crate::runtime) struct HostWritePage {
    physical_page: u32,
    owner: Weak<HostWritePages>,
}

impl HostWritePage {
    /// Release a hold while the engine is already borrowed, including in a Wasm callback.
    pub(in crate::runtime) fn release(
        self: Rc<Self>,
        wasm: &mut impl WasmMemoryAccess<Memory = WasmMemory>,
    ) {
        let Some(mut page) = Rc::into_inner(self) else {
            return;
        };
        // The supplied context handles the final release. Drop has nothing left to restore.
        if let Some(owner) = std::mem::take(&mut page.owner).upgrade() {
            owner.release(wasm, page.physical_page);
        }
    }
}

impl Drop for HostWritePage {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner
                .runtime
                .with_runtime(|wasm| owner.release(wasm, self.physical_page));
        }
    }
}

#[cfg(test)]
mod tests;
