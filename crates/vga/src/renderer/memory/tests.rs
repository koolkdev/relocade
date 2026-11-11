use super::*;
use crate::{DisplaySnapshot, Vga};
use futures_util::FutureExt;
use relocade_clock_test_support::ManualClock;
use relocade_machine::Device;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Uploads {
    previous: Option<MemorySnapshot>,
    bytes: Vec<u8>,
}
impl Default for Uploads {
    fn default() -> Self {
        Self {
            previous: None,
            bytes: vec![0; crate::VIDEO_MEMORY_BYTES as usize],
        }
    }
}
impl Uploads {
    fn update(&mut self, snapshot: &DisplaySnapshot) -> Vec<(usize, usize)> {
        let mut writes = Vec::new();
        write_blocks(self.previous.as_ref(), snapshot, |offset, bytes| {
            self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
            writes.push((offset, bytes.len()));
        });
        for (block, bytes) in snapshot.memory.blocks.iter().enumerate() {
            assert_eq!(
                &self.bytes[block * BLOCK_BYTES..(block + 1) * BLOCK_BYTES],
                bytes.as_ref()
            );
        }
        self.previous = Some(snapshot.memory.clone());
        writes
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn newly_captured_blocks_require_complete_uploads_without_a_baseline() {
    let vga = Vga::with_clock(ManualClock::new().clock());
    let narrow = vga.snapshot();
    let mut wide = narrow.clone();
    wide.memory = vga.memory.snapshot(0x80000);
    wide.changes = Some(crate::snapshot::MemoryChanges {
        baseline: narrow.memory.clone(),
        pages: Default::default(),
    });
    let mut uploads = Uploads::default();
    uploads.update(&narrow);
    let added = [
        (262144, 65536),
        (327680, 65536),
        (393216, 65536),
        (458752, 65536),
    ];
    assert_eq!(uploads.update(&wide), added);
    assert!(uploads.update(&narrow).is_empty());
    assert_eq!(uploads.update(&wide), added);

    // The renderer has this block, but the sparse baseline still does not.
    Arc::make_mut(&mut wide.memory.blocks)[4] = vec![0xa5; 65536].into();
    assert_eq!(uploads.update(&wide), [(262144, 65536)]);
    assert_eq!(&uploads.bytes[262144..327680], &[0xa5; 65536]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn packed_snapshots_preserve_skipped_extended_writes_and_clear_on_mode_entry() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        let mut uploads = Uploads::default();
        uploads.update(&take(&vga, video.as_mut(), &clock));
        for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 1)] {
            vga.borrow_mut()
                .write_port(context.reborrow(), 0x1ce, 2, index);
            vga.borrow_mut()
                .write_port(context.reborrow(), 0x1cf, 2, value);
        }
        assert_eq!(uploads.update(&take(&vga, video.as_mut(), &clock)).len(), 8);
        vga.borrow_mut().write_port(context.reborrow(), 0x1ce, 2, 5);
        vga.borrow_mut().write_port(context.reborrow(), 0x1cf, 2, 4);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0000, 1, 1);
        publish(&vga, video.as_mut(), &clock);
        let skipped = vga.borrow().snapshot();
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa1000, 1, 2);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa2003, 1, 3);
        let latest = take(&vga, video.as_mut(), &clock);
        assert_eq!(uploads.update(&latest), [(262144, 65536), (460800, 1024)]);
        assert_eq!(skipped.vram_byte(0x40000), Some(1));
        assert_eq!(skipped.vram_byte(0x41000), Some(0));

        vga.borrow_mut().write_port(context.reborrow(), 0x1ce, 2, 4);
        vga.borrow_mut().write_port(context.reborrow(), 0x1cf, 2, 0);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8000, 1, u64::from(b'A'));
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(0, 1024)]
        );
        vga.borrow_mut().write_port(context.reborrow(), 0x1cf, 2, 1);
        let cleared = take(&vga, video.as_mut(), &clock);
        let writes = uploads.update(&cleared);
        assert_eq!(writes.len(), 8);
        assert!(writes.iter().all(|(_, bytes)| *bytes == 65536));
        assert_eq!(cleared.vram_byte(0x42003), Some(0));
        assert_eq!(latest.vram_byte(0x42003), Some(3));
    })
}

fn start_video(clock: &ManualClock) -> (Rc<RefCell<Vga>>, Pin<Box<impl Future<Output = ()>>>) {
    let vga = Rc::new(RefCell::new(Vga::with_clock(clock.clock())));
    let mut video = Box::pin(Vga::run(vga.clone()));
    assert!(video.as_mut().now_or_never().is_none());
    (vga, video)
}

