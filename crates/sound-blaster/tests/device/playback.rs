use super::*;
use relocade_dma::{ADDRESS_PORTS, COUNT_PORTS, STATUS_PORT};
use std::{future::Future, pin::pin, task::Poll};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dma_mask_changes_between_batches_preserve_elapsed_sample_slots() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 31);
    hw.io.write_port(relocade_dma::SINGLE_MASK_PORT, 1, 5);
    hw.command(&[0x40, 156, 0x14, 15, 0]); // Sixteen samples at 10 kHz.

    hw.clock.advance_by(Duration::from_micros(500));
    hw.io.write_port(relocade_dma::SINGLE_MASK_PORT, 1, 1);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x100);

    hw.clock.advance_by(Duration::from_micros(250));
    hw.io.write_port(relocade_dma::SINGLE_MASK_PORT, 1, 5);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);

    hw.clock.advance_by(Duration::from_micros(300));
    hw.io.write_port(relocade_dma::SINGLE_MASK_PORT, 1, 1);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    hw.clock.advance_by(Duration::from_micros(50));
    // Reading DMA progress also services the peripheral, without DSP polling.
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x103);
    assert_eq!(hw.dma_word(COUNT_PORTS[1]), 28);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dma_reprogramming_finishes_elapsed_transfers_with_the_old_state() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 15);
    hw.command(&[0x40, 156, 0x14, 3, 0]);
    hw.clock.advance_by(Duration::from_micros(250));
    hw.program_dma(0x200, 15);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x200);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(hw.io.poll_interrupt(&mut cx).is_pending());

    hw.clock.advance_by(Duration::from_micros(150));
    assert_eq!(hw.io.read_port(STATUS_PORT, 1), 0);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x202);
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn muted_headless_playback_completes_and_acknowledgement_rearms_irq() {
    let hw = Hardware::new();
    hw.memory
        .view()
        .write(0x100, &[0x80, 0xff, 0, 0x80])
        .unwrap();
    hw.program_dma(0x100, 3);
    hw.command(&[0x40, 156, 0x14, 3, 0]); // Four samples at 10 kHz.
    let mut activity = pin!(SoundBlaster::run(hw.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(399));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(hw.io.poll_interrupt(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(1));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x104);
    assert_eq!(hw.dma_word(COUNT_PORTS[1]), 0xffff);
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
    relocade_pic::end_of_interrupt(&hw.io, IRQ);

    // PIC EOI and reads of DSP data do not lower the DSP's interrupt line.
    hw.io.read_port(READ_DATA_PORT, 1);
    hw.program_dma(0x100, 0);
    hw.command(&[0x14, 0, 0]);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(100));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(hw.io.poll_interrupt(&mut cx).is_pending());

    hw.io.read_port(READ_STATUS_PORT, 1);
    hw.program_dma(0x100, 0);
    hw.command(&[0x14, 0, 0]);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(100));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
    hw.command(&[0xd8]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0, "DAC stayed muted");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dsp_length_is_independent_of_dma_terminal_count() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 1);
    hw.command(&[0x40, 156, 0x14, 3, 0]);
    hw.clock.advance_by(Duration::from_micros(1_000));
    hw.io.read_port(WRITE_STATUS_PORT, 1); // Synchronize while serving guest I/O.
    assert_eq!(hw.io.read_port(STATUS_PORT, 1), 2);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(hw.io.poll_interrupt(&mut cx).is_pending());

    hw.program_dma(0x200, 3);
    hw.io.read_port(WRITE_STATUS_PORT, 1);
    assert_eq!(
        hw.dma_word(ADDRESS_PORTS[1]),
        0x200,
        "no backlog while DMA was masked"
    );
    hw.clock.advance_by(Duration::from_micros(200));
    hw.io.read_port(WRITE_STATUS_PORT, 1);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x202);
    assert_eq!(hw.dma_word(COUNT_PORTS[1]), 1);
    assert_eq!(hw.io.read_port(STATUS_PORT, 1), 0);
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sample_phase_survives_polls_and_reset_discards_partial_commands() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 3);
    hw.command(&[0x40, 159, 0x14, 3, 0, 0xd1]); // 97 us/sample.
    for micros in [50, 96, 97, 150, 193] {
        hw.clock
            .advance_to(hw.clock.at(Duration::from_micros(micros)));
        hw.io.read_port(WRITE_STATUS_PORT, 1);
        assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x100 + (micros / 97) as u16);
    }
    hw.command(&[0xd8]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xff);
    hw.command(&[0x14, 0xe1]); // A length byte must not be parsed as GET_VERSION.
    assert_eq!(hw.io.read_port(READ_STATUS_PORT, 1), 0);
    hw.io.write_port(RESET_PORT, 1, 1);
    hw.io.write_port(RESET_PORT, 1, 0);
    hw.clock.advance_by(Duration::from_secs(1));
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0xaa);
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
    hw.command(&[0xe1]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 2);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 1);
    hw.command(&[0xd8]);
    assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0);
    assert!(hw
        .io
        .poll_interrupt(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}
