//! One graphics worker drives rendering and window presentation independently.
mod output;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
use crate::{platform::RenderTarget, Exit, Graphics, Renderer, Task, Texture};
use futures_channel::mpsc;
use futures_util::{future::poll_fn, StreamExt};
use output::Output;
use relocade_renderer::Viewer;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub(crate) enum Command<D> {
    Source(D),
    Present([u32; 2]),
    Resume,
    Suspend,
    Failure(String),
}

pub(crate) struct Display<D> {
    task: Task<()>,
    commands: mpsc::UnboundedSender<Command<D>>,
}

impl<D: Send + 'static> Display<D> {
    pub fn new<R: Renderer<Source = D>>(
        target: RenderTarget,
        frame_done: impl Fn() + Send + 'static,
    ) -> Self {
        let (commands, incoming) = mpsc::unbounded();
        let failures = commands.clone();
        let task = Task::spawn(target, move |target, _stop| async move {
            run::<R>(target, incoming, failures, frame_done)
                .await
                .map(Exit::Completed)
        });
        Self { task, commands }
    }

    pub fn send(&self, command: Command<D>) {
        // Completion is observed through the task, including a disconnected queue.
        let _ = self.commands.unbounded_send(command);
    }

    pub fn stop(&self) {
        self.task.stop();
    }
}

impl<D> Future for Display<D> {
    type Output = Result<(), String>;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.task)
            .poll(context)
            .map(|result| result.map(|_| ()))
    }
}

async fn run<R: Renderer>(
    target: RenderTarget,
    mut incoming: mpsc::UnboundedReceiver<Command<R::Source>>,
    failures: mpsc::UnboundedSender<Command<R::Source>>,
    frame_done: impl Fn(),
) -> Result<(), String> {
    let instance = target.instance();
    let surface = target.surface(&instance).await?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .map_err(|error| format!("Cannot select a graphics adapter: {error}"))?;
    let (device, queue) = adapter
        .request_device(&Default::default())
        .await
        .map_err(|error| format!("Cannot create the graphics device: {error}"))?;
    let errors = failures.clone();
    device.on_uncaptured_error(Arc::new(move |error| {
        let _ = errors.unbounded_send(Command::Failure(format!("Graphics error: {error}")));
    }));
    device.set_device_lost_callback(move |reason, message| {
        if reason != wgpu::DeviceLostReason::Destroyed {
            let _ = failures
                .unbounded_send(Command::Failure(format!("Graphics device lost: {message}")));
        }
    });
    let graphics = Graphics { device, queue };
    let mut output = Output::new(target, instance, adapter, surface)?;
    let mut viewer = Viewer::new(
        graphics.device.clone(),
        graphics.queue.clone(),
        output.format(),
    )
    .map_err(|error| error.to_string())?;
    let mut renderer = RendererState::<R>::Empty;
    while let Some(command) = await_host(&mut renderer, incoming.next()).await? {
        match command {
            Command::Source(source) => {
                renderer = RendererState::Active(Box::pin(R::new(source, graphics.clone())?));
            }
            Command::Present(size) => {
                if renderer.texture().is_some() {
                    if let Some(frame) =
                        await_host(&mut renderer, output.acquire(&graphics.device, size)).await??
                    {
                        // Surface acquisition can yield while the renderer replaces its
                        // texture. Borrow the current texture after acquisition.
                        if let Some(texture) = renderer.texture() {
                            viewer.draw(texture, &frame.texture.create_view(&Default::default()));
                            output.present(&graphics.queue, frame);
                        }
                    }
                }
                // Acknowledge consumed requests even before the first texture or
                // after transient acquisition failure. This is not a GPU fence.
                frame_done();
            }
            Command::Resume => {
                await_host(&mut renderer, output.resume()).await??;
            }
            Command::Suspend => output.suspend(),
            Command::Failure(error) => return Err(error),
        }
    }
    Ok(())
}

enum RendererState<R> {
    Empty,
    Active(Pin<Box<R>>),
    Complete(Option<Texture>),
}

impl<R: Renderer> RendererState<R> {
    fn texture(&self) -> Option<&Texture> {
        match self {
            Self::Empty => None,
            Self::Active(renderer) => renderer.as_ref().get_ref().texture(),
            Self::Complete(texture) => texture.as_ref(),
        }
    }
}

/// Rendering remains live through every host await, including surface handoff.
async fn await_host<R: Renderer, T>(
    renderer: &mut RendererState<R>,
    host: impl Future<Output = T>,
) -> Result<T, String> {
    futures_util::pin_mut!(host);
    poll_fn(|context| {
        if let RendererState::Active(active) = renderer {
            if let Poll::Ready(result) = active.as_mut().poll(context) {
                let texture = if result.is_ok() {
                    active.as_ref().get_ref().texture().cloned()
                } else {
                    None
                };
                // Keep the final display while releasing the renderer's channels,
                // timers and other work. Completed futures must not be polled again.
                *renderer = RendererState::Complete(texture);
                result?;
            }
        }
        host.as_mut().poll(context).map(Ok)
    })
    .await
}
