use crate::DisplaySnapshot;
use arc_swap::ArcSwapOption;
use event_listener::Event;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// Immutable rasterizer input sampled at the start of vertical retrace, after
/// latching the start address for the following active display. It preserves
/// coherent display state, not pixel history throughout the preceding refresh.
/// Renderers can read this state directly without allocating a CPU pixel image.
pub struct CapturedFrame {
    /// Complete physical memory, palette and programmable display settings.
    pub display: DisplaySnapshot,
    /// Sampling/latch boundary on the device's clock, not presentation time.
    pub time: Duration,
    pub sequence: u64,
}

#[derive(Default)]
struct Captures {
    frame: ArcSwapOption<CapturedFrame>,
    captured: Event,
    inputs_changed: Event,
    last_change: AtomicU64,
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
        self.wait_for_capture(after, false).await
    }

    /// Waits for changed memory, registers or blink phase, ignoring captures that
    /// only advance time and sequence. Returns the latest capture after a change,
    /// so slow consumers still skip intermediate frames. `after` is a previously
    /// observed capture sequence, as in `next_capture`.
    ///
    /// This conservatively compares captured inputs, not visible pixels. Hidden
    /// memory and register access state can also count as changes. Unchanged
    /// captures do not wake this waiter. Source closure follows `next_capture`.
    pub async fn next_changed_capture(&self, after: Option<u64>) -> Option<Arc<CapturedFrame>> {
        self.wait_for_capture(after, true).await
    }

    async fn wait_for_capture(
        &self,
        after: Option<u64>,
        changes_only: bool,
    ) -> Option<Arc<CapturedFrame>> {
        let event = if changes_only {
            &self.captures.inputs_changed
        } else {
            &self.captures.captured
        };
        loop {
            let changed = event.listen();
            let closed = self.captures.closed.load(Ordering::Acquire);
            // Publication stores the frame before this sequence. Acquire the
            // sequence first so a reported change cannot return an older frame.
            let last_change = self.captures.last_change.load(Ordering::Acquire);
            if let Some(frame) = self.current_capture() {
                let sequence = if changes_only {
                    last_change
                } else {
                    frame.sequence
                };
                if after.is_none_or(|after| sequence > after) {
                    return Some(frame);
                }
            }
            if closed {
                return None;
            }
            changed.await;
        }
    }

    pub(crate) fn publish(&self, frame: CapturedFrame) {
        let inputs_changed = self
            .captures
            .frame
            .load()
            .as_ref()
            .is_none_or(|previous| !frame.display.same_inputs(&previous.display));
        let sequence = frame.sequence;
        self.captures.frame.store(Some(Arc::new(frame)));
        if inputs_changed {
            self.captures.last_change.store(sequence, Ordering::Release);
            self.captures.inputs_changed.notify(usize::MAX);
        }
        self.captures.captured.notify(usize::MAX);
    }

    pub(crate) fn close(&self) {
        self.captures.closed.store(true, Ordering::Release);
        self.captures.captured.notify(usize::MAX);
        self.captures.inputs_changed.notify(usize::MAX);
    }
}
