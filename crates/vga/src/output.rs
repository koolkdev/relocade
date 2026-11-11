//! Capture on the execution owner, then synchronize and publish from its task.

#[cfg(feature = "renderer")]
use crate::snapshot::MemoryChanges;
use crate::{DacSnapshot, DisplaySnapshot, Scanout, Vga, VgaDisplayState};
use async_channel::{Receiver, Sender};
use event_listener::Event;
use futures_lite::future::or;
use relocade_machine::Timestamp;
use std::{cell::RefCell, rc::Rc};

pub(super) struct Output {
    sender: Sender<DisplaySnapshot>,
    pub frames: Receiver<DisplaySnapshot>,
    captured: Option<DisplaySnapshot>,
    pending: Option<PendingSnapshot>,
    ready: Event,
}

struct PendingSnapshot {
    snapshot: DisplaySnapshot,
    time: Timestamp,
}

impl Output {
    pub fn new() -> Self {
        let (sender, frames) = async_channel::bounded(1);
        Self {
            sender,
            frames,
            captured: None,
            pending: None,
            ready: Event::new(),
        }
    }
}

impl Vga {
    /// Captures current display inputs immediately on the execution owner, without
    /// advancing time, publishing a frame, or changing CPU read/access latches.
    /// Shares immutable memory and copies the active display settings and DAC.
    /// Packed SVGA captures own their layout independently of the CPU bank.
    /// The display-start latch stays at the last serviced VGA retrace; this does not
    /// force pending CRTC start-address writes to become effective.
    /// Unlike [`Vga::frames`], this does not wait for a refresh boundary.
    pub fn snapshot(&self) -> DisplaySnapshot {
        let scanout = match self.dispi.display_layout() {
            Some(layout) => Scanout::Svga(layout),
            None => Scanout::Vga(VgaDisplayState::capture(
                &self.registers,
                self.raster.retrace_count,
            )),
        };
        DisplaySnapshot {
            memory: self.memory.snapshot(scanout.memory_bytes()),
            dac: DacSnapshot {
                colors: self.registers.dac.colors,
                mask: self.registers.dac.mask,
            },
            scanout,
            #[cfg(feature = "renderer")]
            changes: self.output.captured.as_ref().map(|previous| MemoryChanges {
                baseline: previous.memory.clone(),
                pages: self.memory.changed_pages(),
            }),
        }
    }

    /// The common write path only reads time and compares the cached boundary.
    /// Capture before mutation if that boundary elapsed before the task ran.
    pub(super) fn capture_if_due(&mut self) {
        if let Some(deadline) = self.raster.next_retrace_at() {
            let now = self.clock.now();
            if now >= deadline {
                self.advance_raster(now);
            }
        }
    }

    /// Refresh due status and capture events. Timing changes then materialize
    /// any remaining phase at this observed time before remapping the counters.
    #[inline]
    pub(super) fn advance_raster(&mut self, now: Timestamp) {
        if self.raster.status.observe(now) {
            self.refresh_raster();
        }
    }

    // Keep capture temporaries and raster arithmetic out of ordinary status reads.
    #[cold]
    #[inline(never)]
    fn refresh_raster(&mut self) {
        let Some(time) = self.raster.update_observed() else {
            return;
        };
        if self.dispi.display_layout().is_none() {
            self.registers.crtc.latch_display_start();
        }
        let display = self.snapshot();
        if self
            .output
            .captured
            .as_ref()
            .is_some_and(|previous| display.same_inputs(previous))
        {
            return;
        }
        self.output.captured = Some(display.clone());
        // Sampling starts a new dirty interval. Coalesced pending captures retain
        // every interval since the task last took a snapshot. A renderer that skips
        // a queued snapshot uses complete changed blocks when its baseline differs.
        #[cfg(feature = "renderer")]
        let display = {
            let mut display = display;
            self.memory.clear_changes();
            if let Some(pending) = &self.output.pending {
                display.merge_changes(&pending.snapshot);
            }
            display
        };
        let pending = PendingSnapshot {
            snapshot: display,
            time,
        };
        if self.output.pending.replace(pending).is_none() {
            // The task's normal waker requests an execution interrupt. Replacing
            // work already waiting for the task does not need another notification.
            self.output.ready.notify(1);
        }
    }
}

/// The task maps each taken timestamp to host time before waiting. New captures
/// replace pending work without extending a wait already in progress.
pub(super) async fn run(device: Rc<RefCell<Vga>>) {
    let (clock, sender) = {
        let device = device.borrow();
        (device.clock.clone(), device.output.sender.clone())
    };
    loop {
        let pending = {
            let mut device = device.borrow_mut();
            device.capture_if_due();
            device.output.pending.take()
        };
        if let Some(pending) = pending {
            clock.sync_to_host(pending.time).await;
            if sender.force_send(pending.snapshot).is_err() {
                return;
            }
        } else {
            // All producers are on this owner. No await separates the pending
            // check from installing both wake paths, and no borrow spans a wait.
            let wait = {
                let device = device.borrow();
                let captured = device.output.ready.listen();
                let retrace = device.retrace_wait(device.raster.next_retrace_at());
                async move {
                    or(captured, async {
                        retrace.await;
                    })
                    .await;
                }
            };
            wait.await;
        }
    }
}
