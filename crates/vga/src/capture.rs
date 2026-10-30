use crate::DisplaySnapshot;
use arc_swap::ArcSwapOption;
use event_listener::Event;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Immutable rasterizer input sampled at a completed refresh boundary.
/// It preserves coherent display state, not pixel history throughout the refresh.
/// Renderers can read this state directly without allocating a CPU pixel image.
pub struct CapturedFrame {
    /// Complete physical memory, palette and programmable display settings.
    pub display: DisplaySnapshot,
    pub time: Duration,
    pub sequence: u64,
}

#[derive(Default)]
struct Captures {
    frame: ArcSwapOption<CapturedFrame>,
    changed: Event,
    closed: AtomicBool,
}

/// Read-only captured VGA state. Only the latest capture is retained; slow
/// consumers skip obsolete inputs without blocking guest execution.
#[derive(Clone)]
pub struct FrameSource {
    captures: Arc<Captures>,
}

impl FrameSource {
    pub(crate) fn new() -> Self {
        Self {
            captures: Arc::new(Captures::default()),
        }
    }

    /// Latest completed capture, including after VGA is destroyed. Consumers
    /// may retain it and render it again independently of the source lifetime.
    pub fn current_capture(&self) -> Option<Arc<CapturedFrame>> {
        self.captures.frame.load_full()
    }

    /// Waits for a newer captured input, not for pixels or app presentation.
    /// Returns `None` when VGA is destroyed and no newer capture is available.
    /// Captures already returned remain usable. This does not schedule rendering.
    pub async fn next_capture(&self, after: Option<u64>) -> Option<Arc<CapturedFrame>> {
        loop {
            let changed = self.captures.changed.listen();
            let closed = self.captures.closed.load(Ordering::Acquire);
            if let Some(frame) = self.current_capture() {
                if after.is_none_or(|sequence| frame.sequence > sequence) {
                    return Some(frame);
                }
            }
            if closed {
                return None;
            }
            changed.await;
        }
    }

    pub(crate) fn capture(&self, frame: CapturedFrame) {
        self.captures.frame.store(Some(Arc::new(frame)));
        self.captures.changed.notify(usize::MAX);
    }

    pub(crate) fn close(&self) {
        self.captures.closed.store(true, Ordering::Release);
        self.captures.changed.notify(usize::MAX);
    }
}
