use super::*;
use std::{future::Future, pin::pin, task::Poll};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn dma_reprogramming_preserves_samples_from_the_old_buffer() {
    let hw = Hardware::new();
    let mut source = hw.card.borrow().audio_source();
    hw.memory.view().write(0x100, &[0; 4]).unwrap();
    hw.memory.view().write(0x200, &[255; 4]).unwrap();
    hw.program_dma(0x100, 3);
    hw.command(&[0xd1, 0x40, 156, 0x14, 3, 0]);
    hw.clock.advance_by(Duration::from_micros(250));
    hw.program_dma(0x200, 3);
    hw.clock.advance_by(Duration::from_micros(250));
    let output: Vec<_> = (0..5)
        .map(|index| {
            source.sample(
                hw.clock.at(Duration::from_micros(index * 100))
                    ..hw.clock.at(Duration::from_micros((index + 1) * 100)),
            )
        })
        .collect();
    assert_eq!(output, [0.0, -1.0, -1.0, 127.0 / 128.0, 127.0 / 128.0]);
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x202);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn consumed_pcm_survives_guest_buffer_reuse() {
    let hw = Hardware::new();
    let mut source = hw.card.borrow().audio_source();
    hw.memory.view().write(0x100, &[0, 128, 255, 128]).unwrap();
    hw.program_dma(0x100, 3);
    hw.command(&[0xd1, 0x40, 156, 0x14, 3, 0]);
    hw.clock.advance_by(Duration::from_micros(500));
    hw.io.read_port(WRITE_STATUS_PORT, 1);
    hw.memory.view().write(0x100, &[128; 4]).unwrap();
    let output: Vec<_> = (0..5)
        .map(|index| {
            source.sample(
                hw.clock.at(Duration::from_micros(index * 100))
                    ..hw.clock.at(Duration::from_micros((index + 1) * 100)),
            )
        })
        .collect();
    assert_eq!(output, [0.0, -1.0, 0.0, 127.0 / 128.0, 0.0]);
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x104);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_dac_and_mute_share_the_sample_timeline() {
    let hw = Hardware::new();
    let mut source = hw.card.borrow().audio_source();
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
            source.sample(
                hw.clock.at(Duration::from_micros(index * 100))
                    ..hw.clock.at(Duration::from_micros((index + 1) * 100)),
            )
        })
        .collect();
    assert_eq!(output, [-1.0 / 256.0, 0.0, 127.0 / 128.0, 0.0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn full_and_closed_output_do_not_block_dma_or_irqs() {
    let hw = Hardware::new();
    let (samples, output) = async_channel::bounded(1);
    let mut mixer = relocade_mixer::Mixer::new(
        hw.clock.clock(),
        cpal::StreamConfig {
            channels: 1,
            sample_rate: 10_000,
            buffer_size: cpal::BufferSize::Fixed(2),
        },
        samples,
    )
    .unwrap();
    let mut source = hw.card.borrow().audio_source();
    mixer.add_source(0.4, move |interval| source.sample(interval));
    let mut mixing = pin!(relocade_mixer::Mixer::run(Rc::new(RefCell::new(mixer))));
    let mut activity = pin!(SoundBlaster::run(hw.card.clone()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(mixing.as_mut().poll(&mut cx).is_pending());
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.memory
        .view()
        .write(0x100, &[0, 0, 255, 255, 128, 128])
        .unwrap();
    hw.program_dma(0x100, 5);
    hw.command(&[0xd1, 0x40, 156, 0x14, 5, 0]);
    hw.clock.advance_by(Duration::from_micros(600));
    // Rendering may be polled before the device's own activity.
    assert!(mixing.as_mut().poll(&mut cx).is_pending());
    assert_eq!(output.len(), 1);
    assert_eq!(output.try_recv().unwrap(), [0.4 * 127.0 / 128.0, 0.0]);
    assert_eq!(hw.dma_word(relocade_dma::ADDRESS_PORTS[1]), 0x106);
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
    hw.io.read_port(READ_STATUS_PORT, 1);
    relocade_pic::end_of_interrupt(&hw.io, IRQ);
    output.close();
    assert!(mixing.as_mut().poll(&mut cx).is_ready());

    hw.program_dma(0x100, 0);
    hw.command(&[0x14, 0, 0]);
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    hw.clock.advance_by(Duration::from_micros(100));
    assert!(activity.as_mut().poll(&mut cx).is_pending());
    assert_eq!(hw.io.poll_interrupt(&mut cx), Poll::Ready(0x0f));
}
