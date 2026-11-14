//! One card's raster, capture queue and display activity lifetime.
mod card;
mod retrace;
pub(crate) mod status;
mod task;
pub(crate) use card::CardOutput;
pub use retrace::RetraceEvent;
pub(crate) use task::run;

#[cfg(feature = "renderer")]
use crate::snapshot::MemoryChanges;
use crate::{
    memory::VideoMemory, raster::Raster, scanout::DisplayConfiguration, DacSnapshot,
    DisplaySnapshot, Framebuffer, Registers, Timing,
};
use async_channel::{Receiver, Sender};
use event_listener::Event;
use relocade_machine::{Clock, Timestamp};

pub(crate) struct Output {
    pub clock: Clock,
    pub raster: Raster,
    /// Display programming prepared by the card before the next capture.
    pub display: DisplayConfiguration,
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
    pub fn new(clock: Clock, timing: Timing, display: DisplayConfiguration) -> Self {
        let (sender, frames) = async_channel::bounded(1);
        Self {
            raster: Raster::new(clock.clone(), timing),
            display,
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
        registers: &Registers,
        memory: &VideoMemory,
        start: u32,
    ) -> DisplaySnapshot {
        let scanout = self
            .display
            .scanout_at(registers, start, self.raster.retrace_count);
        let previous = self.captured.as_ref();
        DisplaySnapshot {
            memory: memory.snapshot(
                scanout.memory_bytes(),
                previous.map(|snapshot| &snapshot.memory),
            ),
            dac: DacSnapshot {
                colors: registers.dac.colors,
                mask: registers.dac.mask,
            },
            blanked: self.display.blanked,
            blanking: self.display.blanking.clone(),
            scanout,
            #[cfg(feature = "renderer")]
            changes: previous.map(|previous| MemoryChanges {
                baseline: previous.memory.clone(),
                pages: memory.dirty_pages(),
            }),
        }
    }

    /// Native drawing resolves the programmed start against the same prepared
    /// configuration as capture. The caller services overdue output first.
    pub fn framebuffer<'memory>(
        &self,
        registers: &Registers,
        memory: &'memory mut VideoMemory,
        start: u32,
    ) -> Framebuffer<'memory> {
        Framebuffer {
            scanout: self
                .display
                .scanout_at(registers, start, self.raster.retrace_count),
            memory,
        }
    }

    pub fn capture(
        &mut self,
        registers: &Registers,
        memory: &mut VideoMemory,
        start: u32,
        time: Timestamp,
    ) {
        let display = self.snapshot(registers, memory, start);
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
