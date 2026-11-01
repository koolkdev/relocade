use super::*;
use crate::{CapturedFrame, Vga};
use relocade_machine::{Clock, Device};
use std::{cell::Cell, rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct TestClock(Cell<Duration>);
impl Clock for TestClock {
    fn now(&self) -> Duration {
        self.0.get()
    }
    async fn wait_until(&self, _: Duration) {
        std::future::pending::<()>().await
    }
}

struct Uploads {
    previous: Option<[Arc<[u8]>; 4]>,
    bytes: Vec<u8>,
}
impl Default for Uploads {
    fn default() -> Self {
        Self {
            previous: None,
            bytes: vec![0; 4 * PLANE_BYTES],
        }
    }
}
impl Uploads {
    fn update(&mut self, snapshot: &DisplaySnapshot) -> Vec<(usize, usize)> {
        let mut writes = Vec::new();
        write_planes(self.previous.as_ref(), snapshot, |offset, bytes| {
            self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
            writes.push((offset, bytes.len()));
        });
        for plane in 0..4 {
            assert_eq!(
                &self.bytes[plane * PLANE_BYTES..(plane + 1) * PLANE_BYTES],
                snapshot.plane(plane)
            );
        }
        self.previous = Some(snapshot.planes.clone());
        writes
    }
}

fn publish(vga: &mut Vga<TestClock>, clock: &TestClock) -> Arc<CapturedFrame> {
    clock.0.set(clock.now() + Duration::from_millis(20));
    vga.read_port(0x3da, 1);
    vga.frame_source().current_capture().unwrap()
}

fn planar(vga: &mut impl Device, mask: u32) {
    for (port, value) in [
        (0x3c4, (mask << 8) | 2),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        vga.write_port(port, 2, value);
    }
}

fn text(vga: &mut impl Device) {
    for (port, value) in [
        (0x3c4, 0x0302),
        (0x3c4, 0x0204),
        (0x3ce, 0x1005),
        (0x3ce, 0x0e06),
    ] {
        vga.write_port(port, 2, value);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn consecutive_captures_keep_character_attribute_and_font_uploads_independent() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let mut uploads = Uploads::default();
    let first = publish(&mut vga, &clock);
    assert_eq!(
        uploads.update(&first.display),
        [(0, 65536), (65536, 65536), (131072, 65536), (196608, 65536)]
    );
    assert!(uploads.update(&first.display).is_empty());

    vga.write_mmio(0xb8000, 1, u64::from(b'A'));
    let changed = publish(&mut vga, &clock);
    let latest = publish(&mut vga, &clock);
    assert!(
        Arc::ptr_eq(&changed, &latest),
        "unchanged refresh keeps dirty hints"
    );
    assert_eq!(uploads.update(&latest.display), [(0, 1024)]);
    vga.write_mmio(0xb8001, 1, 7);
    assert_eq!(
        uploads.update(&publish(&mut vga, &clock).display),
        [(65536, 1024)]
    );
    planar(&mut vga, 4);
    vga.write_mmio(0xa0820, 1, 0x80);
    text(&mut vga);
    assert_eq!(
        uploads.update(&publish(&mut vga, &clock).display),
        [(133120, 1024)]
    );
    vga.write_port(0x3c4, 2, 0x0503); // Bank selection changes settings, not font bytes.
    vga.write_port(0x3d4, 2, 0x080c); // Select previously hidden text memory.
    assert!(uploads
        .update(&publish(&mut vga, &clock).display)
        .is_empty());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn planar_pixel_writes_coalesce_adjacent_pages_and_preserve_gaps_and_plane_edges() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let mut uploads = Uploads::default();
    uploads.update(&publish(&mut vga, &clock).display);
    planar(&mut vga, 15);
    vga.write_mmio(0xa03ff, 2, 0x0201); // One write crossing a page boundary.
    vga.write_mmio(0xa1800, 1, 3);
    vga.write_mmio(0xaffff, 1, 4); // Last page, all four physical planes.
    assert_eq!(
        uploads.update(&publish(&mut vga, &clock).display),
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
        vga.write_mmio(0xa0000 + page * 1024, 1, 0x55);
    }
    assert_eq!(
        uploads.update(&publish(&mut vga, &clock).display),
        [(0, 65536), (65536, 65536), (131072, 65536), (196608, 65536)]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn routing_and_masks_mark_only_final_physical_changes() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    planar(&mut vga, 15);
    vga.write_port(0x3c4, 2, 0x0e04); // Chain four.
    let mut uploads = Uploads::default();
    uploads.update(&publish(&mut vga, &clock).display);
    vga.write_mmio(0xa0401, 1, 0x5a); // Physical plane one, offset 0x400.
    assert_eq!(
        uploads.update(&publish(&mut vga, &clock).display),
        [(66560, 1024)]
    );
    vga.write_mmio(0xa0401, 1, 0x5a); // Equal value.
    vga.read_mmio(0xa0401, 1); // Capture the existing bytes in the latches.
    vga.write_port(0x3ce, 2, 0x0008); // Pixel mask preserves the latches.
    vga.write_mmio(0xa0401, 1, 0xff);
    vga.write_port(0x3c4, 2, 0x0002); // No selected write planes.
    vga.write_mmio(0xa0401, 1, 0);
    vga.write_port(0x3c2, 1, 0x41); // RAM disabled.
    vga.write_mmio(0xa0401, 1, 0);
    vga.write_port(0x3c2, 1, 0x43);
    vga.write_port(0x3c4, 2, 0x0f02);
    vga.write_port(0x3ce, 2, 0xff08);
    // A real edit in another page makes false dirty bits from the no-op writes
    // observable, even though unchanged planes would otherwise skip uploading.
    vga.write_mmio(0xa0c01, 1, 0x33);
    assert_eq!(
        uploads.update(&publish(&mut vga, &clock).display),
        [(68608, 1024)]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn skipped_captures_restore_planes_even_when_the_latest_bitmap_is_empty() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let mut uploads = Uploads::default();
    let first = publish(&mut vga, &clock);
    uploads.update(&first.display);
    planar(&mut vga, 4);
    vga.write_mmio(0xa0820, 1, 0x80);
    text(&mut vga);
    let skipped = publish(&mut vga, &clock);
    let other_reader = vga.frame_source().current_capture().unwrap();
    vga.write_mmio(0xb8000, 1, u64::from(b'A'));
    let newest = publish(&mut vga, &clock);
    // Font changes belong to the skipped frame; its plane still needs restoring.
    assert_eq!(
        uploads.update(&newest.display),
        [(0, 1024), (131072, 65536)]
    );
    assert_eq!(other_reader.display.plane(2)[0x820], 0x80);
    drop(vga);
    assert_eq!(uploads.update(&skipped.display), [(0, 65536)]);
    assert_eq!(uploads.update(&first.display), [(131072, 65536)]);
    let other = Vga::with_clock(Rc::new(TestClock::default()));
    assert_eq!(
        uploads.update(&other.snapshot()),
        [(0, 65536), (65536, 65536), (131072, 65536), (196608, 65536)]
    );
    let mut second_renderer = Uploads::default();
    assert_eq!(second_renderer.update(&newest.display).len(), 4);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn immediate_snapshots_preserve_pending_changes_and_can_be_uploaded_independently() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let mut uploads = Uploads::default();
    uploads.update(&publish(&mut vga, &clock).display);
    vga.write_mmio(0xb8000, 1, u64::from(b'A'));
    let immediate = vga.snapshot();
    vga.write_mmio(0xb8400, 1, u64::from(b'B'));
    let published = publish(&mut vga, &clock);
    assert_eq!(uploads.update(&published.display), [(0, 2048)]);
    assert_eq!(immediate.plane(0)[0x400], 0);
    assert!(uploads
        .update(&publish(&mut vga, &clock).display)
        .is_empty());

    vga.write_mmio(0xb8800, 1, u64::from(b'C'));
    assert_eq!(uploads.update(&vga.snapshot()), [(2048, 1024)]);
    vga.write_mmio(0xb8c00, 1, u64::from(b'D'));
    // The previous immediate snapshot differs from the published baseline.
    assert_eq!(uploads.update(&vga.snapshot()), [(0, 65536)]);
    assert!(uploads
        .update(&publish(&mut vga, &clock).display)
        .is_empty());
}
