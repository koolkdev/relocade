use std::time::Duration;
use web_time::Instant;
use winit::{event_loop::ControlFlow, window::Window};

const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Paces host redraws and permits one outstanding presentation request.
pub(super) struct FrameScheduler {
    pending: bool,
    next: Instant,
}

impl FrameScheduler {
    pub fn new() -> Self {
        Self {
            pending: false,
            next: Instant::now(),
        }
    }

    pub fn submit(&mut self) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        self.next = Instant::now() + FRAME_INTERVAL;
        true
    }

    pub fn complete(&mut self) {
        self.pending = false;
    }

    pub fn invalidate(&mut self) {
        self.next = Instant::now();
    }

    pub fn schedule(&self, window: &Window) -> ControlFlow {
        if self.pending {
            ControlFlow::Wait
        } else if Instant::now() >= self.next {
            window.request_redraw();
            ControlFlow::Wait
        } else {
            ControlFlow::WaitUntil(self.next)
        }
    }
}
