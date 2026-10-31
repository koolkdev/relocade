use super::Event;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Wake,
};
use winit::event_loop::EventLoopProxy;

/// Wakes main-thread futures through winit instead of spinning or blocking it.
pub(super) struct EventWake {
    pub(super) events: EventLoopProxy<Event>,
    queued: AtomicBool,
}

impl EventWake {
    pub(super) fn new(events: EventLoopProxy<Event>) -> Self {
        Self {
            events,
            queued: AtomicBool::new(false),
        }
    }

    pub(super) fn notify(&self) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.events.send_event(Event::Wake);
        }
    }

    pub(super) fn clear(&self) {
        self.queued.store(false, Ordering::Release);
    }
}

impl Wake for EventWake {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}
