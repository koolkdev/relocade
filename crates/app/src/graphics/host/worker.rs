use super::render::{self, DrawOutcome};
use crate::graphics::{Surface, SurfaceSetup};
use futures_channel::oneshot;
use futures_lite::future::{or, race};
use relocade_peripherals::display::DisplayRenderer;
use relocade_renderer::Renderer;
use std::{
    future::pending,
    sync::{Arc, Mutex},
};
use winit::dpi::PhysicalSize;

pub(super) async fn run<D: DisplayRenderer>(
    setup: SurfaceSetup,
    create: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> Result<D, String>,
    requests: async_channel::Receiver<PhysicalSize<u32>>,
    notify_redraw_needed: &impl Fn(),
) -> Result<(), String> {
    let mut surface = Surface::new(setup).await?;
    let mut failure = observe_device(&surface.device);
    let mut display = create(&surface.device, &surface.queue)?;
    let mut renderer =
        Renderer::new(surface.device.clone(), surface.format()).map_err(|e| e.to_string())?;
    // A factory can queue static image uploads before the first redraw.
    surface.queue.submit([]);
    notify_redraw_needed();
    let mut updates_closed = false;
    loop {
        // Give cancellation, GPU callbacks and the worker event loop a turn
        // even when input updates or redraw requests are continuously ready.
        crate::platform::graphics::yield_to_host().await;
        let event = or(
            async {
                Event::Failure(
                    (&mut failure)
                        .await
                        .unwrap_or_else(|_| "GPU error observer closed".into()),
                )
            },
            race(
                async {
                    if updates_closed {
                        pending::<()>().await;
                    }
                    Event::Update(display.update(&surface.queue).await)
                },
                async { Event::Redraw(requests.recv().await.ok()) },
            ),
        )
        .await;
        match event {
            Event::Update(Some(changed)) => {
                surface.queue.submit([]);
                if changed {
                    notify_redraw_needed();
                }
            }
            Event::Update(None) => updates_closed = true,
            Event::Redraw(None) => return Ok(()),
            Event::Redraw(Some(size)) => {
                match render::draw(&mut surface, &mut display, &mut renderer, size)? {
                    DrawOutcome::PresentRequested | DrawOutcome::Skipped => {}
                    DrawOutcome::Retry => notify_redraw_needed(),
                }
            }
            Event::Failure(failure) => return Err(failure),
        }
    }
}

enum Event {
    Update(Option<bool>),
    Redraw(Option<PhysicalSize<u32>>),
    Failure(String),
}

fn observe_device(device: &wgpu::Device) -> oneshot::Receiver<String> {
    let (failed, failure) = oneshot::channel();
    let failed = Arc::new(Mutex::new(Some(failed)));
    let errors = failed.clone();
    device.on_uncaptured_error(Arc::new(move |error| {
        if let Some(failed) = errors.lock().unwrap().take() {
            let _ = failed.send(format!("GPU: {error}"));
        }
    }));
    device.set_device_lost_callback(move |reason, message| {
        if reason != wgpu::DeviceLostReason::Destroyed {
            if let Some(failed) = failed.lock().unwrap().take() {
                let _ = failed.send(format!("GPU device lost: {message}"));
            }
        }
    });
    failure
}
