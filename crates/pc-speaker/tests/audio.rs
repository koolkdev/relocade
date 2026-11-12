use relocade_machine::{timer_service, CpuConfig, Device, MachineConfig};
use relocade_pc_speaker::Speaker;
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};
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
        panic!("speaker worker fixture timed out");
    })
    .await
}

async fn exercise_audio_device() {
    let (samples, buffers) = async_channel::bounded(1);
    let (started, ready) = async_channel::bounded(1);
    let (clock, timers) = timer_service();
    let mut machine = relocade_machine::Machine::new(
        MachineConfig::new(
            4096,
            PhysicalMemoryMap::new([(0..=4095, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let cpu_config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::real_mode()
        },
    )
    .unwrap();
    let prepared = machine.create_cpu(cpu_config, None).unwrap();
    let worker = prepared.spawn(clock, async move |mut runner| {
        let pit = relocade_pit::Pit::initialize(&runner, &relocade_pic::Pic::default()).await;
        let speaker = Speaker::initialize(&runner, pit).await;
        let started = Rc::new(RefCell::new(Started(Some(started))));
        runner
            .connect(started.clone(), |io| {
                Started::configure_io(io, &started);
            })
            .await;
        let memory = runner.memory();
        // Program ~1 kHz, notify the test through port 80, then stay in guest code.
        let code = [
            0xb0, 0xb6, 0xe6, 0x43, 0xb0, 0xa9, 0xe6, 0x42, 0xb0, 0x04, 0xe6, 0x42, 0xb0, 3, 0xe6,
            0x61, 0xe6, 0x80, 0xeb, 0xfe,
        ];
        memory.view().write(0x100, &code).unwrap();

        speaker
            .borrow_mut()
            .attach_audio(
                cpal::StreamConfig {
                    channels: 1,
                    sample_rate: 48_000,
                    buffer_size: cpal::BufferSize::Fixed(256),
                },
                samples,
            )
            .unwrap();
        panic!(
            "busy guest unexpectedly returned: {:?}",
            runner.run_cpu().await
        );
    });
    let timers = timers.spawn_with_peer(worker.wake_peer());
    timeout(ready.recv()).await.unwrap();
    // Initial queued silence can precede the guest's programming writes.
    for index in 0..12 {
        let buffer = timeout(buffers.recv()).await.unwrap();
        assert_eq!(buffer.len(), 256);
        if index >= 2 {
            assert!(buffer.iter().any(|&sample| sample > 0.0));
        }
        // A playback device consumes 256 frames in about 5.3 ms.
        futures_timer::Delay::new(Duration::from_millis(6)).await;
    }
    assert!(timeout(worker.stop()).await.unwrap().is_none());
    assert!(
        buffers.is_closed(),
        "session shutdown drops the device's sender"
    );
    timeout(timers.join()).await.unwrap();
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn speaker_activity_submits_while_guest_execution_is_busy() {
    pollster::block_on(exercise_audio_device());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn speaker_activity_submits_while_guest_execution_is_busy() {
    exercise_audio_device().await;
}
