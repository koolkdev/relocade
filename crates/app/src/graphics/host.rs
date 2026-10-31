mod render;
mod worker;

use super::SurfaceSetup;
use crate::worker::Thread;
use futures_channel::oneshot;
use futures_lite::future::or;
use relocade_frontend::display::DisplayRenderer;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use winit::dpi::PhysicalSize;

/// Main-thread connection to a display hosted on one graphics worker.
/// The worker owns the display, surface, GPU resources and submissions.
///
/// Closing or dropping the connection stops graphics. Await the completion future
/// returned by `spawn` before releasing the window. Dropping that future early can
/// terminate a browser worker before Rust destructors run.
pub struct Graphics {
    redraws: async_channel::Sender<PhysicalSize<u32>>,
    notified: Arc<AtomicBool>,
    stop: oneshot::Sender<()>,
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
    /// Poll the completion future alongside the application to observe startup
    /// and runtime failures. It retains the worker through owner-local cleanup.
    pub fn spawn<D: DisplayRenderer + 'static>(
        setup: SurfaceSetup,
        create: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> Result<D, String> + Send + 'static,
        notify_redraw_needed: impl Fn() + Send + Sync + 'static,
    ) -> (Self, impl Future<Output = Result<(), String>>) {
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
        (
            Self {
                redraws,
                notified,
                stop,
            },
            async move {
                let result = task
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|result| result);
                drop(thread);
                result
            },
        )
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

    /// Requests shutdown. Await the completion future to finish releasing resources.
    pub fn close(self) {
        let _ = self.stop.send(());
    }
}
