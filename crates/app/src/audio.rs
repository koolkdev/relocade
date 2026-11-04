//! CPAL playback and an application-owned audio device worker.

use async_channel::Sender;
use std::future::Future;

pub use cpal;

mod output;
mod task;
use output::Output;
use task::DeviceTask;

/// Owns platform output and its device task. Create and play this on main.
/// Await `close()` before dropping an active owner so device cleanup runs on
/// its worker; early drop can terminate a browser worker without destructors.
pub struct Audio {
    output: Option<Output>,
    device: DeviceTask,
}

impl Audio {
    /// Opens default CPAL output and attaches a device on its own worker.
    /// The callback receives the output configuration and an ordinary bounded
    /// sender of interleaved f32 buffers. It owns synthesis and sample submission.
    /// The channel holds 16 buffers; the device chooses and bounds their sizes.
    ///
    /// The closure crosses threads; its future and device state stay on the
    /// worker and need not be `Send`. Yield during ongoing work so `close()` can
    /// cancel it. Keep `on_error` brief: CPAL may call it from an audio thread.
    pub fn attach_device<F: Future<Output = Result<(), String>> + 'static>(
        attach: impl FnOnce(cpal::StreamConfig, Sender<Vec<f32>>) -> F + Send + 'static,
        on_error: impl FnMut(cpal::Error) + Send + 'static,
    ) -> Result<Self, cpal::Error> {
        let (output, config, samples) = Output::open(on_error)?;
        Ok(Self {
            output: Some(output),
            device: DeviceTask::attach(config, samples, attach),
        })
    }

    /// Starts consuming queued samples; missing samples produce silence.
    /// In browsers this requests playback without awaiting processor readiness
    /// or the resume promise. Call from the application's user activation.
    pub fn play(&self) -> Result<(), cpal::Error> {
        self.output
            .as_ref()
            .ok_or_else(|| cpal::Error::new(cpal::ErrorKind::StreamInvalidated))?
            .play()
    }

    /// Cancels the device task, awaits worker cleanup, then drops platform output
    /// and discards queued samples, even when the device returns an error.
    /// Cancelling this wait retains the shutdown request; call it again to finish.
    /// Browser context teardown remains asynchronous and is owned by CPAL.
    pub async fn close(&mut self) -> Result<(), String> {
        self.device.request_close();
        let result = self.device.join().await;
        self.output.take();
        result
    }

    /// Waits for device completion without requesting shutdown. Output stays
    /// alive to consume queued samples; completion does not mean they have played.
    /// This wait can be cancelled and resumed. Repeated `join()` or `close()`
    /// calls return the same result after device cleanup on its worker.
    pub async fn join(&mut self) -> Result<(), String> {
        self.device.join().await
    }
}
