use super::Storage;
#[cfg(feature = "renderer")]
use crate::memory::BLOCKS;
use crate::memory::{VGA_MEMORY_BYTES, VIDEO_MEMORY_BYTES};
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_addresses_follow_the_linear_backing_through_extended_memory() {
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
    let snapshot = memory.snapshot(VGA_MEMORY_BYTES, None);
    assert_eq!(&snapshot.blocks[0][..5], &[0x11, 0x22, 0x33, 0x44, 0x55]);
    assert_eq!(snapshot.blocks[3][0xffff], 0x66);
    assert_eq!(memory.read(0x3fffe), 0);
    assert_eq!(memory.read(0x40001), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn snapshots_share_untouched_blocks_and_retain_previous_bytes() {
    let mut memory = Storage::default();
    memory.write(9, 0xa5);
    let before = memory.snapshot(VIDEO_MEMORY_BYTES, None);
    assert!(before.same_blocks(&before.clone()));
    memory.clear_dirty();
    memory.write(9, 0xa5);
    memory.write(0x40000, 0x5a);
    let rewritten = memory.snapshot(VIDEO_MEMORY_BYTES, Some(&before));
    memory.clear_dirty();
    assert!(!Arc::ptr_eq(&before.blocks[0], &rewritten.blocks[0]));
    assert_eq!(before.blocks[0], rewritten.blocks[0]);
    for block in 1..4 {
        assert!(Arc::ptr_eq(&before.blocks[block], &rewritten.blocks[block]));
    }
    assert_eq!(before.blocks[4][0], 0);
    assert_eq!(rewritten.blocks[4][0], 0x5a);
    assert!(!Arc::ptr_eq(&before.blocks[4], &rewritten.blocks[4]));
    memory.write(9, 0x5a);
    let after = memory.snapshot(VIDEO_MEMORY_BYTES, Some(&rewritten));
    memory.clear_dirty();
    assert_eq!(before.blocks[0][9], 0xa5);
    assert_eq!(after.blocks[0][9], 0x5a);
    for block in 1..4 {
        assert!(Arc::ptr_eq(&before.blocks[block], &after.blocks[block]));
    }
    memory.write(9, 0xa5);
    memory.write(9, 0x5a);
    let restored = memory.snapshot(VIDEO_MEMORY_BYTES, Some(&after));
    assert!(!Arc::ptr_eq(&after.blocks[0], &restored.blocks[0]));
    assert_eq!(after.blocks[0], restored.blocks[0]);
    drop(memory);
    assert_eq!(before.blocks[0][9], 0xa5);
    assert_eq!(rewritten.blocks[4][0], 0x5a);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn snapshot_extents_cover_complete_allocation_blocks() {
    let mut memory = Storage::default();
    memory.write(0x40000, 0x5a);
    memory.write(0x3fffff, 0xa5);
    let vga = memory.snapshot(0x40000, None);
    memory.clear_dirty();
    let extended = memory.snapshot(0x40001, Some(&vga));
    memory.clear_dirty();
    let full = memory.snapshot(0x400000, Some(&extended));
    memory.clear_dirty();
    assert_eq!(vga.blocks.len(), 4);
    assert_eq!(extended.blocks.len(), 5);
    assert_eq!(full.blocks.len(), 64);
    assert!(!vga.same_blocks(&extended));
    assert!(!extended.same_blocks(&full));
    assert!(extended.same_blocks(&memory.snapshot(0x50000, Some(&full))));
    drop(memory);
    assert_eq!(extended.blocks[4][0], 0x5a);
    assert_eq!(full.blocks[63][0xffff], 0xa5);
}

#[cfg(feature = "renderer")]
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_writes_track_distinct_pages_throughout_physical_memory() {
    let mut memory = Storage::default();
    memory.write(0x1003, 1);
    memory.write(0x3ffff, 2);
    memory.write(0x40000, 3);
    memory.write(0x3fffff, 4);
    memory.snapshot(VIDEO_MEMORY_BYTES, None);
    assert_eq!(
        memory.dirty_pages().ranges(0).collect::<Vec<_>>(),
        vec![4096..5120]
    );
    assert_eq!(
        memory.dirty_pages().ranges(3).collect::<Vec<_>>(),
        vec![64512..65536]
    );
    assert_eq!(
        memory.dirty_pages().ranges(4).collect::<Vec<_>>(),
        vec![0..1024]
    );
    assert_eq!(
        memory.dirty_pages().ranges(63).collect::<Vec<_>>(),
        vec![64512..65536]
    );
    memory.clear_dirty();
    memory.write(0x1003, 1);
    memory.write(0x40000, 3);
    memory.write(0x3fffff, 4);
    memory.snapshot(VIDEO_MEMORY_BYTES, None);
    assert_eq!(
        memory.dirty_pages().ranges(0).collect::<Vec<_>>(),
        vec![4096..5120]
    );
    assert_eq!(memory.dirty_pages().ranges(3).count(), 0);
    assert_eq!(
        memory.dirty_pages().ranges(4).collect::<Vec<_>>(),
        vec![0..1024]
    );
    assert_eq!(
        memory.dirty_pages().ranges(63).collect::<Vec<_>>(),
        vec![64512..65536]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn consecutive_writes_copy_each_written_block() {
    let mut memory = Storage::default();
    memory.write_bytes(0xfffe, &[1, 2, 3, 4]);
    let before = memory.snapshot(VGA_MEMORY_BYTES, None);
    memory.clear_dirty();
    memory.write_bytes(0xfffe, &[1, 2, 5, 6]);
    let after = memory.snapshot(VGA_MEMORY_BYTES, Some(&before));
    assert_eq!(&before.blocks[0][0xfffe..], &[1, 2]);
    assert_eq!(&before.blocks[1][..3], &[3, 4, 0]);
    assert_eq!(&after.blocks[1][..3], &[5, 6, 0]);
    for block in [2, 3] {
        assert!(Arc::ptr_eq(&before.blocks[block], &after.blocks[block]));
    }
    assert!(!Arc::ptr_eq(&before.blocks[0], &after.blocks[0]));
    assert!(!Arc::ptr_eq(&before.blocks[1], &after.blocks[1]));
    #[cfg(feature = "renderer")]
    {
        assert_eq!(
            memory.dirty_pages().ranges(0).collect::<Vec<_>>(),
            vec![64512..65536]
        );
        assert_eq!(
            memory.dirty_pages().ranges(1).collect::<Vec<_>>(),
            vec![0..1024]
        );
        memory.clear_dirty();
        // Both pages are written, including the equal trailing byte.
        memory.write_bytes(0x103ff, &[7, 0]);
        memory.snapshot(VGA_MEMORY_BYTES, None);
        assert_eq!(
            memory.dirty_pages().ranges(1).collect::<Vec<_>>(),
            vec![0..2048]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clearing_vram_preserves_snapshots_and_marks_all_pages() {
    let mut memory = Storage::default();
    memory.write(0x3ffff, 0xa5);
    memory.write(0x3fffff, 0x5a);
    let before = memory.snapshot(VIDEO_MEMORY_BYTES, None).blocks;
    memory.clear_dirty();
    memory.clear();
    memory.snapshot(VIDEO_MEMORY_BYTES, None);
    assert_eq!(memory.read(0x3ffff), 0);
    assert_eq!(memory.read(0x3fffff), 0);
    assert_eq!(before[3][0xffff], 0xa5);
    assert_eq!(before[63][0xffff], 0x5a);
    #[cfg(feature = "renderer")]
    for block in 0..BLOCKS {
        assert_eq!(
            memory.dirty_pages().ranges(block).collect::<Vec<_>>(),
            vec![0..65536]
        );
    }
}
