mod access;
mod accesses;
mod bulk;
mod page_table;
mod physical;
mod physical_map;
mod transfer;
mod update;
mod value;
mod virtual_memory;

pub(crate) use access::Access;
pub(crate) use accesses::Accesses;
pub(crate) use page_table::{PageCache, PageCacheInputs};
pub use physical_map::{PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};
pub(crate) use value::TransferType;

use crate::{alu::OperandUpdate, ExecutionProfile};
use access::FaultHandler;
use page_table::{PRESENT, WRITABLE};
use physical::PhysicalMemory;
use virtual_memory::VirtualMemory;
use wasm86_compiler::{BlockBuilder, BuildError, MemoryInt, Program, Val, I1, I32};

/// Selects the generated memory model once, during module construction.
pub(crate) enum Memory {
    Virtual(VirtualMemory),
    Physical(PhysicalMemory),
}

/// A non-faulting probe result. Backing is usable only when the span is available.
pub(crate) struct DirectRange {
    pub(crate) unavailable: Val<I1>,
    pub(crate) physical: Val<I32>,
}

#[derive(Clone, Copy)]
pub(super) enum Intent {
    Fetch,
    Read,
    Write,
}

impl Intent {
    fn required_permissions(self) -> u32 {
        match self {
            Self::Fetch | Self::Read => PRESENT,
            Self::Write => PRESENT | WRITABLE,
        }
    }
    fn base_error_code(self) -> u32 {
        match self {
            Self::Fetch => 16,
            Self::Read => 0,
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

    pub(crate) fn declare(
        program: &mut Program,
        profile: ExecutionProfile,
    ) -> Result<Self, BuildError> {
        match profile {
            ExecutionProfile::Protected(_) => VirtualMemory::declare(program).map(Self::Virtual),
            ExecutionProfile::Real16 => Ok(Self::Physical(PhysicalMemory::declare(program))),
        }
    }
    /// Probes direct backing after the caller's eligibility check. A true
    /// `denied` condition skips lookup and preserves the cache without faulting.
    /// With `None`, the caller has already established the span's eligibility.
    pub(crate) fn check_direct_access(
        &self,
        body: &mut BlockBuilder<'_>,
        start: &Val<I32>,
        bytes: u32,
        intent: Intent,
        denied: Option<Val<I1>>,
        cache: Option<&mut PageCache>,
    ) -> Result<DirectRange, BuildError> {
        let probe = |body: &mut BlockBuilder<'_>, cache: Option<&mut PageCache>| match self {
            Self::Virtual(memory) => {
                let access = memory.resolve_access(body, start, bytes, intent, cache, None)?;
                Ok(DirectRange {
                    unavailable: access.unavailable,
                    physical: access.physical,
                })
            }
            // MMIO callbacks can change routing during an entry. Physical
            // lookups always read live metadata rather than the loop page cache.
            Self::Physical(memory) => memory.check_direct_access(body, start, bytes, intent),
        };
        let Some(denied) = denied else {
            return probe(body, cache);
        };
        let mut next_cache = cache.as_deref().cloned();
        let initial_cache = next_cache
            .clone()
            .map(PageCache::into_inputs)
            .unwrap_or_else(|| PageCache::EMPTY.map(Into::into));
        let (unavailable, physical, cache_inputs) = body.if_value::<(I1, I32, PageCacheInputs)>(
            denied,
            |denied| denied.yield_((true, 0, initial_cache.clone())),
            |mut allowed| {
                let direct = probe(&mut allowed, next_cache.as_mut())?;
                let inputs = next_cache
                    .map(PageCache::into_inputs)
                    .unwrap_or_else(|| initial_cache.clone());
                allowed.yield_((direct.unavailable, direct.physical, inputs))
            },
        )?;
        if let Some(cache) = cache {
            *cache = PageCache::from_inputs(cache_inputs);
        }
        Ok(DirectRange {
            unavailable,
            physical,
        })
    }

    /// Resolves a segment-checked span. Physical routing remains live until each transfer.
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
            Self::Physical(_) => {
                let bytes = body.value(bytes)?;
                let constant_bytes = body.constant_bits(&bytes)?.map(|bytes| bytes as u32);
                if on_fault.is_some() {
                    assert_ne!(constant_bytes, Some(0), "a faulting span must be nonempty");
                }
                Ok(Access {
                    linear: start.clone(),
                    physical: body.value(0)?,
                    denied: bytes.eq(0),
                    unavailable: body.value(true)?,
                    intent,
                    constant_bytes,
                })
            }
        }
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

    pub(crate) fn load<T: wasm86_compiler::MemoryType>(
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

    pub(crate) fn store<T: wasm86_compiler::MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Virtual(memory) => memory.store(body, backing, value),
            Self::Physical(memory) => memory.store(body, backing, value),
        }
    }

    pub(crate) fn copy(
        &self,
        body: &mut BlockBuilder<'_>,
        destination: &Val<I32>,
        source: &Val<I32>,
        bytes: &Val<I32>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Virtual(memory) => memory.copy(body, destination, source, bytes),
            Self::Physical(_) => unreachable!("bulk transfers require stable mappings"),
        }
    }

    pub(crate) fn fill<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        destination: &Val<I32>,
        value: &Val<T>,
        bytes: &Val<I32>,
    ) -> Result<(), BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        match self {
            Self::Virtual(memory) => memory.fill(body, destination, value, bytes),
            Self::Physical(_) => unreachable!("bulk transfers require stable mappings"),
        }
    }
}

#[cfg(test)]
mod tests;
