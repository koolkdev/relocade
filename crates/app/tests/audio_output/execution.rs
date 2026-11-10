use super::{timeout, Audio};
use futures_channel::oneshot;
use futures_lite::future::poll_once;
use relocade_machine::{
    timer_service, CpuConfig, Device, ExecutionInterrupt, ExecutionWorker, IoBus, MachineConfig,
    Memory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{cell::RefCell, pin::pin, rc::Rc};

struct Started {
    ready: Option<oneshot::Sender<()>>,
    interrupt: Option<ExecutionInterrupt>,
}

impl Device for Started {
    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {
        // Ignore setup wakes before the callback under test can run.
        self.interrupt.as_ref().unwrap().take();
        self.ready.take().unwrap().send(()).unwrap();
    }
}

async fn callback_wakes_execution() {
    let (audio, config, samples) = Audio::open(|error| panic!("{error}")).unwrap();
    let channels = usize::from(config.channels);
    for _ in 0..samples.capacity().unwrap() {
        samples.try_send(vec![0.0; 128 * channels]).unwrap();
    }
    let (ready, registered) = oneshot::channel();
    let (clock, _timers) = timer_service();
    let worker = ExecutionWorker::spawn(clock, async move |mut runner| {
        let memory = Memory::new(
            MachineConfig::new(
                4096,
                PhysicalMemoryMap::new([(0..=4095, PhysicalMapping::Ram { backing_offset: 0 })])
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        // OUT confirms actual guest entry before the host starts playback.
        memory
            .view()
            .write(0x100, &[0xe6, 0x80, 0xeb, 0xfe])
            .unwrap();
        let started = Rc::new(RefCell::new(Started {
            ready: Some(ready),
            interrupt: None,
        }));
        let io = IoBus::new([runner
            .connect(started.clone(), [], std::iter::once(0x80..=0x80))
            .await]);
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let config = CpuConfig::new(
            compiler.target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..CpuState::default()
            },
        )
        .unwrap();
        let cpu = runner.create_cpu(memory, config, io).await.unwrap();
        started.borrow_mut().interrupt = Some(cpu.interrupt_handle());
        let module = compiler.compile_block(0x102, &[0xeb, 0xfe], 1).unwrap();
        cpu.install_module(
            &module.bytes,
            &[relocade_machine::DispatchEntry {
                address: 0x102,
                export: &module.entry,
            }],
        )
        .await
        .unwrap();

        let mut submission = pin!(samples.send(vec![0.25; 128 * channels]));
        assert!(
            poll_once(submission.as_mut()).await.is_none(),
            "full output must register backpressure"
        );
        // No timer, input or cooperative yield can supply this request. The
        // real CPAL callback must free channel capacity while Wasm stays busy.
        assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
        submission.await.unwrap();
    });
    timeout(registered).await.unwrap();
    audio.play().unwrap();
    assert_eq!(timeout(worker.join()).await.unwrap(), Some(()));
    drop(audio);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a default audio output; CI uses ALSA null"]
fn output_backpressure_interrupts_busy_guest_execution() {
    pollster::block_on(callback_wakes_execution());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
#[ignore = "AudioWorklet bugs: https://github.com/koolkdev/relocade/issues/970 and https://github.com/koolkdev/relocade/issues/971"]
async fn output_backpressure_interrupts_busy_guest_execution() {
    callback_wakes_execution().await;
}