fn publish(vga: &Rc<RefCell<Vga>>, video: Pin<&mut impl Future<Output = ()>>, clock: &ManualClock) {
    relocade_machine::IoBus::default().with_context(|mut context| {
        clock.advance_by(Duration::from_millis(20));
        vga.borrow_mut().read_port(context.reborrow(), 0x3da, 1);
        assert!(video.now_or_never().is_none());
    })
}

fn take(
    vga: &Rc<RefCell<Vga>>,
    video: Pin<&mut impl Future<Output = ()>>,
    clock: &ManualClock,
) -> DisplaySnapshot {
    publish(vga, video, clock);
    vga.borrow().frames().try_recv().unwrap()
}

fn planar(vga: &mut impl Device, mask: u32) {
    relocade_machine::IoBus::default().with_context(|mut context| {
        for (port, value) in [
            (0x3c4, (mask << 8) | 2),
            (0x3c4, 0x0604),
            (0x3ce, 0x0005),
            (0x3ce, 0x0506),
        ] {
            vga.write_port(context.reborrow(), port, 2, value);
        }
    })
}

fn text(vga: &mut impl Device) {
    relocade_machine::IoBus::default().with_context(|mut context| {
        for (port, value) in [
            (0x3c4, 0x0302),
            (0x3c4, 0x0204),
            (0x3ce, 0x1005),
            (0x3ce, 0x0e06),
        ] {
            vga.write_port(context.reborrow(), port, 2, value);
        }
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn consecutive_captures_keep_character_attribute_and_font_uploads_independent() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        let mut uploads = Uploads::default();
        let first = take(&vga, video.as_mut(), &clock);
        assert_eq!(
            uploads.update(&first),
            [(0, 65536), (65536, 65536), (131072, 65536), (196608, 65536)]
        );
        assert!(uploads.update(&first).is_empty());

        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8000, 1, u64::from(b'A'));
        publish(&vga, video.as_mut(), &clock);
        publish(&vga, video.as_mut(), &clock);
        assert_eq!(vga.borrow().frames().len(), 1);
        assert_eq!(
            uploads.update(&vga.borrow().frames().try_recv().unwrap()),
            [(0, 1024)]
        );
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8001, 1, 7);
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(65536, 1024)]
        );
        planar(&mut *vga.borrow_mut(), 4);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0820, 1, 0x80);
        text(&mut *vga.borrow_mut());
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(133120, 1024)]
        );
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3c4, 2, 0x0503); // Bank selection changes settings, not font bytes.
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3d4, 2, 0x080c); // Select previously hidden text memory.
        assert!(uploads
            .update(&take(&vga, video.as_mut(), &clock))
            .is_empty());
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn planar_pixel_writes_coalesce_adjacent_pages_and_preserve_gaps_and_plane_edges() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        let mut uploads = Uploads::default();
        uploads.update(&take(&vga, video.as_mut(), &clock));
        planar(&mut *vga.borrow_mut(), 15);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa03ff, 2, 0x0201); // One write crossing a page boundary.
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa1800, 1, 3);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xaffff, 1, 4); // Last page, all four physical planes.
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [
                (0, 2048),
                (6144, 1024),
                (64512, 1024),
                (65536, 2048),
                (71680, 1024),
                (130048, 1024),
                (131072, 2048),
                (137216, 1024),
                (195584, 1024),
                (196608, 2048),
                (202752, 1024),
                (261120, 1024),
            ]
        );
        for page in 0..64 {
            vga.borrow_mut()
                .write_mmio(context.reborrow(), 0xa0000 + page * 1024, 1, 0x55);
        }
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(0, 65536), (65536, 65536), (131072, 65536), (196608, 65536)]
        );
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn routing_and_masks_mark_only_final_physical_changes() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        planar(&mut *vga.borrow_mut(), 15);
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3c4, 2, 0x0e04); // Chain four.
        let mut uploads = Uploads::default();
        uploads.update(&take(&vga, video.as_mut(), &clock));
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0401, 1, 0x5a); // Physical plane one, offset 0x400.
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(66560, 1024)]
        );
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0401, 1, 0x5a); // Equal value.
        vga.borrow_mut().read_mmio(context.reborrow(), 0xa0401, 1); // Capture the existing bytes in the latches.
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3ce, 2, 0x0008); // Pixel mask preserves the latches.
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0401, 1, 0xff);
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3c4, 2, 0x0002); // No selected write planes.
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0401, 1, 0);
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3c2, 1, 0x41); // RAM disabled.
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0401, 1, 0);
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3c2, 1, 0x43);
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3c4, 2, 0x0f02);
        vga.borrow_mut()
            .write_port(context.reborrow(), 0x3ce, 2, 0xff08);
        // A real edit in another page makes false dirty bits from the no-op writes
        // observable, even though unchanged planes would otherwise skip uploading.
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0c01, 1, 0x33);
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(68608, 1024)]
        );
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn skipped_queued_snapshots_restore_blocks_with_a_different_upload_baseline() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        let mut uploads = Uploads::default();
        let first = take(&vga, video.as_mut(), &clock);
        uploads.update(&first);
        planar(&mut *vga.borrow_mut(), 4);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xa0820, 1, 0x80);
        text(&mut *vga.borrow_mut());
        publish(&vga, video.as_mut(), &clock);
        let skipped = vga.borrow().snapshot();
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8000, 1, u64::from(b'A'));
        publish(&vga, video.as_mut(), &clock);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8800, 1, u64::from(b'B'));
        let newest = take(&vga, video.as_mut(), &clock);
        // The queued font and first text snapshots were replaced. Their changed
        // blocks no longer match the renderer's baseline, so restore them in full.
        assert_eq!(uploads.update(&newest), [(0, 65536), (131072, 65536)]);
        assert_eq!(skipped.plane(2)[0x820], 0x80);
        assert_eq!(skipped.plane(0)[0], 0);
        assert_eq!(skipped.plane(0)[0x800], 0);

        // Later writes start a fresh interval, without resending the earlier pages.
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb9000, 1, u64::from(b'C'));
        assert_eq!(
            uploads.update(&take(&vga, video.as_mut(), &clock)),
            [(4096, 1024)]
        );
        drop(video);
        drop(vga);
        assert_eq!(uploads.update(&skipped), [(0, 65536)]);
        assert_eq!(uploads.update(&first), [(131072, 65536)]);
        let other = Vga::with_clock(ManualClock::new().clock());
        assert_eq!(
            uploads.update(&other.snapshot()),
            [(0, 65536), (65536, 65536), (131072, 65536), (196608, 65536)]
        );
        let mut second_renderer = Uploads::default();
        assert_eq!(second_renderer.update(&newest).len(), 4);
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn concurrent_queue_replacement_preserves_uploaded_memory() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        let mut uploads = Uploads::default();
        uploads.update(&take(&vga, video.as_mut(), &clock));
        let source = vga.borrow().frames();
        let start = std::sync::Barrier::new(2);

        let uploads = std::thread::scope(|scope| {
            let consumer = scope.spawn(|| {
                start.wait();
                while let Ok(capture) = pollster::block_on(source.recv()) {
                    let writes = uploads.update(&capture);
                    assert!(writes.is_empty() || writes == [(0, 1024)] || writes == [(0, 65536)]);
                    std::thread::yield_now();
                }
                uploads
            });
            start.wait();
            for value in 1..=128 {
                vga.borrow_mut()
                    .write_mmio(context.reborrow(), 0xb8000, 1, value);
                publish(&vga, video.as_mut(), &clock);
                std::thread::yield_now();
            }
            drop(video);
            drop(vga);
            consumer.join().unwrap()
        });
        assert_eq!(uploads.bytes[0], 128);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn immediate_snapshots_preserve_pending_changes_and_can_be_uploaded_independently() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let clock = ManualClock::default();
        let (vga, mut video) = start_video(&clock);
        let mut uploads = Uploads::default();
        uploads.update(&take(&vga, video.as_mut(), &clock));
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8000, 1, u64::from(b'A'));
        let immediate = vga.borrow().snapshot();
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8400, 1, u64::from(b'B'));
        let published = take(&vga, video.as_mut(), &clock);
        assert_eq!(uploads.update(&published), [(0, 2048)]);
        assert_eq!(immediate.plane(0)[0x400], 0);
        publish(&vga, video.as_mut(), &clock);
        assert!(vga.borrow().frames().is_empty());
        assert!(uploads.update(&published).is_empty());

        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8800, 1, u64::from(b'C'));
        assert_eq!(uploads.update(&vga.borrow().snapshot()), [(2048, 1024)]);
        vga.borrow_mut()
            .write_mmio(context.reborrow(), 0xb8c00, 1, u64::from(b'D'));
        // The previous immediate snapshot differs from the published baseline.
        assert_eq!(uploads.update(&vga.borrow().snapshot()), [(0, 65536)]);
        assert!(uploads
            .update(&take(&vga, video.as_mut(), &clock))
            .is_empty());
    })
}
