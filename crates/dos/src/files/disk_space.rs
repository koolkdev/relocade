//! Advisory DOS capacity and geometry derived from logical file sizes.

use super::FileSystem;
use crate::DosError;
use std::io::{self, ErrorKind};

pub(crate) const BYTES_PER_SECTOR: u16 = 512;
const DEFAULT_CAPACITY: u64 = 512 * 1024 * 1024;
const MAX_CLUSTER_BYTES: u64 = 32 * 1024;

pub(crate) struct DiskSpace {
    pub sectors_per_cluster: u16,
    pub free_clusters: u16,
    pub total_clusters: u16,
}

impl FileSystem {
    pub(crate) fn set_drive_capacity(&mut self, letter: char, capacity: u64) -> io::Result<()> {
        if !letter.is_ascii_alphabetic() || capacity > u64::from(u16::MAX) * MAX_CLUSTER_BYTES {
            return Err(ErrorKind::InvalidInput.into());
        }
        self.drives.get(letter).ok_or(ErrorKind::NotFound)?;
        self.drive_capacities
            .insert(letter.to_ascii_uppercase(), capacity);
        Ok(())
    }

    pub(crate) fn disk_space(&self, number: u8) -> Result<DiskSpace, DosError> {
        let letter = self.query_drive(number)?;
        let capacity = self
            .drive_capacities
            .get(&letter)
            .copied()
            .unwrap_or(DEFAULT_CAPACITY);
        // AH=36h returns 16-bit counts. Choose the smallest power-of-two cluster
        // size that fits the capacity, then discard any incomplete final cluster.
        let mut sectors_per_cluster = 1u16;
        let mut cluster_bytes = u64::from(BYTES_PER_SECTOR);
        while capacity / cluster_bytes > u64::from(u16::MAX) {
            sectors_per_cluster *= 2;
            cluster_bytes *= 2;
        }
        let total_clusters = capacity / cluster_bytes;
        // Round the aggregate logical size once; this does not model FAT allocation.
        let used_clusters = self
            .drives
            .get(letter)
            .unwrap()
            .used_bytes()
            .div_ceil(cluster_bytes);
        Ok(DiskSpace {
            sectors_per_cluster,
            total_clusters: total_clusters as u16,
            free_clusters: total_clusters.saturating_sub(used_clusters) as u16,
        })
    }
}
