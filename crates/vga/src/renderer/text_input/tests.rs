use super::*;
use crate::{renderer::memory::PlaneContents, renderer::upload::QueuedBytes, Vga};
use relocade_machine::{Clock, Device};
use std::{cell::Cell, rc::Rc, time::Duration};

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

fn publish(vga: &mut impl Device, clock: &TestClock) {
    clock.0.set(clock.now() + Duration::from_millis(15));
    vga.read_port(0x3da, 1);
}

struct Uploads {
    selection: TextSelection,
    contents: PlaneContents,
}
impl Uploads {
    fn new() -> Self {
        Self {
            selection: TextSelection::new(),
            contents: PlaneContents::new(),
        }
    }
    fn update(&mut self, snapshot: &DisplaySnapshot) -> Vec<(usize, Vec<u8>)> {
        let changed = self.selection.update(&snapshot.text_display());
        let mut writes = Vec::new();
        self.contents.update(
            snapshot,
            &self.selection.required,
            changed,
            |offset, bytes| writes.push((offset, bytes.to_vec())),
        );
        writes
    }
}

fn font_byte(vga: &mut impl Device, address: u32, value: u8) {
    for (port, value) in [
        (0x3c4, 0x0402),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        vga.write_port(port, 2, value);
    }
    vga.write_mmio(0xa0000 + address, 1, u64::from(value));
    for (port, value) in [
        (0x3c4, 0x0302),
        (0x3c4, 0x0204),
        (0x3ce, 0x1005),
        (0x3ce, 0x0e06),
    ] {
        vga.write_port(port, 2, value);
    }
}

#[test]
fn text_edits_reuse_resident_fonts_and_hidden_changes_wait_until_visible() {
    let mut vga = Vga::with_clock(Rc::new(TestClock::default()));
    let first = vga.snapshot();
    let mut uploads = Uploads::new();
    let initial = uploads.update(&first);
    assert_eq!(
        initial
            .iter()
            .filter(|(offset, _)| *offset >= 2 * 65536)
            .map(|(_, bytes)| bytes.len())
            .sum::<usize>(),
        8192
    );
    assert!(uploads.update(&first).is_empty());
    vga.write_mmio(0xb8000, 1, u64::from(b'A')); // Previously unseen glyph is already resident.
    assert_eq!(uploads.update(&vga.snapshot()), [(0, vec![b'A', 0, 0, 0])]);
    vga.write_mmio(0xb8001, 1, 7);
    assert_eq!(uploads.update(&vga.snapshot()), [(65536, vec![7, 0, 0, 0])]);
    vga.write_mmio(0xb9000, 1, 1);
    assert!(uploads.update(&vga.snapshot()).is_empty());
    vga.write_port(0x3d4, 2, 0x080c);
    assert!(uploads
        .update(&vga.snapshot())
        .iter()
        .any(|(offset, bytes)| *offset == 0x1000 && bytes[0] == 1));
    assert_eq!(
        uploads.update(&first),
        [(0, vec![0; 4]), (65536, vec![0; 4])]
    );
}

#[test]
fn fonts_change_independently_and_hidden_banks_are_refreshed_when_selected() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    let mut uploads = Uploads::new();
    publish(&mut vga, &clock);
    uploads.update(&source.current_capture().unwrap().display);

    font_byte(&mut vga, 65 * 32, 0x80);
    publish(&mut vga, &clock);
    assert_eq!(
        uploads.update(&source.current_capture().unwrap().display),
        [(2 * 65536 + 65 * 32, vec![0x80, 0, 0, 0])]
    );
    font_byte(&mut vga, 0x4000 + 65 * 32, 0x40);
    publish(&mut vga, &clock);
    assert!(uploads
        .update(&source.current_capture().unwrap().display)
        .is_empty());
    vga.write_port(0x3c4, 2, 0x0503); // Select the previously hidden bank for both fonts.
    publish(&mut vga, &clock);
    let writes = uploads.update(&source.current_capture().unwrap().display);
    assert_eq!(
        writes.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
        8192
    );
    assert!(writes
        .iter()
        .all(|(offset, _)| (2 * 65536 + 0x4000..2 * 65536 + 0x6000).contains(offset)));
    let glyph = writes
        .iter()
        .find(|(offset, bytes)| {
            *offset <= 2 * 65536 + 0x4820 && *offset + bytes.len() > 2 * 65536 + 0x4820
        })
        .unwrap();
    assert_eq!(glyph.1[2 * 65536 + 0x4820 - glyph.0], 0x40);
    // The old bank is resident but no longer selected. Writes stay pending until
    // selection returns to it; its previously uploaded bytes are not assumed fresh.
    font_byte(&mut vga, 65 * 32, 0x20);
    publish(&mut vga, &clock);
    assert!(uploads
        .update(&source.current_capture().unwrap().display)
        .is_empty());
    vga.write_port(0x3c4, 2, 0x0003);
    publish(&mut vga, &clock);
    assert_eq!(
        uploads.update(&source.current_capture().unwrap().display),
        [(2 * 65536 + 65 * 32, vec![0x20, 0, 0, 0])]
    );
}

#[test]
fn accumulated_rectangles_upload_all_skipped_writes_and_restore_old_captures() {
    let clock = Rc::new(TestClock::default());
    let mut vga = Vga::with_clock(clock.clone());
    let source = vga.frame_source();
    let mut uploads = Uploads::new();
    publish(&mut vga, &clock);
    let first = source.current_capture().unwrap();
    uploads.update(&first.display);
    vga.write_mmio(0xb8000, 1, 1);
    publish(&mut vga, &clock); // Graphics owner has not consumed this capture.
    vga.write_mmio(0xb8200, 1, 2);
    publish(&mut vga, &clock);
    let newest = source.current_capture().unwrap();
    assert_eq!(
        uploads.update(&newest.display),
        [(0, vec![1, 0, 0, 0]), (0x200, vec![2, 0, 0, 0])]
    );
    publish(&mut vga, &clock);
    assert!(uploads
        .update(&source.current_capture().unwrap().display)
        .is_empty());
    assert_eq!(
        uploads.update(&first.display),
        [(0, vec![0; 4]), (0x200, vec![0; 4])]
    );
    assert_eq!(
        uploads.update(&newest.display),
        [(0, vec![1, 0, 0, 0]), (0x200, vec![2, 0, 0, 0])]
    );
}

#[test]
fn blink_and_cursor_update_settings_without_reuploading_memory() {
    let vga = Vga::with_clock(Rc::new(TestClock::default()));
    let mut snapshot = vga.snapshot();
    let mut contents = QueuedBytes::new(SETTINGS_WORDS * 4);
    let mut memory = Uploads::new();
    memory.update(&snapshot);
    let required = (1 << SETTINGS_WORDS) - 1;
    contents.update(
        0,
        settings(&snapshot.text_display()).as_flattened(),
        required,
        |_, _| {},
    );
    snapshot.blink_on = false;
    snapshot.registers.crtc[15] = 3;
    let mut writes = Vec::new();
    contents.update(
        0,
        settings(&snapshot.text_display()).as_flattened(),
        required,
        |offset, bytes| writes.push((offset, bytes.to_vec())),
    );
    assert_eq!(writes, [(64, vec![3, 0, 0, 0]), (80, vec![0; 4])]);
    assert!(memory.update(&snapshot).is_empty());
}
