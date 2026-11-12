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

    svga::enable(&vga.io);
    reads.store(0, Ordering::Relaxed);
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
