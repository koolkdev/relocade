//! VGA plane inspection over a snapshot's physical memory.

use relocade_vga::DisplaySnapshot;
use std::ops::Range;

pub trait SnapshotPlanes {
    fn plane_byte(&self, plane: usize, address: usize) -> u8;
    fn plane_bytes(
        &self,
        plane: usize,
        range: Range<usize>,
    ) -> impl ExactSizeIterator<Item = u8> + '_;
}

impl SnapshotPlanes for DisplaySnapshot {
    fn plane_byte(&self, plane: usize, address: usize) -> u8 {
        assert!(plane < 4 && address < 65536, "invalid VGA plane address");
        self.vram_byte((address * 4 + plane) as u32)
            .expect("captured VGA plane byte")
    }

    fn plane_bytes(
        &self,
        plane: usize,
        range: Range<usize>,
    ) -> impl ExactSizeIterator<Item = u8> + '_ {
        assert!(
            plane < 4 && range.start <= range.end && range.end <= 65536,
            "invalid VGA plane range"
        );
        range.map(move |address| self.plane_byte(plane, address))
    }
}
