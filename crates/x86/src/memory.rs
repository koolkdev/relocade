mod access;
mod accesses;
mod page_table;
mod transfer;
mod update;
mod virtual_memory;

pub(crate) use access::Access;
pub(crate) use accesses::Accesses;
pub(crate) use page_table::{PageCache, PageCacheInputs};

use crate::{alu::OperandUpdate, ExecutionProfile};
use access::FaultHandler;
use page_table::{PRESENT, WRITABLE};
use virtual_memory::VirtualMemory;
use wasm86_compiler::{BlockBuilder, BuildError, MemoryInt, Program, Val, I32};

/// Selects the generated memory model once, during module construction.
pub(crate) enum Memory {
    Virtual(VirtualMemory),
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
    pub(crate) fn declare(
        program: &mut Program,
        profile: ExecutionProfile,
    ) -> Result<Self, BuildError> {
        match profile {
            ExecutionProfile::Protected(_) => VirtualMemory::declare(program).map(Self::Virtual),
        }
    }
    /// Resolves a complete segment-checked span, optionally reporting architectural faults.
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
        }
    }

    pub(crate) fn read<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Virtual(memory) => memory.read(body, access, offset),
        }
    }

    pub(crate) fn write<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Virtual(memory) => memory.write(body, access, offset, value),
        }
    }

    pub(crate) fn atomic_update<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        update: &OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Virtual(memory) => memory.atomic_update(body, access, update),
        }
    }

    pub(crate) fn load<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        match self {
            Self::Virtual(memory) => memory.load(body, backing, offset),
        }
    }

    pub(crate) fn store<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        backing: &Val<I32>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Virtual(memory) => memory.store(body, backing, value),
        }
    }
}

#[cfg(test)]
mod tests;
