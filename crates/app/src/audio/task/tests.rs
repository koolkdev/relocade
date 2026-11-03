use super::DeviceTask;
use async_channel::Sender;
use futures_lite::future::{or, poll_once};
use std::{
    future::Future,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        Arc,
    },
    time::Duration,
};

#[derive(Default)]
struct Counts {
    reads: AtomicUsize,
    dropped: AtomicBool,
}

struct Source {
    input: async_channel::Receiver<Result<f32, String>>,
    counts: Arc<Counts>,
    owner: std::thread::ThreadId,
    // Neither this device nor its running future is Send.
    local: Rc<()>,
}

impl Source {
    async fn run(self, samples: Sender<Vec<f32>>) -> Result<(), String> {
        // This device chooses its own batch and submits directly to the output.
        loop {
            self.counts.reads.fetch_add(1, SeqCst);
            let value = self.input.recv().await.map_err(|e| e.to_string())??;
            if samples.send(vec![value, -value]).await.is_err() {
                return Ok(());
            }
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        assert_eq!(self.owner, std::thread::current().id());
        assert_eq!(Rc::strong_count(&self.local), 1);
        self.counts.dropped.store(true, SeqCst);
    }
}

async fn timeout<T>(future: impl Future<Output = T>) -> T {
    or(future, async {
        futures_timer::Delay::new(Duration::from_secs(5)).await;
        panic!("audio fixture timed out");
    })
    .await
}

async fn until(mut ready: impl FnMut() -> bool) {
    timeout(async {
        while !ready() {
            futures_timer::Delay::new(Duration::from_millis(2)).await;
        }
    })
    .await;
}

async fn exercise_submission_and_shutdown() {
    let config = config();
    let (samples, receiver) = async_channel::bounded(1);
    let (input, incoming) = async_channel::bounded(1);
    let counts = Arc::new(Counts::default());
    let worker_counts = counts.clone();
    let main = std::thread::current().id();
    let mut device = DeviceTask::attach(config, samples, move |config, samples| async move {
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.channels, 2);
        let owner = std::thread::current().id();
        assert_ne!(owner, main);
        Source {
            input: incoming,
            counts: worker_counts,
            owner,
            local: Rc::new(()),
        }
        .run(samples)
        .await
    });
    until(|| counts.reads.load(SeqCst) == 1).await;
    // Cancelling a join wait leaves the device active for subsequent input.
    assert!(poll_once(device.join()).await.is_none());
    input.send(Ok(0.25)).await.unwrap();
    until(|| counts.reads.load(SeqCst) == 2).await;
    input.send(Ok(0.5)).await.unwrap();
    until(|| input.is_empty()).await;
    futures_timer::Delay::new(Duration::from_millis(30)).await;
    assert_eq!(
        counts.reads.load(SeqCst),
        2,
        "the device waits for submission before reading another input"
    );
    assert_eq!(receiver.recv().await.unwrap(), vec![0.25, -0.25]);
    until(|| counts.reads.load(SeqCst) == 3).await;
    assert_eq!(receiver.recv().await.unwrap(), vec![0.5, -0.5]);
    assert!(receiver.is_empty());
    // An interrupted close retains its request and keeps the worker alive.
    device.request_close();
    if let Some(result) = poll_once(device.join()).await {
        result.unwrap();
    }
    timeout(device.join()).await.unwrap();
    assert!(counts.dropped.load(SeqCst), "cleanup precedes completion");
    assert!(input.is_closed());
    device.join().await.unwrap();
}

async fn exercise_device_results() {
    let config = config();
    let (samples, _receiver) = async_channel::bounded(1);
    let mut device = DeviceTask::attach(config, samples, |_, _| async {
        Err("device setup failed".into())
    });
    assert_eq!(
        timeout(device.join()).await.unwrap_err(),
        "device setup failed"
    );
    // Observing failure must still allow an ordinary shutdown request.
    device.request_close();
    assert_eq!(
        timeout(device.join()).await.unwrap_err(),
        "device setup failed"
    );

    let (samples, receiver) = async_channel::bounded(1);
    let (input, incoming) = async_channel::bounded(1);
    let counts = Arc::new(Counts::default());
    let worker_counts = counts.clone();
    let mut device = DeviceTask::attach(config, samples, move |_, samples| async move {
        Source {
            input: incoming,
            counts: worker_counts,
            owner: std::thread::current().id(),
            local: Rc::new(()),
        }
        .run(samples)
        .await
    });
    input.send(Err("device input failed".into())).await.unwrap();
    assert_eq!(
        timeout(device.join()).await.unwrap_err(),
        "device input failed"
    );
    assert!(counts.dropped.load(SeqCst));
    drop(receiver);

    let (samples, receiver) = async_channel::bounded(1);
    let mut device = DeviceTask::attach(config, samples, |_, samples| async move {
        samples
            .send(vec![0.5, -0.5])
            .await
            .map_err(|e| e.to_string())
    });
    timeout(device.join()).await.unwrap();
    assert_eq!(receiver.recv().await.unwrap(), vec![0.5, -0.5]);
    device.request_close();
    device.join().await.unwrap();
}

async fn exercise_waiting_send() {
    for close_output in [false, true] {
        let (samples, receiver) = async_channel::bounded(1);
        samples.try_send(vec![0.25, -0.25]).unwrap();
        let (input, incoming) = async_channel::bounded(1);
        input.send(Ok(0.5)).await.unwrap();
        let counts = Arc::new(Counts::default());
        let worker_counts = counts.clone();
        let mut device = DeviceTask::attach(config(), samples, move |_, samples| async move {
            Source {
                input: incoming,
                counts: worker_counts,
                owner: std::thread::current().id(),
                local: Rc::new(()),
            }
            .run(samples)
            .await
        });
        until(|| input.is_empty()).await;
        futures_timer::Delay::new(Duration::from_millis(30)).await;
        assert_eq!(counts.reads.load(SeqCst), 1, "submission waits for space");
        if close_output {
            drop(receiver);
            timeout(device.join()).await.unwrap();
        } else {
            device.request_close();
            timeout(device.join()).await.unwrap();
            assert_eq!(receiver.recv().await.unwrap(), vec![0.25, -0.25]);
            assert!(
                receiver.is_empty(),
                "cancellation did not submit the pending buffer"
            );
        }
        assert!(counts.dropped.load(SeqCst), "cleanup precedes completion");
        assert!(input.is_closed());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_submits_buffers_and_cancels_pending_input() {
    pollster::block_on(exercise_submission_and_shutdown());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_reports_results() {
    pollster::block_on(exercise_device_results());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn device_cancels_submission_on_worker_or_output_close() {
    pollster::block_on(exercise_waiting_send());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_submits_buffers_and_cancels_pending_input() {
    exercise_submission_and_shutdown().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_reports_results() {
    exercise_device_results().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn device_cancels_submission_on_worker_or_output_close() {
    exercise_waiting_send().await;
}

fn config() -> cpal::StreamConfig {
    cpal::StreamConfig {
        channels: 2,
        sample_rate: 48_000,
        buffer_size: cpal::BufferSize::Default,
    }
}
