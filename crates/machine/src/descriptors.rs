//! Machine-owned descriptor bytes, separate from the guest memory address space.

use crate::MachineStorage;
use relocade_x86::{
    CpuState, Descriptor, DescriptorTables, Exception, Segment, SegmentDescriptorInfo,
    StoredSegment, StoredSystemSegment, StoredTableRegister,
};
use std::{fmt, mem::offset_of};

/// Initial image of the dedicated descriptor address space. CPU table bases are
/// byte offsets in this space, not guest linear addresses. There is no guest
/// page translation or guest access to these bytes.
pub struct DescriptorMemory {
    bytes: Vec<u8>,
}

impl Default for DescriptorMemory {
    fn default() -> Self {
        Self {
            bytes: vec![0; Self::BYTE_LEN as usize],
        }
    }
}

impl DescriptorMemory {
    pub const BYTE_LEN: u32 = 2 * 65536;

    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn install(
        &mut self,
        cpu: &mut CpuState,
        tables: DescriptorTables,
        ldt_selector: Option<u16>,
    ) -> Result<(), DescriptorError> {
        let [mut global, local] = tables.to_packed_tables();
        let ldtr = if let Some(selector) = ldt_selector {
            if selector & !3 == 0 || selector & 4 != 0 {
                return Err(DescriptorError::InvalidLdtSelector);
            }
            if tables.get(selector).is_some() {
                return Err(DescriptorError::OccupiedLdtSlot);
            }
            let offset = usize::from(selector & !7);
            global.resize(global.len().max(offset + 8), 0);
            let descriptor = Descriptor::ldt(65536, u16::MAX);
            global[offset..offset + 8].copy_from_slice(&descriptor.to_bytes());
            StoredSystemSegment {
                base: 65536,
                limit: u32::from(u16::MAX),
                selector,
                usable: 1,
                access_rights: descriptor.access_rights(),
                ..StoredSystemSegment::default()
            }
        } else {
            if local.iter().any(|&byte| byte != 0) {
                return Err(DescriptorError::MissingLdtSelector);
            }
            StoredSystemSegment::default()
        };
        self.bytes.fill(0);
        self.bytes[..global.len()].copy_from_slice(&global);
        self.bytes[65536..65536 + local.len()].copy_from_slice(&local);
        cpu.gdtr = StoredTableRegister::new(0, u16::MAX);
        cpu.ldtr = ldtr;
        Ok(())
    }
}

/// Direct access to descriptor storage and lookup through the current CPU table
/// registers. Only GDTR/LDTR are read, so runtime resolution does not require
/// publishing the instruction's other CPU state.
pub struct Descriptors<'a, S: MachineStorage + ?Sized = dyn MachineStorage + 'a> {
    storage: &'a mut S,
}

/// A checked segment cache and an optional descriptor access-byte update.
/// The update is committed only after the instruction's remaining guards pass.
pub struct ResolvedDescriptor {
    pub cache: StoredSegment,
    pub accessed_byte: Option<u32>,
}

impl<'a, S: MachineStorage + ?Sized> Descriptors<'a, S> {
    pub fn new(storage: &'a mut S) -> Self {
        Self { storage }
    }

    /// Reads a bounded packed descriptor without consulting table registers.
    pub fn read_at(&self, address: u32) -> Result<Descriptor, DescriptorError> {
        check_address(address)?;
        let mut bytes = [0; 8];
        self.storage.read_descriptors(address, &mut bytes);
        Ok(Descriptor::from_bytes(bytes))
    }

    /// Edits bytes only; table limits and all loaded caches remain unchanged.
    pub fn write_at(
        &mut self,
        address: u32,
        descriptor: impl Into<Descriptor>,
    ) -> Result<(), DescriptorError> {
        check_address(address)?;
        self.storage
            .write_descriptors(address, &descriptor.into().to_bytes());
        Ok(())
    }

    pub fn read(&self, selector: u16) -> Option<Descriptor> {
        self.address(selector)
            .and_then(|address| self.read_at(address).ok())
    }

    /// Edits a slot within the active table's bounds. RPL does not select a slot.
    pub fn write(
        &mut self,
        selector: u16,
        descriptor: impl Into<Descriptor>,
    ) -> Result<(), DescriptorError> {
        let address = self
            .address(selector)
            .ok_or(DescriptorError::OutsideTable)?;
        self.write_at(address, descriptor)
    }

    pub fn query_user_segment_descriptor(&self, selector: u16) -> SegmentDescriptorInfo {
        self.read(selector)
            .map(Descriptor::query_user)
            .unwrap_or_default()
    }

    pub fn resolve_user_segment(
        &self,
        destination: Segment,
        selector: u16,
    ) -> Result<ResolvedDescriptor, Exception> {
        let address = self.address(selector);
        let descriptor = address
            .and_then(|address| self.read_at(address).ok())
            .unwrap_or_default();
        let cache = descriptor.resolve_user_segment(destination, selector)?;
        Ok(ResolvedDescriptor {
            cache,
            accessed_byte: address
                .filter(|_| descriptor.needs_accessed_write())
                .map(|address| address + 5),
        })
    }

    /// Completes a previously resolved load. The offset must come from
    /// `ResolvedDescriptor::accessed_byte` for this machine. Descriptor storage
    /// stays stable during an execution entry, so this operation cannot fault.
    pub fn mark_accessed(&mut self, address: u32) {
        assert!(
            address < DescriptorMemory::BYTE_LEN,
            "resolved descriptor byte must fit storage"
        );
        let mut byte = [0];
        self.storage.read_descriptors(address, &mut byte);
        byte[0] |= 1;
        self.storage.write_descriptors(address, &byte);
    }

    fn address(&self, selector: u16) -> Option<u32> {
        if selector & !3 == 0 {
            return None;
        }
        let (base, limit) = if selector & 4 == 0 {
            let mut bytes = [0; StoredTableRegister::BYTE_LEN];
            self.storage
                .read_cpu(offset_of!(CpuState, gdtr) as u32, &mut bytes);
            let table = StoredTableRegister::from_bytes(bytes);
            (table.base, u32::from(table.limit))
        } else {
            let mut bytes = [0; StoredSystemSegment::BYTE_LEN];
            self.storage
                .read_cpu(offset_of!(CpuState, ldtr) as u32, &mut bytes);
            let table = StoredSystemSegment::from_bytes(bytes);
            if table.usable == 0 {
                return None;
            }
            (table.base, table.limit)
        };
        let offset = u32::from(selector & !7);
        if offset + 7 > limit {
            return None;
        }
        let address = base.checked_add(offset)?;
        check_address(address).ok()?;
        Some(address)
    }
}

fn check_address(address: u32) -> Result<(), DescriptorError> {
    if address > DescriptorMemory::BYTE_LEN - 8 {
        return Err(DescriptorError::OutsideStorage);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    OutsideStorage,
    OutsideTable,
    InvalidLdtSelector,
    OccupiedLdtSlot,
    MissingLdtSelector,
    RealMode,
}
impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OutsideStorage => "descriptor exceeds dedicated storage",
            Self::OutsideTable => "selector is outside the active descriptor table",
            Self::InvalidLdtSelector => "LDTR requires a nonnull GDT selector",
            Self::OccupiedLdtSlot => "LDTR selector would overwrite a GDT descriptor",
            Self::MissingLdtSelector => "local descriptors require an LDTR selector",
            Self::RealMode => "descriptor table installation requires protected user mode",
        })
    }
}
impl std::error::Error for DescriptorError {}
