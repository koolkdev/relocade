mod transfers;

use crate::{memory::check_range, Environment, GuestMemory, MemoryAccess, MemoryError, MemoryMap};
use crate::{HostCallback, HostFunctions};
use wasm86_x86::ExecutionTarget;

/// Shared implementation of the generated interpreter's host imports. Backends
/// bind these methods to Wasm imports and supply scoped access to guest backing.
/// Each device call publishes changed routing before returning to generated code.
pub struct Imports<D> {
    environment: D,
    map: MemoryMap,
    initial: Vec<(u32, Vec<u8>)>,
    target: ExecutionTarget,
    pub(crate) pending_functions: HostFunctions<D>,
    pub(crate) callbacks: Vec<Option<HostCallback<D>>>,
}

impl<D: Environment> Imports<D> {
    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    /// Consumes configured functions for compilation by the attaching runtime.
    pub fn take_host_functions(&mut self) -> HostFunctions<D> {
        std::mem::take(&mut self.pending_functions)
    }

    pub fn environment(&self) -> &D {
        &self.environment
    }
    pub fn environment_mut(&mut self) -> &mut D {
        &mut self.environment
    }
    pub fn memory_map(&self) -> &MemoryMap {
        &self.map
    }

    /// Adapts typed environment resolution to the generated x86 import ABI.
    pub fn resolve_segment(
        &mut self,
        segment: u32,
        selector: u32,
    ) -> (u32, u32, u32, u32, u32, u32) {
        use wasm86_x86::{Exception, Segment};
        match self
            .environment
            .resolve_segment(Segment::ALL[segment as usize], selector as u16)
        {
            Ok(cache) => (
                0,
                0,
                cache.base,
                cache.limit,
                u32::from(cache.selector),
                u32::from(cache.attributes.bits()),
            ),
            Err(exception) => {
                let (vector, code) = match exception {
                    Exception::GeneralProtection { error_code } => (13, error_code),
                    Exception::StackFault { error_code } => (12, error_code),
                    Exception::SegmentNotPresent { error_code } => (11, error_code),
                    _ => panic!("segment resolution must return a segment fault"),
                };
                (vector, code, 0, 0, 0, 0)
            }
        }
    }
    pub fn query_segment_descriptor(&mut self, selector: u32) -> (u32, u32, u32, u32, u32) {
        let info = self.environment.query_segment_descriptor(selector as u16);
        (
            u32::from(info.visible),
            u32::from(info.readable),
            u32::from(info.writable),
            info.access_rights,
            info.limit,
        )
    }

    pub(crate) fn new(target: ExecutionTarget, map: MemoryMap, environment: D) -> Self {
        Self {
            map,
            environment,
            target,
            pending_functions: HostFunctions::new(),
            callbacks: Vec::new(),
            initial: Vec::new(),
        }
    }

    pub(crate) fn initialize_backing(
        &mut self,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        if !check_range(offset, bytes.len(), self.map.required_backing_bytes()) {
            return Err(MemoryError::BackingRange);
        }
        self.initial.push((offset, bytes.to_vec()));
        Ok(())
    }

    /// Publishes the configured map and consumes initial backing writes in order.
    /// The backend must first allocate all backing required by [`Self::memory_map`].
    pub fn initialize_memory(&mut self, memory: &mut dyn GuestMemory) {
        self.publish_map(memory);
        for (offset, bytes) in self.initial.drain(..) {
            memory.write(offset, &bytes);
        }
    }

    pub fn read_port(&mut self, memory: &mut dyn GuestMemory, port: u16, bytes: u8) -> u32 {
        self.access(memory, |environment, memory| {
            environment.read_port(memory, port, bytes)
        })
    }

    pub fn write_port(&mut self, memory: &mut dyn GuestMemory, port: u16, bytes: u8, value: u32) {
        self.access(memory, |environment, memory| {
            environment.write_port(memory, port, bytes, value)
        })
    }

    pub fn read_mmio(&mut self, memory: &mut dyn GuestMemory, address: u32, bytes: u8) -> u64 {
        self.access(memory, |environment, memory| {
            environment.read_mmio(memory, address, bytes)
        })
    }

    pub fn write_mmio(
        &mut self,
        memory: &mut dyn GuestMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        self.access(memory, |environment, memory| {
            environment.write_mmio(memory, address, bytes, value)
        })
    }

    pub fn publish_map(&self, memory: &mut dyn GuestMemory) {
        memory.publish_map(&self.map.to_bytes());
    }

    pub fn access<R>(
        &mut self,
        backing: &mut dyn GuestMemory,
        access: impl FnOnce(&mut D, &mut MemoryAccess<'_>) -> R,
    ) -> R {
        let mut changed = false;
        let result = access(
            &mut self.environment,
            &mut MemoryAccess {
                backing,
                map: &mut self.map,
                changed: &mut changed,
            },
        );
        if changed {
            self.publish_map(backing);
        }
        result
    }
}
