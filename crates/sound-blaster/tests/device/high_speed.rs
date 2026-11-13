use super::*;
use relocade_dma::{ADDRESS_PORTS, MODE_PORT};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn high_speed_single_cycle_rejects_commands_until_completion() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 1);
    hw.command(&[0xd1, 0x40, 231, 0x48, 1, 0, 0x91]); // Two samples at 40 kHz.
    assert_eq!(hw.io.read_port(WRITE_STATUS_PORT, 1), 0x80);
    hw.command(&[0xd3, 0xe1, 0x40, 156]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xff);
    hw.clock.advance_by(Duration::from_micros(49));
    assert_eq!(hw.io.read_port(WRITE_STATUS_PORT, 1), 0x80);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
    hw.clock.advance_by(Duration::from_micros(1));
    assert_eq!(hw.io.read_port(WRITE_STATUS_PORT, 1), 0);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    hw.acknowledge_irq();
    hw.command(&[0xd8]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xff, "mute was ignored");
    hw.command(&[0xe1]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 2);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn high_speed_reset_stops_streaming_and_preserves_dsp_settings() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 3);
    hw.io.write_port(MODE_PORT, 1, 0x59);
    hw.command(&[0xd1, 0x40, 231, 0x48, 1, 0, 0x90]);
    hw.clock.advance_by(Duration::from_micros(75));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x103);
    hw.acknowledge_irq();
    hw.command(&[0xd0, 0xd3, 0xda, 0x40, 156, 0x14, 0, 0]);
    hw.clock.advance_by(Duration::from_micros(25));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x100);
    assert_eq!(hw.io.read_port(WRITE_STATUS_PORT, 1), 0x80);
    hw.acknowledge_irq();

    hw.io.write_port(RESET_PORT, 1, 1);
    hw.io.write_port(RESET_PORT, 1, 1); // One pulse must not become a full reset.
    hw.io.write_port(RESET_PORT, 1, 0);
    hw.clock.advance_by(Duration::from_micros(100));
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xaa);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x100);
    hw.command(&[0xd8]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xff);
    hw.command(&[0x91]); // Reuse the preserved two-sample block and 25 us period.
    hw.clock.advance_by(Duration::from_micros(49));
    assert_eq!(hw.io.read_port(WRITE_STATUS_PORT, 1), 0x80);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
    hw.clock.advance_by(Duration::from_micros(1));
    assert_eq!(hw.io.read_port(WRITE_STATUS_PORT, 1), 0);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    hw.acknowledge_irq();

    // Once high-speed mode has ended, reset restores normal power-on settings.
    hw.io.write_port(RESET_PORT, 1, 1);
    hw.io.write_port(RESET_PORT, 1, 0);
    hw.clock.advance_by(Duration::from_micros(100));
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xaa);
    hw.command(&[0xd8]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0);
    hw.command(&[0x14, 0, 0]);
    hw.clock.advance_by(Duration::from_micros(255));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    hw.clock.advance_by(Duration::from_micros(1));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x103);
    hw.acknowledge_irq();
}
