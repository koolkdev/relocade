use relocade_app::audio::cpal::{BufferSize, StreamConfig};
use relocade_clock_test_support::ManualClock;
use relocade_dos::Dos;
use relocade_machine::{worker::wake_connection, Memory, Runner};
use std::{
    future::{poll_fn, Future},
    io::Write,
    pin::{pin, Pin},
    task::Poll,
    time::Duration,
};

fn write_adlib(code: &mut Vec<u8>, register: u8, value: u8) {
    // MOV DX,388h; MOV AL,register; OUT DX,AL; INC DX; MOV AL,value; OUT DX,AL.
    code.extend_from_slice(&[
        0xba, 0x88, 0x03, 0xb0, register, 0xee, 0x42, 0xb0, value, 0xee,
    ]);
}

fn pause_program(code: &mut Vec<u8>, stage: u8) {
    code.extend_from_slice(&[
        0xc6, 6, 0, 4, stage, // MOV byte [0400h],stage.
        0xb4, 7, 0xcd, 0x21, // DOS read without echo.
    ]);
}

fn detection_program() -> Vec<u8> {
    let mut code = Vec::new();
    write_adlib(&mut code, 4, 0x60);
    write_adlib(&mut code, 4, 0x80);
    code.extend_from_slice(&[0xba, 0x88, 3, 0xec, 0xa2, 1, 4]); // Save initial status.
    write_adlib(&mut code, 2, 0xff);
    write_adlib(&mut code, 4, 0x21);
    pause_program(&mut code, 1);
    code.extend_from_slice(&[0xba, 0x88, 3, 0xec, 0xa2, 2, 4]); // Save overflow status.
    write_adlib(&mut code, 4, 0x60);
    write_adlib(&mut code, 4, 0x80);
    code.extend_from_slice(&[0xb8, 7, 0x4c, 0xcd, 0x21]);
    code
}

fn music_program() -> Vec<u8> {
    let mut code = Vec::new();
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
        (0xb0, 0x31), // Sustained ~309 Hz carrier.
    ] {
        write_adlib(&mut code, register, value);
    }
    pause_program(&mut code, 1); // AdLib only.
    code.extend_from_slice(&[
        0xb0, 0xb6, 0xe6, 0x43, // PIT channel 2, square wave.
        0xb0, 0xa9, 0xe6, 0x42, 0xb0, 4, 0xe6, 0x42, // ~1 kHz.
        0xb0, 3, 0xe6, 0x61,
    ]);
    pause_program(&mut code, 2); // Both devices.
    write_adlib(&mut code, 0xb0, 0x11);
    pause_program(&mut code, 3); // Speaker only, after AdLib's release.
    code.extend_from_slice(&[0xb0, 0, 0xe6, 0x61]);
    pause_program(&mut code, 4); // Silence.
    code.extend_from_slice(&[0xb8, 9, 0x4c, 0xcd, 0x21]);
    code
}

async fn wait_for_stage(mut run: Pin<&mut impl Future>, memory: &Memory, base: u32, stage: u8) {
    poll_fn(|cx| {
        assert!(
            run.as_mut().poll(cx).is_pending(),
            "guest exited before stage {stage}"
        );
        if memory.view().read_u8(base + 0x400).unwrap() == stage {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
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
                let mut dos = Dos::new(runner).await.unwrap();
                let memory = dos.cpu().memory();
                let mut input = dos.stdin();

                // The first process detects AdLib before any output is attached.
                dos.launch_com(&detection_program(), Default::default())
                    .unwrap();
                let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
                memory.view().write_u8(base + 0x400, 0).unwrap();
                {
                    let mut run = pin!(dos.run());
                    wait_for_stage(run.as_mut(), &memory, base, 1).await;
                    clock.advance_by(Duration::from_micros(80));
                    input.write_all(b"A").unwrap();
                    assert_eq!(run.await.unwrap(), 7);
                }
                assert_eq!(memory.view().read_u8(base + 0x401).unwrap(), 0x06);
                assert_eq!(memory.view().read_u8(base + 0x402).unwrap(), 0xc6);

                // Capture the production mixer output on both native and Wasm, without
                // depending on a platform playback device or an AudioWorklet.
                let (samples, buffers) = async_channel::bounded(1);
                dos.attach_audio(
                    StreamConfig {
                        channels: 1,
                        sample_rate: 48_000,
                        buffer_size: BufferSize::Fixed(256),
                    },
                    samples,
                )
                .await
                .unwrap();
                dos.launch_com(&music_program(), Default::default())
                    .unwrap();
                let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
                memory.view().write_u8(base + 0x400, 0).unwrap();
                let mut run = pin!(dos.run());
                for stage in 1..=4 {
                    wait_for_stage(run.as_mut(), &memory, base, stage).await;
                    // Advance settled audio while the process remains in its DOS read.
                    clock.advance_by(Duration::from_millis(40));
                    let buffer = buffers.recv().await.unwrap();
                    assert_eq!(buffer.len(), 256);
                    let low = buffer.iter().copied().fold(f32::INFINITY, f32::min);
                    let high = buffer.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    match stage {
                        1 => {
                            assert!(low < -0.05 && high > 0.05, "AdLib: {low}..{high}");
                            assert!(low >= -0.11 && high <= 0.11);
                        }
                        2 => {
                            assert!(low < -0.02 && high > 0.22, "mixed: {low}..{high}");
                            assert!(low >= -0.8 && high <= 1.0);
                        }
                        3 => {
                            assert!(low < 0.01 && high > 0.19, "speaker: {low}..{high}");
                            assert!(low >= -0.001 && high <= 0.201);
                        }
                        // Released OPL operators can retain a few PCM quantization units.
                        4 => assert!(low >= -0.001 && high <= 0.001),
                        _ => unreachable!(),
                    }
                    input.write_all(b"A").unwrap();
                }
                assert_eq!(run.await.unwrap(), 9);
            },
        )
        .await
        .expect("DOS AdLib fixture timed out");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_adlib_and_speaker_play_across_dos_keyboard_waits() {
    pollster::block_on(exercise());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_adlib_and_speaker_play_across_dos_keyboard_waits() {
    exercise().await;
}
