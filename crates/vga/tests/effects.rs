mod common;

use common::{fixture::ConnectedVga, support::svga};
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Clock, ClockSource, Sleep, Timestamp};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct ObservedClock {
    clock: ManualClock,
    reads: Arc<AtomicUsize>,
}

impl ClockSource for ObservedClock {
    fn now(&self) -> Timestamp {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.clock.now()
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        self.clock.sleep_until(deadline)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn access_and_decode_controls_do_no_clock_work() {
    let reads = Arc::new(AtomicUsize::new(0));
    let vga = ConnectedVga::new(Clock::new(ObservedClock {
        clock: ManualClock::new(),
        reads: reads.clone(),
    }));
    reads.store(0, Ordering::Relaxed);
    for (port, bytes, value) in [
        (0x3c4, 1, 2),
        (0x3c5, 1, 0x0f),   // CPU write-plane mask.
        (0x3ce, 2, 0x0500), // CPU set/reset value.
        (0x3ce, 1, 4),
        (0x3d4, 1, 0x0f),
        (0x3c7, 1, 3),
        (0x3c8, 1, 7),
        (0x1ce, 2, 1),
        (0x1cf, 2, 640),
        (0x46e8, 1, 0x18),
        (0x102, 1, 1),
        (0x46e8, 1, 8),
    ] {
        vga.io.write_port(port, bytes, value);
    }
    assert_eq!(reads.load(Ordering::Relaxed), 0);

    vga.io.write_mmio(0xb8000, 1, 0x55); // VGA writes without graphics transformations.
    vga.io.write_port(0x3ce, 2, 0x0101); // Enable set/reset for general VGA writes.
    vga.io.write_mmio(0xb8000, 1, 0xaa);
    assert_eq!(reads.load(Ordering::Relaxed), 0);

    // Palette gating and register validity determine video preparation for data
    // writes; display addressing still permits other Attribute registers.
    for (address, prepares_video) in [(0x24, false), (0x1f, false), (0x32, true)] {
        vga.io.write_port(0x3c0, 1, address);
        reads.store(0, Ordering::Relaxed);
        vga.io.write_port(0x3c0, 1, 7);
        assert_eq!(reads.load(Ordering::Relaxed) > 0, prepares_video);
    }
    assert_eq!(vga.io.read_port(0x3c1, 1), 7);

    svga::enable(&vga.io);
    reads.store(0, Ordering::Relaxed);
    vga.io.write_mmio(0xa0000, 4, 0x44332211);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    // Legacy registers still mutate while packed output ignores their image
    // and timing settings. They become effective when DISPI is disabled.
    for (port, value) in [(0x3c4, 0x2101), (0x3ce, 0x0506), (0x3d4, 0x230d)] {
        vga.io.write_port(port, 2, value);
        assert_eq!(vga.io.read_port(port, 2), value);
    }
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    vga.io.write_port(0x3c9, 1, 17);
    assert!(
        reads.load(Ordering::Relaxed) > 0,
        "the palette still affects packed output"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn font_addressing_change_preserves_overdue_display_inputs() {
    let clock = ManualClock::new();
    let (vga, mut video) = common::start_video(ConnectedVga::new(clock.clock()));
    let frames = vga.device.borrow().frames();
    clock.advance_to(clock.at(common::boundary(1)));
    // Sequencer Memory Mode controls extended font addressing as well as CPU
    // accesses, so its write must capture the previous display first.
    vga.io.write_port(0x3c4, 2, 0x0004);
    assert_eq!(vga.snapshot().vga().unwrap().sequencer[4], 0);
    common::service_video(video.as_mut());
    assert_eq!(frames.try_recv().unwrap().vga().unwrap().sequencer[4], 2);
}
