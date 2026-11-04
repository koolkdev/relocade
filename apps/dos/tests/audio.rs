#[path = "audio/output.rs"]
mod output;

use async_channel::{Receiver, Sender};
use futures_lite::future::{or, zip};
use relocade_app::audio::cpal::StreamConfig;
use relocade_dos::Dos;
use relocade_machine::{timer_service, ExecutionWorker, TimerHandle};
use std::{
    future::{pending, poll_fn, Future},
    io::Write,
    pin::pin,
    time::Duration,
};

// Two pitches, each held across a DOS keyboard read, then speaker off and exit.
const FIRST: &[u8] = &[
    0xb0, 0xb6, 0xe6, 0x43, // PIT channel 2, low/high bytes, mode 3.
    0xb0, 0xa9, 0xe6, 0x42, 0xb0, 4, 0xe6, 0x42, // Divisor 1193: about 1000 Hz.
    0xe4, 0x61, 0x0c, 3, 0xe6, 0x61, // Enable timer and speaker through IN/OUT.
    0xc6, 6, 0, 4, 1, // Stage 1.
    0xb4, 7, 0xcd, 0x21, // Read without echo.
    0xb0, 0xb6, 0xe6, 0x43, 0xb0, 0x54, 0xe6, 0x42, 0xb0, 2, 0xe6,
    0x42, // Divisor 596: about 2002 Hz.
    0xc6, 6, 0, 4, 2, // Stage 2.
    0xb4, 7, 0xcd, 0x21, 0xe4, 0x61, 0x24, 0xfc, 0xe6, 0x61, // Disable speaker and timer.
    0xb8, 7, 0x4c, 0xcd, 0x21,
];
// A later process deliberately leaves a 500 Hz tone enabled when it exits.
const SECOND: &[u8] = &[
    0xb0, 0xb6, 0xe6, 0x43, 0xb0, 0x52, 0xe6, 0x42, 0xb0, 9, 0xe6, 0x42, // Divisor 2386.
    0xe4, 0x61, 0x0c, 3, 0xe6, 0x61, 0xb8, 9, 0x4c, 0xcd, 0x21,
];

struct Session {
    worker: ExecutionWorker<()>,
    stages: Receiver<u8>,
    keys: Sender<u8>,
    exits: Receiver<u8>,
    launch: Sender<()>,
}

impl Session {
    fn start(clock: TimerHandle, config: StreamConfig, samples: Sender<Vec<f32>>) -> Self {
        let (stage_changed, stages) = async_channel::bounded(2);
        let (keys, input) = async_channel::bounded(1);
        let (done, exits) = async_channel::bounded(1);
        let (launch, next) = async_channel::bounded(1);
        let worker = ExecutionWorker::spawn(clock, (), async move |runner, ()| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.speaker()
                .borrow_mut()
                .attach_audio(config, samples)
                .unwrap();
            dos.launch_com(FIRST, Default::default()).unwrap();
            let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
            let memory = dos.cpu().memory();
            let mut stdin = dos.stdin();
            or(
                async {
                    let exit = {
                        let mut run = pin!(dos.run(runner));
                        let mut previous = 0;
                        poll_fn(|cx| {
                            let result = run.as_mut().poll(cx);
                            // Guest memory stays on its owner. Report progress
                            // after execution yields, including keyboard waits.
                            let stage = memory.view().read_u8(base + 0x400).unwrap();
                            if stage != previous {
                                stage_changed.try_send(stage).unwrap();
                                previous = stage;
                            }
                            result
                        })
                        .await
                        .unwrap()
                    };
                    done.send(exit).await.unwrap();
                    next.recv().await.unwrap();
                    dos.launch_com(SECOND, Default::default()).unwrap();
                    done.send(dos.run(runner).await.unwrap()).await.unwrap();
                    pending::<()>().await;
                },
                async {
                    while let Ok(key) = input.recv().await {
                        stdin.write_all(&[key]).unwrap();
                    }
                    pending::<()>().await;
                },
            )
            .await;
        });
        Self {
            worker,
            stages,
            keys,
            exits,
            launch,
        }
    }

    async fn stage(&self, expected: u8) {
        assert_eq!(self.stages.recv().await.unwrap(), expected);
    }
}

async fn exercise() {
    let (clock, timers) = timer_service();
    let main = async move {
        let (output, config, samples) = output::Output::open();
        let session = Session::start(clock.clone(), config, samples.clone());
        session.stage(1).await;
        while samples.len() != samples.capacity().unwrap() {
            futures_timer::Delay::new(Duration::from_millis(5)).await;
        }
        // The full, unplayed output has not blocked guest programming or input.
        session.keys.send(b'A').await.unwrap();
        session.stage(2).await;
        assert!(session.exits.is_empty());
        output.play();
        output.check(config.sample_rate, 2002).await;
        session.keys.send(b'B').await.unwrap();
        assert_eq!(session.exits.recv().await.unwrap(), 7);
        output.check(config.sample_rate, 0).await;

        session.launch.send(()).await.unwrap();
        assert_eq!(session.exits.recv().await.unwrap(), 9);
        output.check(config.sample_rate, 500).await;
        assert_eq!(session.worker.stop().await.unwrap(), None);
        assert!(session.keys.is_closed());
        assert!(!samples.is_closed(), "output outlives execution");
        output.check(config.sample_rate, 0).await;

        // The same platform output accepts another independent DOS session.
        let replacement = Session::start(clock, config, samples.clone());
        replacement.stage(1).await;
        output.check(config.sample_rate, 1000).await;
        drop(output);
        assert!(samples.is_closed());
        replacement.keys.send(b'A').await.unwrap();
        replacement.stage(2).await;
        replacement.keys.send(b'B').await.unwrap();
        assert_eq!(replacement.exits.recv().await.unwrap(), 7);
        assert_eq!(replacement.worker.stop().await.unwrap(), None);
    };
    or(
        async {
            zip(timers.run(), main).await;
        },
        async {
            futures_timer::Delay::new(Duration::from_secs(120)).await;
            panic!("DOS audio integration timed out");
        },
    )
    .await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn speaker_audio_survives_input_processes_and_session_replacement() {
    pollster::block_on(exercise());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn speaker_audio_survives_input_processes_and_session_replacement() {
    exercise().await;
}
