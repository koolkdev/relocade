//! Shared capture queue state, independent of a card's mode selection.
use crate::DisplaySnapshot;
use async_channel::{Receiver, Sender};
use event_listener::Event;
use relocade_machine::Timestamp;

pub(crate) struct Output {
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
