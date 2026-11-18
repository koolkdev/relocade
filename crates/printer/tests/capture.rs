use futures_lite::{future::poll_once, io::AsyncWriteExt};
use relocade_clock_test_support::ManualClock;
use relocade_machine::{worker::wake_connection, Runner};
use relocade_parallel::Centronics;
use relocade_pc::{Configuration, ParallelPort, Pc};
use relocade_printer::Capture;
use std::{num::NonZeroUsize, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn capture_lifecycle() {
    let clock = ManualClock::default();
    let mut machine = relocade_machine::Machine::new(relocade_pc::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_pc::configuration::cpu(), None)
        .unwrap();
    let (_host, endpoint) = wake_connection(prepared.interrupt_handle());
    let runner = Runner::new(clock.clock(), prepared, endpoint)
        .await
        .unwrap();
    runner
        .run(
            futures_timer::Delay::new(Duration::from_secs(30)),
            async |runner| {
                let mut pc = Pc::with_configuration(
                    runner,
                    Configuration {
                        parallel_port: Some(ParallelPort {
                            base: 0x378,
                            irq: 5,
                        }),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let port = pc.parallel_port().unwrap();
                let (input, mut output) = Centronics::connect(pc.runner(), &port).await;
                let capture =
                    Capture::attach(pc.runner(), input, NonZeroUsize::new(4).unwrap()).await;
                let io = pc.cpu().context().io.clone();

                // A raw port byte and direct bytes reach the same capture unchanged.
                io.write_port(0x378, 1, 0xff);
                io.write_port(0x37a, 1, 0x0d);
                io.write_port(0x37a, 1, 0x0c);
                assert_eq!(capture.snapshot().await.unwrap(), [0xff]);
                clock.advance_by(Duration::from_micros(5));
                output.write_all(b"\x00\x1b\x0c").await.unwrap();
                assert_eq!(capture.snapshot().await.unwrap(), [0xff, 0, 0x1b, 0x0c]);
                assert_eq!(capture.snapshot().await.unwrap(), [0xff, 0, 0x1b, 0x0c]);
                assert!(!output.ready());
                assert_eq!(
                    io.read_port(0x379, 1) & 0xa0,
                    0,
                    "full is BUSY, not paper empty"
                );

                // INIT preserves captured output. A full buffer waits without advancing
                // time; Clear releases the blocked writer and starts a fresh job.
                io.write_port(0x37a, 1, 0x08);
                io.write_port(0x37a, 1, 0x0c);
                let before = clock.clock().now();
                {
                    let mut pending = std::pin::pin!(output.write_all(b"B"));
                    assert!(poll_once(pending.as_mut()).await.is_none());
                    assert_eq!(capture.snapshot().await.unwrap(), [0xff, 0, 0x1b, 0x0c]);
                    capture.clear().await.unwrap();
                    pending.await.unwrap();
                }
                assert_eq!(clock.clock().now(), before);
                assert_eq!(capture.snapshot().await.unwrap(), b"B");

                // Cancellation of a queued snapshot cannot consume the retained job.
                assert!(poll_once(capture.snapshot()).await.is_none());
                assert_eq!(capture.snapshot().await.unwrap(), b"B");
                capture.clear().await.unwrap();
                assert!(capture.snapshot().await.unwrap().is_empty());
                output.write_all(b"C").await.unwrap();
                // Clear also discards bytes accepted by the stream but not read yet.
                capture.clear().await.unwrap();
                assert!(capture.snapshot().await.unwrap().is_empty());
                output.write_all(b"D").await.unwrap();
                output.close().await.unwrap();
                assert_eq!(capture.snapshot().await.unwrap(), b"D");
            },
        )
        .await
        .expect("capture lifecycle timed out");
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn capture_preserves_raw_bytes_and_releases_backpressure_on_clear() {
    pollster::block_on(capture_lifecycle());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn capture_preserves_raw_bytes_and_releases_backpressure_on_clear() {
    capture_lifecycle().await;
}
