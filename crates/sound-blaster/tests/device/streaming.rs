use super::*;
use relocade_dma::{ADDRESS_PORTS, MODE_PORT};
use std::{future::Future, pin::pin, task::Poll};

fn acknowledge(hw: &Hardware) {
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
    hw.io.read_port(READ_STATUS_PORT, 1);
    relocade_pic::end_of_interrupt(&hw.io, IRQ);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn streaming_refills_dma_halves_and_exits_after_the_current_block() {
    let hw = Hardware::new();
    let mut source = hw.card.borrow().audio_source();
    hw.memory.view().write(0x100, &[0, 128, 255, 64]).unwrap();
    hw.program_dma(0x100, 3);
    hw.io.write_port(MODE_PORT, 1, 0x59); // DMA loops over four bytes.
    hw.command(&[0xd1, 0x40, 156, 0x48, 1, 0, 0x1c]); // DSP blocks contain two.
    let mut activity = pin!(SoundBlaster::run(hw.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());

    hw.clock.advance_by(Duration::from_micros(200));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    acknowledge(&hw);
    hw.memory.view().write(0x100, &[64, 192]).unwrap();

    hw.clock.advance_by(Duration::from_micros(200));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x100);
    acknowledge(&hw);
    hw.clock.advance_by(Duration::from_micros(50));
    hw.command(&[0xda]);
    hw.clock.advance_by(Duration::from_micros(250));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    acknowledge(&hw);

    let output: Vec<_> = (0..7)
        .map(|index| {
            source.sample(
                hw.clock.at(Duration::from_micros(index * 100))
                    ..hw.clock.at(Duration::from_micros((index + 1) * 100)),
            )
        })
        .collect();
    assert_eq!(output, [0.0, -1.0, 0.0, 127.0 / 128.0, -0.5, -0.5, 0.5]);
    hw.clock.advance_by(Duration::from_micros(1_000));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    assert!(hw.io.poll_interrupt(&mut cx).is_pending());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pause_and_continue_preserve_the_position_and_fractional_sample_phase() {
    for auto_initialize in [false, true] {
        let hw = Hardware::new();
        hw.program_dma(0x100, 3);
        hw.io.write_port(MODE_PORT, 1, 0x59);
        hw.command(&[0x40, 159, 0x48, 1, 0]); // 97 us/sample.
        hw.command(if auto_initialize {
            &[0x1c]
        } else {
            &[0x14, 1, 0]
        });
        hw.clock.advance_by(Duration::from_micros(145));
        hw.command(&[0xd0]);
        assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
        hw.clock.advance_by(Duration::from_micros(500));
        hw.command(&[0xd0]); // Repeated pause does not move the paused instant.
        hw.clock.advance_by(Duration::from_micros(500));
        assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
        hw.command(&[0xd4, 0xd4]);
        hw.clock.advance_by(Duration::from_micros(48));
        assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
        hw.clock.advance_by(Duration::from_micros(1));
        assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
        acknowledge(&hw);
        // Muting is the reset default, and has not prevented DMA or completion.
        hw.command(&[0xd8]);
        assert_eq!(hw.io.read_port(READ_DATA_PORT, 1), 0);
        hw.clock.advance_by(Duration::from_micros(97));
        assert_eq!(
            hw.dma_word(ADDRESS_PORTS[1]),
            if auto_initialize { 0x103 } else { 0x102 }
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn switching_to_single_cycle_waits_for_the_auto_initialize_block_boundary() {
    let hw = Hardware::new();
    hw.program_dma(0x100, 3);
    hw.io.write_port(MODE_PORT, 1, 0x59);
    hw.command(&[0x40, 156, 0x48, 1, 0, 0x1c]);
    hw.clock.advance_by(Duration::from_micros(150));
    hw.command(&[0x14, 0, 0]); // One final sample after this two-sample block.
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x101);
    hw.clock.advance_by(Duration::from_micros(50));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x102);
    acknowledge(&hw);
    hw.clock.advance_by(Duration::from_micros(100));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x103);
    acknowledge(&hw);
    hw.clock.advance_by(Duration::from_micros(500));
    assert_eq!(hw.dma_word(ADDRESS_PORTS[1]), 0x103);
}
