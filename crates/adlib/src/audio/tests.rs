use crate::{Adlib, ADDRESS_STATUS_PORT, DATA_PORT};
use relocade_clock_test_support::ManualClock;
use relocade_machine::IoBus;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn dropping_output_releases_recording_but_not_timer_status() {
    let clock = ManualClock::new();
    let adlib = Rc::new(RefCell::new(Adlib::new(clock.clock())));
    let io = IoBus::default();
    io.connect(adlib.clone(), |io| Adlib::configure_io(io, &adlib));
    let source = adlib.borrow_mut().audio_source(48_000).unwrap();
    io.write_port(ADDRESS_STATUS_PORT, 1, 0xb0);
    io.write_port(DATA_PORT, 1, 0x31);
    assert_eq!(source.writes.borrow().len(), 1);
    drop(source);
    for _ in 0..100 {
        io.write_port(DATA_PORT, 1, 0x11);
        io.write_port(DATA_PORT, 1, 0x31);
    }
    assert!(adlib.borrow().writes.upgrade().is_none());
    io.write_port(ADDRESS_STATUS_PORT, 1, 2);
    io.write_port(DATA_PORT, 1, 0xff);
    io.write_port(ADDRESS_STATUS_PORT, 1, 4);
    io.write_port(DATA_PORT, 1, 1);
    clock.advance_by(Duration::from_micros(80));
    assert_eq!(io.read_port(ADDRESS_STATUS_PORT, 1), 0xc6);
}
