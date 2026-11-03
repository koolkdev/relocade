use crate::worker::{SendTask, Thread};
use async_channel::Sender;
use futures_lite::future::or;
use std::future::Future;

pub(super) struct DeviceTask {
    stop: Sender<()>,
    task: SendTask<Result<(), String>>,
    result: Option<Result<(), String>>,
    thread: Option<Thread>,
}

impl DeviceTask {
    pub(super) fn attach<F: Future<Output = Result<(), String>> + 'static>(
        config: cpal::StreamConfig,
        samples: Sender<Vec<f32>>,
        attach: impl FnOnce(cpal::StreamConfig, Sender<Vec<f32>>) -> F + Send + 'static,
    ) -> Self {
        let (stop, stopping) = async_channel::bounded(1);
        let thread = Thread::new();
        let task = thread.run_send((), move |()| async move {
            or(
                async {
                    let _ = stopping.recv().await;
                    Ok(())
                },
                async { attach(config, samples).await },
            )
            .await
        });
        Self {
            stop,
            task,
            result: None,
            thread: Some(thread),
        }
    }

    pub(super) async fn join(&mut self) -> Result<(), String> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        let result = (&mut self.task)
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        self.result = Some(result.clone());
        self.thread.take();
        result
    }

    pub(super) fn request_close(&mut self) {
        self.stop.close();
    }
}

impl Drop for DeviceTask {
    fn drop(&mut self) {
        self.request_close();
    }
}

#[cfg(test)]
mod tests;
