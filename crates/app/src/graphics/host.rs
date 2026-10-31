mod render;
mod worker;

use super::SurfaceSetup;
use crate::worker::{SendTask, Thread};
use futures_channel::oneshot;
use futures_lite::future::or;
use relocade_frontend::display::DisplayRenderer;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use winit::dpi::PhysicalSize;

/// Main-thread connection to a display hosted on one graphics worker.
/// The worker owns the display, surface, GPU resources and submissions.
///
/// Request closure and await `finished()` before releasing the window. Dropping
/// this owner early can terminate a browser worker before Rust destructors run.
pub struct Graphics {
    redraws: async_channel::Sender<PhysicalSize<u32>>,
    notified: Arc<AtomicBool>,
    stop: Option<oneshot::Sender<()>>,
    task: SendTask<Result<(), String>>,
    _thread: Thread,
}

impl Graphics {
    /// The factory crosses to graphics; its resulting display stays there and
    /// need not be `Send`. The factory runs once. The display keeps its logical
    /// state across device loss and controls its own recovery through the
    /// `DisplayRenderer` lifecycle hooks.
    /// `notify_redraw_needed` runs on graphics and asks the app to schedule
    /// a redraw opportunity. Notifications coalesce until `request_redraw`
    /// is called.
    /// Device loss replaces the host GPU on this same worker. Replacement failure
    /// or a replacement lost before presentation ends the connection with an error.
    /// Poll `finished()` alongside the application to observe startup and runtime
    /// failures. This owner retains the worker through its resource cleanup.
    pub fn spawn<D: DisplayRenderer + 'static>(
        setup: SurfaceSetup,
        create: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> Result<D, String> + Send + 'static,
        notify_redraw_needed: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (redraws, requests) = async_channel::bounded(1);
        let notified = Arc::new(AtomicBool::new(false));
        let changed = notified.clone();
        let notify = move || {
            if !changed.swap(true, Ordering::AcqRel) {
                notify_redraw_needed();
            }
        };
        let (stop, stopping) = oneshot::channel();
        let thread = Thread::new();
        let task = thread.run_send(setup, move |setup| async move {
            or(
                async {
                    let _ = stopping.await;
                    Ok(())
                },
                worker::run(setup, create, requests, &notify),
            )
            .await
        });
        Self {
            redraws,
            notified,
            stop: Some(stop),
            task,
            _thread: thread,
        }
    }

    /// Queues a redraw of the latest available image at this physical size.
    /// Repeated requests replace one pending size. Zero size suppresses drawing.
    /// Returns without waiting for drawing or GPU completion. Completing a draw
    /// does not request another one.
    pub fn request_redraw(&mut self, size: PhysicalSize<u32>) {
        self.notified.store(false, Ordering::Release);
        // Replace the pending size. Disconnection is reported by completion.
        let _ = self.redraws.force_send(size);
    }

    /// Requests shutdown without releasing the worker. Repeated requests are harmless.
    pub fn request_close(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    /// Waits for startup failure, runtime failure or requested shutdown, including
    /// resource cleanup on the graphics worker. This wait may be cancelled and
    /// resumed until it completes; await its result only once.
    pub async fn finished(&mut self) -> Result<(), String> {
        (&mut self.task).await.map_err(|error| error.to_string())?
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        self.request_close();
    }
}
