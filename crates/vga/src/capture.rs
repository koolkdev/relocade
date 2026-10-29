use crate::{scanout, TextScreen, VideoFrame};
use arc_swap::ArcSwapOption;
use event_listener::Event;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Immutable rasterizer input sampled at a completed text refresh boundary.
/// It preserves coherent text state, not pixel history throughout the refresh.
pub struct CapturedFrame {
    pub(crate) screen: TextScreen,
    pub(crate) blink_on: bool,
    pub(crate) blanked: bool,
    pub time: Duration,
    pub sequence: u64,
}

impl CapturedFrame {
    /// Performs synchronous pixel conversion on the calling worker. No live VGA
    /// state is accessed. The execution worker only creates these small captures;
    /// call this on the graphics worker when a redraw is requested.
    pub fn rasterize(&self) -> VideoFrame {
        VideoFrame {
            width: scanout::WIDTH,
            height: scanout::HEIGHT,
            pixels: scanout::render(&self.screen, self.blink_on, self.blanked),
            // Represent scan doubling by pixel aspect instead of duplicate rows.
            pixel_aspect_ratio: (5, 12),
            time: self.time,
            sequence: self.sequence,
        }
    }
}

#[derive(Default)]
struct Captures {
    frame: ArcSwapOption<CapturedFrame>,
    changed: Event,
    closed: AtomicBool,
}

/// Read-only input for VGA rasterization. Only one capture is retained; a slow
/// graphics worker can skip obsolete inputs without blocking guest execution.
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

    pub fn current_capture(&self) -> Option<Arc<CapturedFrame>> {
        self.captures.frame.load_full()
    }

    /// Waits for a newer captured input, not for pixels or app presentation.
    /// A rendering consumer must wait for redraw demand separately.
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
