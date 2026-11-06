use super::Storage;
#[cfg(feature = "renderer")]
use crate::memory::BLOCKS;
use crate::memory::{VGA_MEMORY_BYTES, VIDEO_MEMORY_BYTES};
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_addresses_interleave_planes_and_reach_extended_memory() {
    let mut memory = Storage::default();
    let writes = [
        (0, 0x11),
        (1, 0x22),
        (2, 0x33),
        (3, 0x44),
        (4, 0x55),
        (0x3ffff, 0x66),
        (0x40000, 0x77),
        (0x3fffff, 0x88),
    ];
    for (address, value) in writes {
        assert_eq!(memory.read(address), 0);
        memory.write(address, value);
    }
    for (address, value) in writes {
        assert_eq!(memory.read(address), value);
    }
    let snapshot = memory.snapshot(VGA_MEMORY_BYTES);
    let planes = &snapshot.blocks;
    assert_eq!(&planes[0][..2], &[0x11, 0x55]);
    assert_eq!(planes[1][0], 0x22);
    assert_eq!(planes[2][0], 0x33);
    assert_eq!(planes[3][0], 0x44);
    assert_eq!(planes[3][0xffff], 0x66);
    assert_eq!(memory.read(0x3fffe), 0);
    assert_eq!(memory.read(0x40001), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn snapshots_share_unchanged_planes_and_retain_previous_bytes() {
    let mut memory = Storage::default();
    memory.write(9, 0xa5);
    let before = memory.snapshot(VIDEO_MEMORY_BYTES).blocks;
    memory.write(9, 0xa5);
    memory.write(0x40000, 0x5a);
    let unchanged = memory.snapshot(VIDEO_MEMORY_BYTES).blocks;
    for plane in 0..4 {
        assert!(Arc::ptr_eq(&before[plane], &unchanged[plane]));
    }
    assert_eq!(before[4][0], 0);
    assert_eq!(unchanged[4][0], 0x5a);
    assert!(!Arc::ptr_eq(&before[4], &unchanged[4]));
    memory.write(9, 0x5a);
    let after = memory.snapshot(VIDEO_MEMORY_BYTES).blocks;
    assert_eq!(before[1][2], 0xa5);
    assert_eq!(after[1][2], 0x5a);
    for plane in [0, 2, 3] {
        assert!(Arc::ptr_eq(&before[plane], &after[plane]));
    }
    drop(memory);
    assert_eq!(before[1][2], 0xa5);
    assert_eq!(unchanged[4][0], 0x5a);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn snapshot_extents_cover_whole_interleaved_groups() {
    let mut memory = Storage::default();
    memory.write(0x40000, 0x5a);
    memory.write(0x3fffff, 0xa5);
    let vga = memory.snapshot(0x40000);
    let extended = memory.snapshot(0x40001);
    let full = memory.snapshot(0x400000);
    assert_eq!(vga.blocks.len(), 4);
    assert_eq!(extended.blocks.len(), 8);
    assert_eq!(full.blocks.len(), 64);
    assert!(!vga.same_blocks(&extended));
    assert!(!extended.same_blocks(&full));
    assert!(extended.same_blocks(&memory.snapshot(0x80000)));
    drop(memory);
    assert_eq!(extended.blocks[4][0], 0x5a);
    assert_eq!(full.blocks[63][0xffff], 0xa5);
}

#[cfg(feature = "renderer")]
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_writes_track_distinct_pages_throughout_physical_memory() {
    let mut memory = Storage::default();
    memory.write(0x1003, 1); // Plane 3, byte 1024.
    memory.write(0x3ffff, 2); // Plane 3, byte 65535.
    memory.write(0x40000, 3);
    memory.write(0x3fffff, 4);
    assert_eq!(memory.changed_pages().ranges(0).count(), 0);
    assert_eq!(
        memory.changed_pages().ranges(3).collect::<Vec<_>>(),
        vec![1024..2048, 64512..65536]
    );
    assert_eq!(
        memory.changed_pages().ranges(4).collect::<Vec<_>>(),
        vec![0..1024]
    );
    assert_eq!(
        memory.changed_pages().ranges(63).collect::<Vec<_>>(),
        vec![64512..65536]
    );
    memory.clear_changes();
    memory.write(0x1003, 1);
    memory.write(0x40000, 3);
    memory.write(0x3fffff, 4);
    assert_eq!(memory.changed_pages().ranges(3).count(), 0);
    assert_eq!(memory.changed_pages().ranges(4).count(), 0);
    assert_eq!(memory.changed_pages().ranges(63).count(), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clearing_vram_preserves_snapshots_and_invalidates_every_physical_block() {
    let mut memory = Storage::default();
    memory.write(0x3ffff, 0xa5);
    memory.write(0x3fffff, 0x5a);
    let before = memory.snapshot(VIDEO_MEMORY_BYTES).blocks;
    #[cfg(feature = "renderer")]
    memory.clear_changes();
    memory.clear();
    assert_eq!(memory.read(0x3ffff), 0);
    assert_eq!(memory.read(0x3fffff), 0);
    assert_eq!(before[3][0xffff], 0xa5);
    assert_eq!(before[63][0xffff], 0x5a);
    #[cfg(feature = "renderer")]
    for plane in 0..BLOCKS {
        assert_eq!(
            memory.changed_pages().ranges(plane).collect::<Vec<_>>(),
            vec![0..65536]
        );
    }
}
