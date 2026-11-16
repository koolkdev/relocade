use super::*;
use relocade_frontend::audio::AudioTiming;
use std::{future::Future, pin::pin, task::Poll};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn registered_sources_include_both_fm_and_digital_output() {
    let [fm, mixed] = [false, true].map(|digital| {
        let hw = Hardware::new();
        let mut sources = relocade_frontend::audio::Sources::new(
            AudioTiming::new(hw.clock.clock().now(), hw.clock.clock().tick_rate(), 10_000).unwrap(),
        );
        hw.card.borrow().add_audio_sources(&mut sources).unwrap();
        for (register, value) in [
            (0x20, 0x21),
            (0x23, 0x21),
            (0x40, 0x3f),
            (0x43, 0),
            (0x60, 0xf0),
            (0x63, 0xf0),
            (0x80, 0x0f),
            (0x83, 0x0f),
            (0xa0, 0x98),
            (0xb0, 0x31),
        ] {
            hw.io.write_port(FM_ADDRESS_STATUS_PORT, 1, register);
            hw.io.write_port(FM_DATA_PORT, 1, value);
        }
        if digital {
            hw.command(&[0xd1, 0x10, 0xc0]); // Held DAC output of +0.5.
        }
        hw.clock.advance_by(Duration::from_millis(40));
        sources.record(0..400, 1)()
    });
    let low = fm.iter().copied().fold(f32::INFINITY, f32::min);
    let high = fm.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        high - low > 0.01,
        "FM source must produce the programmed tone"
    );
    for (fm, mixed) in fm.into_iter().zip(mixed) {
        assert!((mixed - fm - 0.5).abs() < 0.000001);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn timed_silence_zeros_the_dac_and_completes_without_consuming_dma() {
    let hw = Hardware::new();
    let (mut recorder, mut source) = hw.card.borrow().audio_source(
        AudioTiming::new(hw.clock.clock().now(), hw.clock.clock().tick_rate(), 10_000).unwrap(),
    );
    hw.program_dma(0x100, 3);
    hw.command(&[0xd1, 0x10, 0, 0x40, 156]);
    hw.clock.advance_by(Duration::from_micros(50));
    hw.command(&[0x80, 2, 0]); // Three 100 us periods, completing at 350 us.
    let mut activity = pin!(SoundBlaster::run(hw.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(299));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert!(hw.io.poll_interrupt(&mut cx).is_pending());
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x100);
    hw.clock.advance_by(Duration::from_micros(1));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.acknowledge_irq();
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x100);
    assert_eq!(hw.dma_word(relocade_dma::COUNT_PORTS[1]), 3);
    hw.clock.advance_by(Duration::from_micros(50));
    let output: Vec<_> = (0..4)
        .map(|index| {
            let mut mono = [0.0];
            source.generate_samples(recorder.take_events(), index..index + 1, &mut mono);
            mono[0]
        })
        .collect();
    assert_eq!(output, [-0.5, 0.0, 0.0, 0.0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dma_reprogramming_preserves_samples_from_the_old_buffer() {
    let hw = Hardware::new();
    let (mut recorder, mut source) = hw.card.borrow().audio_source(
        AudioTiming::new(hw.clock.clock().now(), hw.clock.clock().tick_rate(), 10_000).unwrap(),
    );
    hw.memory.view().write(0x100, &[0; 4]).unwrap();
    hw.memory.view().write(0x200, &[255; 4]).unwrap();
    hw.program_dma(0x100, 3);
    hw.command(&[0xd1, 0x40, 156, 0x14, 3, 0]);
    hw.clock.advance_by(Duration::from_micros(250));
    hw.program_dma(0x200, 3);
    hw.clock.advance_by(Duration::from_micros(250));
    let output: Vec<_> = (0..5)
        .map(|index| {
            let mut mono = [0.0];
            source.generate_samples(recorder.take_events(), index..index + 1, &mut mono);
            mono[0]
        })
        .collect();
    assert_eq!(output, [0.0, -1.0, -1.0, 127.0 / 128.0, 127.0 / 128.0]);
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x202);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn consumed_pcm_survives_guest_buffer_reuse() {
    let hw = Hardware::new();
    let (mut recorder, mut source) = hw.card.borrow().audio_source(
        AudioTiming::new(hw.clock.clock().now(), hw.clock.clock().tick_rate(), 10_000).unwrap(),
    );
    hw.memory.view().write(0x100, &[0, 128, 255, 128]).unwrap();
    hw.program_dma(0x100, 3);
    hw.command(&[0xd1, 0x40, 156, 0x14, 3, 0]);
    hw.clock.advance_by(Duration::from_micros(500));
    let events = recorder.take_events();
    drop(recorder);
    hw.memory.view().write(0x100, &[128; 4]).unwrap();
    let mut output = [0.0; 5];
    source.generate_samples(events, 0..5, &mut output);
    assert_eq!(output, [0.0, -1.0, 0.0, 127.0 / 128.0, 0.0]);
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x104);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_dac_and_mute_share_the_sample_timeline() {
    let hw = Hardware::new();
    let (mut recorder, mut source) = hw.card.borrow().audio_source(
        AudioTiming::new(hw.clock.clock().now(), hw.clock.clock().tick_rate(), 10_000).unwrap(),
    );
    hw.command(&[0xd1, 0x10, 0]);
    hw.clock.advance_by(Duration::from_micros(50));
    hw.command(&[0x10, 255]);
    hw.clock.advance_by(Duration::from_micros(50));
    hw.command(&[0xd3]);
    hw.clock.advance_by(Duration::from_micros(100));
    hw.command(&[0xd1]);
    hw.clock.advance_by(Duration::from_micros(100));
    hw.io.write_port(RESET_PORT, 1, 1);
    hw.clock.advance_by(Duration::from_micros(100));
    let output: Vec<_> = (0..4)
        .map(|index| {
            let mut mono = [0.0];
            source.generate_samples(recorder.take_events(), index..index + 1, &mut mono);
            mono[0]
        })
        .collect();
    assert_eq!(output, [-1.0 / 256.0, 0.0, 127.0 / 128.0, 0.0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn taking_events_synchronizes_dma_and_dropping_recorder_keeps_irqs_active() {
    let hw = Hardware::new();
    let (mut recorder, mut source) = hw.card.borrow().audio_source(
        AudioTiming::new(hw.clock.clock().now(), hw.clock.clock().tick_rate(), 10_000).unwrap(),
    );
    let mut activity = pin!(SoundBlaster::run(hw.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.memory
        .view()
        .write(0x100, &[0, 0, 255, 255, 128, 128])
        .unwrap();
    hw.program_dma(0x100, 5);
    hw.command(&[0xd1, 0x40, 156, 0x14, 5, 0]);
    hw.clock.advance_by(Duration::from_micros(600));
    // Event capture synchronizes hardware before its own activity is polled.
    let events = recorder.take_events();
    let mut output = [0.0; 2];
    source.generate_samples(events, 4..6, &mut output);
    assert_eq!(output, [127.0 / 128.0, 0.0]);
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x106);
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
    hw.io.read_port(READ_STATUS_PORT, 1);
    relocade_pic::end_of_interrupt(&hw.io, IRQ);
    drop(recorder);

    hw.program_dma(0x100, 0);
    hw.command(&[0x14, 0, 0]);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(100));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
}
