#[cfg(feature = "renderer")]
use crate::snapshot::MemoryChanges;
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

/// Immutable rasterizer input sampled at the start of vertical retrace, after
/// latching the start address for the following active display. It preserves
/// coherent display state, not pixel history throughout the preceding refresh.
/// Renderers can read this state directly without allocating a CPU pixel image.
#[derive(Clone)]
pub struct CapturedFrame {
    /// Complete physical memory, palette and programmable display settings.
    pub display: DisplaySnapshot,
    /// Sampling/latch boundary on the device's clock, not presentation time.
    pub time: Duration,
    /// Hardware retrace count at capture. Unchanged refreshes are not published,
    /// so successive captures can have gaps in their sequence numbers.
    pub sequence: u64,
}

#[derive(Default)]
struct Captures {
    frame: ArcSwapOption<CapturedFrame>,
    changed: Event,
    closed: AtomicBool,
}

/// Captured VGA state. Only the latest capture is retained; slow
/// consumers skip obsolete inputs without blocking guest execution.
/// A capture is published when memory, registers or blink phase change. This is
/// a conservative input comparison: hidden memory and register access state can
/// also count as changes. Hardware timing continues through unchanged refreshes.
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

    /// Latest published capture. Unchanged refreshes keep it and its timestamp
    /// intact, including after VGA is destroyed. Consumers may retain it and
    /// render it again independently of the source lifetime.
    pub fn current_capture(&self) -> Option<Arc<CapturedFrame>> {
        self.captures.frame.load_full()
    }

    /// Takes the latest capture and its accumulated dirty pages for upload.
    /// The capture remains readable; only the pending upload hints are consumed.
    /// `current_capture` and `next_capture` never consume these hints.
    ///
    /// Use one consuming renderer per source. Upload every taken capture before
    /// taking another; skipped publications are already included in its dirty map.
    /// A new GPU still needs a full upload. Independent readers can render retained
    /// captures using the renderer's full-plane fallback when baselines differ.
    #[cfg(feature = "renderer")]
    pub fn take_capture(&self) -> Option<Arc<CapturedFrame>> {
        // Swap the snapshot and hints together. A concurrent publication retries
        // against the new baseline, so its later changes cannot be cleared here.
        self.captures.frame.rcu(|current| {
            current.as_ref().map(|current| {
                let mut retained = (**current).clone();
                retained.display.changes = Some(MemoryChanges {
                    baseline: retained.display.planes.clone(),
                    pages: Default::default(),
                });
                Arc::new(retained)
            })
        })
    }

    /// Waits for changed captured inputs after the supplied sequence, not for
    /// every hardware refresh, pixels or app presentation.
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

    pub(crate) fn publish(&self, frame: CapturedFrame) {
        #[cfg(feature = "renderer")]
        self.captures.frame.rcu(|previous| {
            let mut frame = frame.clone();
            if let Some(pending) = previous
                .as_ref()
                .and_then(|frame| frame.display.changes.as_ref())
            {
                let changes = frame
                    .display
                    .changes
                    .as_mut()
                    .expect("later captures have a memory baseline");
                changes.baseline.clone_from(&pending.baseline);
                changes.pages.merge(pending.pages);
            }
            Some(Arc::new(frame))
        });
        #[cfg(not(feature = "renderer"))]
        self.captures.frame.store(Some(Arc::new(frame)));
        self.captures.changed.notify(usize::MAX);
    }

    pub(crate) fn close(&self) {
        self.captures.closed.store(true, Ordering::Release);
        self.captures.changed.notify(usize::MAX);
    }
}
