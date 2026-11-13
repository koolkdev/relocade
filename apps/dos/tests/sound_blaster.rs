#[path = "sound_blaster/program.rs"]
mod program;

use relocade_app::audio::cpal::{BufferSize, StreamConfig};
use relocade_clock_test_support::ManualClock;
use relocade_dos::{Configuration, Dos, LoadOptions, SoundCard};
use relocade_machine::{worker::wake_connection, Memory, Runner};
use std::{
    future::{poll_fn, Future},
    io::Write,
    pin::{pin, Pin},
    task::Poll,
    time::Duration,
};

async fn wait_for_byte(mut run: Pin<&mut impl Future>, memory: &Memory, address: u32, value: u8) {
    poll_fn(|cx| {
        assert!(
            run.as_mut().poll(cx).is_pending(),
            "guest exited before {value}"
        );
        if memory.view().read_u8(address).unwrap() == value {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

fn check_mix(buffer: &[f32], previous: f32, pcm: f32) {
    assert_eq!(buffer.len(), 32);
    // Each DAC byte takes effect at the end of its 125-us sample period.
    // Removing the known PCM leaves the FM carrier plus the 0..1 speaker.
    let remainder: Vec<_> = buffer
        .iter()
        .enumerate()
        .map(|(index, sample)| sample - if index == 0 { previous } else { pcm })
        .collect();
    let low = remainder.iter().copied().fold(f32::INFINITY, f32::min);
    let high = remainder.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(low >= -0.13 && high <= 1.13, "FM + speaker: {low}..{high}");
    assert!(low < -0.02 && high > 1.02, "FM + speaker: {low}..{high}");
}

async fn exercise() {
    let clock = ManualClock::new();
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let (_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let runner = Runner::new(clock.clock(), prepared, endpoint)
        .await
        .unwrap();
    runner
        .run(
            futures_timer::Delay::new(Duration::from_secs(30)),
            async |runner| {
                let mut dos = Dos::with_configuration(
                    runner,
                    Configuration {
                        sound_card: SoundCard::SoundBlaster2,
                    },
                )
                .await
                .unwrap();
                let (samples, output) = async_channel::bounded(1);
                dos.attach_audio(
                    StreamConfig {
                        channels: 1,
                        sample_rate: 8_000,
                        buffer_size: BufferSize::Fixed(32),
                    },
                    samples,
                )
                .await
                .unwrap();
                let memory = dos.cpu().memory();
                let mut input = dos.stdin();
                let vector = memory.view().read_u32(0x0f * 4).unwrap();
                let image = program::image();
                let environment = dos.initial_environment();
                let entries: Vec<_> = environment.iter().map(Vec::as_slice).collect();
                for launch in 0..2 {
                    dos.launch_com(
                        &image,
                        LoadOptions {
                            environment: &entries,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
                    let mut run = pin!(dos.run());
                    wait_for_byte(run.as_mut(), &memory, base + u32::from(program::STAGE), 1).await;
                    // Finish reset on a mixer boundary before the guest starts any sources.
                    clock.advance_by(Duration::from_millis(4));
                    if launch == 0 {
                        assert_eq!(output.recv().await.unwrap(), vec![0.0; 32]);
                    }
                    input.write_all(b"A").unwrap();
                    wait_for_byte(run.as_mut(), &memory, base + u32::from(program::STAGE), 2).await;
                    assert_eq!(
                        memory
                            .view()
                            .read_u8(base + u32::from(program::RESET_RESPONSE))
                            .unwrap(),
                        0xaa
                    );
                    for block in 1..=5 {
                        if launch == 0 && block == 5 {
                            output.close();
                        }
                        clock.advance_by(Duration::from_millis(4));
                        // IRQ7 must interrupt the pending DOS keyboard read and refill memory.
                        wait_for_byte(
                            run.as_mut(),
                            &memory,
                            base + u32::from(program::IRQ_COUNT),
                            block,
                        )
                        .await;
                        if launch == 0 && block <= 4 {
                            let (previous, pcm) = match block {
                                1 => (0.0, -0.5),
                                2 => (-0.5, 0.5),
                                3 => (0.5, -0.25),
                                4 => (-0.25, 0.25),
                                _ => unreachable!(),
                            };
                            check_mix(&output.recv().await.unwrap(), previous, pcm);
                        }
                    }
                    let mut buffer = [0; 64];
                    memory
                        .view()
                        .read(base + u32::from(program::BUFFER), &mut buffer)
                        .unwrap();
                    assert_eq!(&buffer[..32], &[0x60; 32]);
                    assert_eq!(&buffer[32..], &[0xa0; 32]);
                    input.write_all(b"A").unwrap();
                    assert_eq!(run.await, Ok(0));
                    assert_eq!(memory.view().read_u32(0x0f * 4).unwrap(), vector);
                }
            },
        )
        .await
        .expect("DOS Sound Blaster fixture timed out");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_streams_and_refills_sound_blaster_during_dos_waits_and_after_output_closes() {
    pollster::block_on(exercise());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_streams_and_refills_sound_blaster_during_dos_waits_and_after_output_closes() {
    exercise().await;
}
