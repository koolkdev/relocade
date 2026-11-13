use relocade_clock_test_support::ManualClock;
use relocade_machine::IoBus;
use relocade_sound_blaster::*;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn hardware() -> (ManualClock, IoBus) {
    let clock = ManualClock::new();
    let card = Rc::new(RefCell::new(SoundBlaster::new(clock.clock())));
    let io = IoBus::new(card.clone(), |io| SoundBlaster::configure_io(io, &card));
    (clock, io)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn reset_readiness_and_version_use_the_dsp_ports() {
    let (clock, io) = hardware();
    io.write_port(RESET_PORT, 1, 1);
    assert_eq!(io.read_port(WRITE_STATUS_PORT, 1) & 0x80, 0x80);
    io.write_port(WRITE_DATA_PORT, 1, 0xe1); // Ignored while reset is held.
    assert_eq!(io.read_port(READ_STATUS_PORT, 1) & 0x80, 0);
    io.write_port(RESET_PORT, 1, 0);
    clock.advance_by(Duration::from_micros(99));
    assert_eq!(io.read_port(READ_STATUS_PORT, 1) & 0x80, 0);
    clock.advance_by(Duration::from_micros(1));
    assert_eq!(io.read_port(WRITE_STATUS_PORT, 1) & 0x80, 0);
    assert_eq!(io.read_port(READ_STATUS_PORT, 1) & 0x80, 0x80);
    assert_eq!(io.read_port(READ_DATA_PORT, 1), 0xaa);
    assert_eq!(io.read_port(READ_STATUS_PORT, 1) & 0x80, 0);
    io.write_port(WRITE_DATA_PORT, 2, 0xe1); // Byte-only command port.
    assert_eq!(io.read_port(READ_STATUS_PORT, 1) & 0x80, 0);
    io.write_port(WRITE_DATA_PORT, 1, 0xe1);
    assert_eq!(io.read_port(READ_DATA_PORT, 1), 2);
    assert_eq!(io.read_port(READ_DATA_PORT, 2), 0xffff);
    assert_eq!(io.read_port(READ_DATA_PORT, 1), 1);
    assert_eq!(io.read_port(READ_DATA_PORT, 1), 0xff);
    io.write_port(WRITE_DATA_PORT, 1, 0xe1);
    io.write_port(RESET_PORT, 1, 1); // Discard unread responses.
    io.write_port(RESET_PORT, 1, 0);
    clock.advance_by(Duration::from_micros(100));
    assert_eq!(io.read_port(READ_DATA_PORT, 1), 0xaa);
    assert_eq!(io.read_port(READ_STATUS_PORT, 1) & 0x80, 0);
    for port in [BASE_PORT, BASE_PORT + 4, BASE_PORT + 5, BASE_PORT + 0x0f] {
        assert_eq!(io.read_port(port, 1), 0xff);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fm_aliases_share_the_register_latch_and_timer() {
    let (clock, io) = hardware();
    io.write_port(FM_ADDRESS_STATUS_PORT, 1, 2);
    io.write_port(relocade_adlib::DATA_PORT, 1, 0xff);
    io.write_port(relocade_adlib::ADDRESS_STATUS_PORT, 1, 4);
    io.write_port(FM_DATA_PORT, 1, 0x21);
    clock.advance_by(Duration::from_micros(80));
    assert_eq!(io.read_port(FM_ADDRESS_STATUS_PORT, 1), 0xc6);
    assert_eq!(io.read_port(relocade_adlib::ADDRESS_STATUS_PORT, 1), 0xc6);
    io.write_port(RESET_PORT, 1, 1); // DSP reset leaves FM alone.
    assert_eq!(io.read_port(FM_ADDRESS_STATUS_PORT, 1), 0xc6);
    io.write_port(FM_DATA_PORT, 1, 0x80);
    assert_eq!(io.read_port(relocade_adlib::ADDRESS_STATUS_PORT, 1), 0x06);
}
