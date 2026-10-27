use crate::{Display, TextRenderer};
use futures_timer::Delay;
use relocade_renderer::{Graphics, Renderer, Texture};
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
    time::Duration,
};
use web_time::Instant;

const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Updates the VGA display texture independently of host redraws and visibility.
pub struct VgaRenderer {
    display: Display,
    queue: wgpu::Queue,
    text: TextRenderer,
    texture: Texture,
    started: Instant,
    timer: Delay,
}

impl Renderer for VgaRenderer {
    type Source = Display;

    fn new(display: Display, graphics: Graphics) -> Result<Self, String> {
        let texture = Texture::new(
            &graphics.device,
            [TextRenderer::WIDTH, TextRenderer::HEIGHT],
        )
        .map_err(|error| error.to_string())?;
        let mut renderer = Self {
            display,
            queue: graphics.queue,
            text: TextRenderer::new(),
            texture,
            started: Instant::now(),
            timer: Delay::new(FRAME_INTERVAL),
        };
        renderer.refresh()?;
        Ok(renderer)
    }

    fn texture(&self) -> Option<&Texture> {
        Some(&self.texture)
    }
}

impl VgaRenderer {
    fn refresh(&mut self) -> Result<(), String> {
        let screen = self.display.screen();
        self.texture
            .write(
                &self.queue,
                self.text.render(&screen, self.started.elapsed()),
            )
            .map_err(|error| error.to_string())
    }
}

impl Future for VgaRenderer {
    type Output = Result<(), String>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        ready!(Pin::new(&mut this.timer).poll(context));
        this.refresh()?;
        // This temporary text-display cadence also advances blinking while
        // execution waits for input. Emulated scanout timing belongs to VGA.
        this.timer.reset(FRAME_INTERVAL);
        // Reset replaces the browser future, so register this worker's waker.
        // If the timer already fired, schedule another bounded poll.
        if Pin::new(&mut this.timer).poll(context).is_ready() {
            context.waker().wake_by_ref();
        }
        Poll::Pending
    }
}
