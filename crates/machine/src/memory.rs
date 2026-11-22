pub(crate) mod code;
pub use code::{CodeSource, InstalledCode};
mod paging;
pub use paging::FlatPageTable;
mod resolve;
mod view;
pub use relocade_x86::real_mode::FarPointer;
pub use view::MemoryView;

use crate::{ConfigurationError, CpuConfig, IoBus, MachineConfig, Result};
use relocade_wasm_runtime::SharedMemory;
use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
/// Devices belong to the execution worker and are not retained by this handle.
#[derive(Clone, crate::worker::Transfer)]
pub struct Memory {
    config: Arc<MachineConfig>,
    backing: SharedMemory,
    page_table: Option<FlatPageTable>,
    descriptors: Option<Arc<Mutex<relocade_x86::DescriptorTables>>>,
    code: Arc<code::CodeRegistry>,
    // Only a PC address space has exclusive CPU ownership. Its device routing
    // and generated-code write barriers belong to the attached CPU.
    pc_cpu: Option<Arc<Mutex<Weak<CpuConfig>>>>,
}

/// Binds one CPU configuration to its memory. On a PC, a prepared handle or
/// live runtime keeps the sole CPU attachment reserved.
#[derive(Clone, crate::worker::Transfer)]
pub struct CpuBinding {
    pub(crate) memory: Memory,
    pub(crate) config: Arc<CpuConfig>,
}

impl Memory {
    /// Allocates zeroed guest backing with the supplied address policy.
    pub fn new(config: MachineConfig) -> Result<Self> {
        let minimum = config.memory_size().div_ceil(65536).max(1) as u32;
        let maximum = relocade_x86::abi::GUEST
            .maximum
            .expect("guest memory has a maximum");
        let backing = SharedMemory::new(minimum, maximum)?;
        let pc_cpu = config.physical_map().map(|_| Arc::default());
        let page_table = if config.physical_map().is_none() {
            Some(FlatPageTable::new(config.memory_size())?)
        } else {
            None
        };
        let descriptors = page_table
            .as_ref()
            .map(|_| Arc::new(Mutex::new(relocade_x86::DescriptorTables::flat32())));
        Ok(Self {
            descriptors,
            page_table,
            config: Arc::new(config),
            backing,
            code: Arc::default(),
            pc_cpu,
        })
    }

    pub(crate) fn attach_cpu(&self, config: CpuConfig) -> Result<CpuBinding> {
        self.configuration()
            .validate_target(config.execution_target())?;
        let config = Arc::new(config);
        if let Some(pc_cpu) = &self.pc_cpu {
            let mut cpu = pc_cpu.lock().unwrap();
            if cpu.strong_count() != 0 {
                return Err(ConfigurationError::CpuAlreadyAttached.into());
            }
            *cpu = Arc::downgrade(&config);
        }
        Ok(CpuBinding {
            memory: self.clone(),
            config,
        })
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }

    /// The flat address space's default descriptor tables, shared by its CPUs.
    /// Drop the guard before resuming CPUs. Edits affect subsequent queries and
    /// reloads. A CPU can instead supply a private
    /// thread view through [`crate::CpuStorage::with_descriptor_tables`].
    pub fn descriptor_tables(&self) -> Option<MutexGuard<'_, relocade_x86::DescriptorTables>> {
        self.descriptors
            .as_ref()
            .map(|tables| tables.lock().unwrap())
    }

    pub(crate) fn shared_descriptors(&self) -> Option<&Arc<Mutex<relocade_x86::DescriptorTables>>> {
        self.descriptors.as_ref()
    }

    /// Shared host-managed mappings for a flat address space.
    pub fn page_table(&self) -> Option<&FlatPageTable> {
        self.page_table.as_ref()
    }

    /// Raw Wasm storage. Serialize direct writes with code installation. When
    /// changing executable bytes, stop the PC and retire affected installations
    /// before resuming it. Writes through [`MemoryView`] synchronize with
    /// installation and record modified instructions.
    pub fn backing(&self) -> &SharedMemory {
        &self.backing
    }

    /// Retains this memory and its execution worker's device bus together.
    /// Keep raw [`Memory`] handles for sharing backing storage between workers.
    pub fn with_io(self, io: IoBus) -> MemoryBus {
        MemoryBus { memory: self, io }
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    /// For MMIO, retain device routing with [`Self::with_io`], or borrow a bus
    /// for this view with [`MemoryView::with_io`].
    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(self)
    }
}

/// Retained access to guest memory and devices on one execution worker.
///
/// Clones share backing bytes and device connections independently of a CPU.
/// Views borrow this handle and may cross awaits. Devices are borrowed only
/// for individual transfers. The bus retains its runtime for synchronous I/O,
/// including after CPU drop. Device state stays on the execution worker.
#[derive(Clone)]
pub struct MemoryBus {
    memory: Memory,
    io: IoBus,
}

impl MemoryBus {
    /// Borrows a live memory view with its device routing already attached.
    pub fn view(&self) -> MemoryView<'_> {
        self.memory.view().with_io(&self.io)
    }

    /// The same device connections used by this handle's memory views.
    pub fn io(&self) -> &IoBus {
        &self.io
    }
}

fn range_fits(address: u32, len: usize, limit: u64) -> bool {
    u64::from(address)
        .checked_add(len as u64)
        .is_some_and(|end| end <= limit)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressRange,
    UnalignedMapping,
    MappingInUse,
    Translation(relocade_x86::Exception),
    BackingRange,
    SegmentRange,
    NotRamOrRom,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnalignedMapping => "mappings require whole aligned 4-KiB pages",
            Self::MappingInUse => "the page table is in use",
            Self::Translation(exception) => return exception.fmt(f),
            Self::AddressRange => "memory access exceeds the 32-bit guest address space",
            Self::BackingRange => "backing access exceeds memory_size",
            Self::SegmentRange => "memory access exceeds the segment limit",
            Self::NotRamOrRom => "instruction range includes MMIO or unmapped memory",
        })
    }
}

impl std::error::Error for MemoryError {}
