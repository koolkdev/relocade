#[path = "audio/output.rs"]
mod output;

use futures_lite::future::{or, zip};
use relocade_dos::Dos;
use relocade_machine::{timer_service, ExecutionWorker};
use std::{
    future::{pending, poll_fn, Future},
    io::Write,
    pin::pin,
    time::Duration,
};

// Hold a tone across a DOS keyboard read, then silence across another read.
const PROGRAM: &[u8] = &[
    0xb0, 0xb6, 0xe6, 0x43, // PIT channel 2, low/high bytes, mode 3.
    0xb0, 0xa9, 0xe6, 0x42, 0xb0, 4, 0xe6, 0x42, // Divisor 1193: about 1000 Hz.
    0xe4, 0x61, 0x0c, 3, 0xe6, 0x61, // Enable timer and speaker through IN/OUT.
    0xc6, 6, 0, 4, 1, // Stage 1.
    0xb4, 7, 0xcd, 0x21, // Read without echo.
    0xe4, 0x61, 0x24, 0xfc, 0xe6, 0x61, // Disable speaker and timer.
    0xc6, 6, 0, 4, 2, // Stage 2.
    0xb4, 7, 0xcd, 0x21, // Wait again so silence is observed before shutdown.
    0xb8, 7, 0x4c, 0xcd, 0x21,
];

async fn exercise() {
    let (clock, timers) = timer_service();
    let main = async move {
        let (output, config, samples) = output::Output::open();
        let (stage_changed, stages) = async_channel::bounded(2);
        let (keys, input) = async_channel::bounded(1);
        let worker = ExecutionWorker::spawn(clock, async move |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.speaker()
                .borrow_mut()
                .attach_audio(config, samples)
                .unwrap();
            dos.launch_com(PROGRAM, Default::default()).unwrap();
            let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
            let memory = dos.cpu().memory();
            let mut stdin = dos.stdin();
            or(
                async {
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
                    .unwrap();
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
        let result = async {
            assert_eq!(stages.recv().await.unwrap(), 1);
            output.play();
            output.check(config.sample_rate, 1000).await?;
            keys.send(b'A').await.unwrap();
            assert_eq!(stages.recv().await.unwrap(), 2);
            output.check(config.sample_rate, 0).await
        }
        .await;
        // Keep timers alive until execution stops, including a failed audio check.
        assert_eq!(worker.stop().await.unwrap(), None);
        result.unwrap();
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
fn guest_speaker_plays_and_stops_during_dos_keyboard_waits() {
    pollster::block_on(exercise());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_speaker_plays_and_stops_during_dos_keyboard_waits() {
    exercise().await;
}
