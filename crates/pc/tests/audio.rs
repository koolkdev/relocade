use relocade_machine::{timer_service, Device};
use relocade_pc::{Configuration, Pc, SoundCard};
use std::{cell::RefCell, future::Future, rc::Rc, time::Duration};

struct Started(Option<async_channel::Sender<()>>);

impl Started {
    fn configure_io(io: &mut relocade_machine::DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        let callbacks = relocade_machine::DeviceCallbacks::new(device);
        io.bind(
            0x80,
            &relocade_machine::PortRead::new(|_, _, _| u32::MAX),
            &relocade_machine::PortWrite::low_byte(callbacks.write(|_, device, _| device.signal())),
        );
    }
    fn signal(&mut self) {
        self.0.take().unwrap().try_send(()).unwrap();
    }
}

impl Device for Started {}

async fn timeout<T>(future: impl Future<Output = T>) -> T {
    futures_lite::future::or(future, async {
        futures_timer::Delay::new(Duration::from_secs(30)).await;
        panic!("PC audio fixture timed out");
    })
    .await
}

async fn exercise_audio_device(sound_card: SoundCard) {
    let (samples, buffers) = async_channel::bounded(1);
    let (started, ready) = async_channel::bounded(1);
    let (clock, timers) = timer_service();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let worker = prepared.spawn(clock, async move |runner| {
        let started = Rc::new(RefCell::new(Started(Some(started))));
        runner
            .connect(started.clone(), |io| {
                Started::configure_io(io, &started);
            })
            .await;
        let mut pc = Pc::with_configuration(runner, Configuration { sound_card })
            .await
            .unwrap();
        let expected_status = match sound_card {
            SoundCard::None => 0xff,
            SoundCard::AdLib => 0x06,
        };
        assert_eq!(pc.cpu().context().io.read_port(0x388, 1), expected_status);
        assert_eq!(pc.adlib().is_some(), sound_card == SoundCard::AdLib);
        let memory = pc.cpu().memory();
        // A sustained ~309 Hz AdLib carrier, programmed through guest OUT DX,AL.
        let mut code = vec![0xba, 0x88, 0x03]; // DX = address/status port.
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
            code.extend_from_slice(&[0xb0, register, 0xee, 0x42, 0xb0, value, 0xee, 0x4a]);
        }
        // Add ~1 kHz speaker output, notify port 80, then stay in guest code.
        code.extend_from_slice(&[
            0xb0, 0xb6, 0xe6, 0x43, 0xb0, 0xa9, 0xe6, 0x42, 0xb0, 0x04, 0xe6, 0x42, 0xb0, 3, 0xe6,
            0x61, 0xe6, 0x80, 0xeb, 0xfe,
        ]);
        memory.view().write(0x1000, &code).unwrap();
        pc.cpu().cpu_state().set_eip(0x1000);
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: 48_000,
            buffer_size: cpal::BufferSize::Fixed(256),
        };
        pc.attach_audio(config, samples.clone()).await.unwrap();
        assert!(pc.attach_audio(config, samples).await.is_err());
        panic!("busy guest unexpectedly returned: {:?}", pc.run().await);
    });
    let timers = timers.spawn_with_peer(worker.wake_peer());
    timeout(ready.recv()).await.unwrap();
    // Initial queued silence can precede the guest's programming writes.
    for index in 0..12 {
        let buffer = timeout(buffers.recv()).await.unwrap();
        assert_eq!(buffer.len(), 256);
        if index >= 2 {
            match sound_card {
                SoundCard::AdLib => {
                    assert!(buffer.iter().all(|sample| (-0.8..=1.0).contains(sample)));
                    // AdLib alone peaks below 0.1; speaker alone cannot go negative.
                    // Both must contribute to the same buffer.
                    assert!(buffer.iter().any(|&sample| sample > 0.22));
                    assert!(buffer.iter().any(|&sample| sample < -0.02));
                }
                SoundCard::None => {
                    assert!(buffer.iter().all(|sample| (0.0..=0.2).contains(sample)));
                    assert!(buffer.iter().any(|&sample| sample > 0.19));
                    assert!(buffer.iter().any(|&sample| sample < 0.01));
                }
            }
        }
        // A playback device consumes 256 frames in about 5.3 ms.
        futures_timer::Delay::new(Duration::from_millis(6)).await;
    }
    assert!(timeout(worker.stop()).await.unwrap().is_none());
    assert!(
        buffers.is_closed(),
        "session shutdown drops the mixer's sender"
    );
    timeout(timers.join()).await.unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_mixes_adlib_and_speaker_while_guest_execution_is_busy() {
    pollster::block_on(exercise_audio_device(SoundCard::AdLib));
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_mixes_adlib_and_speaker_while_guest_execution_is_busy() {
    exercise_audio_device(SoundCard::AdLib).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_without_a_sound_card_still_plays_the_speaker() {
    pollster::block_on(exercise_audio_device(SoundCard::None));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_without_a_sound_card_still_plays_the_speaker() {
    exercise_audio_device(SoundCard::None).await;
}
