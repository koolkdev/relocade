//! Allocation accounting retained by the filesystem and its live files.

use std::{cell::Cell, io, io::ErrorKind, rc::Rc};

/// A snapshot of a volume's logical file-data allocation. Directory metadata
/// is not charged; each file occupies a whole number of allocation units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Space {
    /// Bytes per allocation unit.
    pub allocation_unit: u32,
    /// Configured capacity in whole allocation units.
    pub total_units: u64,
    /// Units not occupied by live file data.
    pub free_units: u64,
}

#[derive(Debug)]
pub(crate) struct Storage {
    allocation_unit: u32,
    total_units: u64,
    used_units: Cell<u64>,
}

/// One file owns one allocation, regardless of how many handles refer to it.
#[derive(Debug)]
pub(crate) struct Allocation {
    storage: Rc<Storage>,
    units: u64,
}

impl Storage {
    pub(crate) fn new(capacity: u64, allocation_unit: u32) -> io::Result<Self> {
        if allocation_unit == 0 || !capacity.is_multiple_of(u64::from(allocation_unit)) {
            return Err(ErrorKind::InvalidInput.into());
        }
        Ok(Self {
            allocation_unit,
            total_units: capacity / u64::from(allocation_unit),
            used_units: Cell::new(0),
        })
    }

    pub(crate) fn space(&self) -> Space {
        Space {
            allocation_unit: self.allocation_unit,
            total_units: self.total_units,
            free_units: self.total_units - self.used_units.get(),
        }
    }

    pub(crate) fn allocate(self: &Rc<Self>, len: u64) -> io::Result<Allocation> {
        let mut allocation = Allocation {
            storage: self.clone(),
            units: 0,
        };
        allocation.resize(len)?;
        Ok(allocation)
    }
}

impl Allocation {
    pub(crate) fn max_len(&self) -> u64 {
        (self.units + self.storage.space().free_units) * u64::from(self.storage.allocation_unit)
    }

    pub(crate) fn resize(&mut self, len: u64) -> io::Result<()> {
        let units = len.div_ceil(u64::from(self.storage.allocation_unit));
        let other_units = self.storage.used_units.get() - self.units;
        if units > self.storage.total_units - other_units {
            return Err(ErrorKind::StorageFull.into());
        }
        self.storage.used_units.set(other_units + units);
        self.units = units;
        Ok(())
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        self.storage
            .used_units
            .set(self.storage.used_units.get() - self.units);
    }
}
