mod access;
mod accesses;
mod bulk;
mod masked;
pub mod page_table;
mod physical;
mod physical_map;
mod transfer;
mod update;
mod value;
mod virtual_memory;

pub(crate) use access::Access;
pub(crate) use accesses::Accesses;
pub(crate) use page_table::{PageCache, PageCacheInputs};
pub use physical_map::{MmioHandlerId, PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};
pub(crate) use value::TransferType;

use crate::{alu::OperandUpdate, ExecutionProfile};
use access::FaultHandler;
use page_table::{PRESENT, WRITABLE};
use physical::PhysicalMemory;
use relocade_compiler::{
    BlockBuilder, BuildError, Mem, MemoryInt, MemoryType, Program, Val, I1, I32,
};
use virtual_memory::VirtualMemory;

/// Selects the generated memory model once, during module construction.
pub(crate) enum Memory {
    Virtual(VirtualMemory),
    Physical(PhysicalMemory),
}

#[derive(Clone, Copy)]
pub(super) enum Intent {
    Fetch,
    Read,
    /// Data-read paging checks, with access to execute-only code segments too.
    ReadOrExecute,
    Write,
}

impl Intent {
    fn required_permissions(self) -> u32 {
        match self {
            Self::Fetch | Self::Read | Self::ReadOrExecute => PRESENT,
            Self::Write => PRESENT | WRITABLE,
        }
    }
    fn base_error_code(self) -> u32 {
        match self {
            Self::Fetch => 16,
            Self::Read | Self::ReadOrExecute => 0,
            Self::Write => 2,
        }
    }
}

impl Memory {
    /// Virtual mappings stay fixed during an entry. Physical MMIO callbacks can
    /// remap backing, so their execution paths cannot retain mapping proofs.
    pub(crate) fn has_stable_mappings(&self) -> bool {
        matches!(self, Self::Virtual(_))
    }

    /// Declares one module's memory handles and deferred access helpers.
    /// The compiler constructs and retains only reachable helper bodies.
    pub(crate) fn declare(program: &mut Program, profile: ExecutionProfile) -> Self {
        match profile {
            ExecutionProfile::Protected(_) => Self::Virtual(VirtualMemory::declare(program)),
            ExecutionProfile::Real16 => Self::Physical(PhysicalMemory::declare(program)),
        }
    }

    /// Non-faulting access query after the caller's span eligibility checks.
    /// A denied span skips lookup; otherwise the complete result can be reused.
    pub(crate) fn check_access(
        &self,
        body: &mut BlockBuilder<'_>,
        start: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
        denied: Option<Val<I1>>,
        cache: Option<&mut PageCache>,
    ) -> Result<Access, BuildError> {
        let bytes = body.value(bytes)?;
        let Some(denied) = denied else {
            return self.resolve_access(body, start, bytes, intent, cache, None);
        };
        let constant_bytes = body.constant_bits(&bytes)?.map(|bytes| bytes as u32);
        let mut next_cache = cache.as_deref().cloned();
        let initial_cache = next_cache
            .clone()
            .map(PageCache::into_inputs)
            .unwrap_or_else(|| PageCache::EMPTY.map(Into::into));
        let (denied, unavailable, physical, cache_inputs) = body
            .if_value::<(I1, I1, I32, PageCacheInputs)>(
                denied,
                |denied| denied.yield_((true, true, 0, initial_cache.clone())),
                |mut allowed| {
                    let access = self.resolve_access(
                        &mut allowed,
                        start,
                        &bytes,
                        intent,
                        next_cache.as_mut(),
                        None,
                    )?;
                    let inputs = next_cache
                        .map(PageCache::into_inputs)
                        .unwrap_or_else(|| initial_cache.clone());
                    allowed.yield_((access.denied, access.unavailable, access.physical, inputs))
                },
            )?;
        if let Some(cache) = cache {
            *cache = PageCache::from_inputs(cache_inputs);
        }
        Ok(Access {
            linear: start.clone(),
            physical,
            denied,
            unavailable,
            intent,
            constant_bytes,
        })
    }

    /// Resolves a segment-checked span. Without a fault handler this is a probe.
    /// Retained physical mappings must not cross callbacks that can change them.
    pub(crate) fn resolve_access(
        &self,
        body: &mut BlockBuilder<'_>,
        start: &Val<I32>,
        bytes: impl Into<Val<I32>>,
        intent: Intent,
        cache: Option<&mut PageCache>,
        on_fault: Option<&mut FaultHandler<'_>>,
    ) -> Result<Access, BuildError> {
        match self {
            Self::Virtual(memory) => {
                memory.resolve_access(body, start, bytes, intent, cache, on_fault)
            }
            Self::Physical(memory) => memory.resolve_access(body, start, bytes, intent),
        }
    }

    /// Reuses a successful span probe. A denied probe checks this field alone,
    /// so a later field's fault cannot erase earlier architectural progress.
    pub(crate) fn resolve_field<T: MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
        on_fault: &mut FaultHandler<'_>,
    ) -> Result<Access, BuildError> {
        let mut field = access.field::<T>(offset)?;
        let (unavailable, physical) = body.if_value::<(I1, I32)>(
            &access.denied,
            |mut denied| {
                let checked = self.resolve_access(
                    &mut denied,
                    &field.linear,
                    T::BYTES,
                    field.intent,
                    None,
                    Some(on_fault),
                )?;
                denied.yield_((checked.unavailable, checked.physical))
            },
            |allowed| allowed.yield_((&field.unavailable, &field.physical)),
        )?;
        field.denied = false.into();
        field.unavailable = unavailable;
        field.physical = physical;
        Ok(field)
    }

    /// Architectural checks have already passed; only live routing can change
    /// across a device callback. Virtual mappings remain fixed during an entry.
    pub(crate) fn refresh_access(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
    ) -> Result<Access, BuildError> {
        if self.has_stable_mappings() {
            return Ok(access.clone());
        }
        self.resolve_access(
            body,
            &access.linear,
            access.constant_bytes.expect("a transfer has a fixed width"),
            access.intent,
            None,
            None,
        )
    }

    pub(crate) fn read<T: TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Virtual(memory) => memory.read(body, access, offset),
            Self::Physical(memory) => memory.read(body, access, offset),
        }
    }

    pub(crate) fn write<T: TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Virtual(memory) => memory.write(body, access, offset, value),
            Self::Physical(memory) => memory.write(body, access, offset, value),
        }
    }

    pub(crate) fn atomic_update<T: MemoryInt + TransferType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        update: &OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Virtual(memory) => memory.atomic_update(body, access, update),
            Self::Physical(memory) => memory.atomic_update(body, access, update),
        }
    }

    pub(crate) fn load<T: MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Virtual(memory) => memory.load(body, backing, offset),
            Self::Physical(memory) => memory.load(body, backing, offset),
        }
    }

    fn backing(&self) -> Mem {
        match self {
            Self::Virtual(memory) => memory.guest,
            Self::Physical(memory) => memory.backing(),
        }
    }
}

#[cfg(test)]
mod tests;
