//! One card's raster, capture queue and display activity lifetime.
mod retrace;
pub(crate) mod status;
mod task;
pub use retrace::RetraceEvent;
pub(crate) use task::run;

use crate::snapshot::Blanking;
#[cfg(feature = "renderer")]
use crate::snapshot::MemoryChanges;
use crate::{memory::VideoMemory, raster::Raster, DacSnapshot, DisplaySnapshot, Scanout, Timing};
use async_channel::{Receiver, Sender};
use event_listener::Event;
use relocade_machine::{Clock, Timestamp};

pub(crate) struct Output {
    pub clock: Clock,
    pub raster: Raster,
    /// Display programming prepared by the card before the next capture.
    pub scanout: Scanout,
    pub blanked: bool,
    pub blanking: Blanking,
    pub configuration_changed: Event,
    pub(crate) sender: Sender<DisplaySnapshot>,
    pub frames: Receiver<DisplaySnapshot>,
    /// The producer's baseline for the next capture, without upload hints.
    pub(crate) captured: Option<DisplaySnapshot>,
    pub(crate) pending: Option<PendingSnapshot>,
    pub(crate) ready: Event,
}

pub(crate) struct PendingSnapshot {
    pub(crate) snapshot: DisplaySnapshot,
    pub(crate) time: Timestamp,
}

impl Output {
    pub fn new(clock: Clock, timing: Timing, scanout: Scanout) -> Self {
        let (sender, frames) = async_channel::bounded(1);
        Self {
            raster: Raster::new(clock.clone(), timing),
            scanout,
            blanked: false,
            blanking: Default::default(),
            clock,
            configuration_changed: Event::new(),
            sender,
            frames,
            captured: None,
            pending: None,
            ready: Event::new(),
        }
    }

    /// A due capture needs the card's latch and mode policy before sampling.
    pub fn capture_due(&self) -> Option<Timestamp> {
        let deadline = self.raster.next_retrace_at()?;
        let now = self.clock.now();
        (now >= deadline).then_some(now)
    }

    pub fn snapshot(
        &self,
        scanout: Scanout,
        dac: DacSnapshot,
        memory: &VideoMemory,
    ) -> DisplaySnapshot {
        let previous = self.captured.as_ref();
        DisplaySnapshot {
            memory: memory.snapshot(
                scanout.memory_bytes(),
                previous.map(|snapshot| &snapshot.memory),
            ),
            dac,
            blanked: self.blanked,
            blanking: self.blanking.clone(),
            scanout,
            #[cfg(feature = "renderer")]
            changes: previous.map(|previous| MemoryChanges {
                baseline: previous.memory.clone(),
                pages: memory.dirty_pages(),
            }),
        }
    }

    pub fn capture(&mut self, display: DisplaySnapshot, memory: &mut VideoMemory, time: Timestamp) {
        if self
            .captured
            .as_ref()
            .is_some_and(|previous| display.same_inputs(previous))
        {
            return;
        }
        self.captured = Some(DisplaySnapshot {
            // Only outgoing snapshots retain their upload baseline and dirty hints.
            #[cfg(feature = "renderer")]
            changes: None,
            ..display.clone()
        });
        memory.clear_dirty();
        // Coalesced captures retain every dirty interval since the task took one.
        #[cfg(feature = "renderer")]
        let display = {
            let mut display = display;
            if let Some(pending) = &self.pending {
                display.merge_changes(&pending.snapshot);
            }
            display
        };
        let pending = PendingSnapshot {
            snapshot: display,
            time,
        };
        if self.pending.replace(pending).is_none() {
            self.ready.notify(1);
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.configuration_changed.notify(usize::MAX);
    }
}
