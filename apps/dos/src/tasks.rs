use super::application::Event;
use futures_util::{stream::FuturesUnordered, Stream};
use relocade_app::winit::event_loop::EventLoopProxy;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

struct WakeMain {
    events: EventLoopProxy<Event>,
    queued: AtomicBool,
}

impl Wake for WakeMain {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.events.send_event(Event::Poll);
        }
    }
}

/// Poll host timers and worker completion on main, using winit for wake delivery.
/// These tasks do not execute the guest or determine redraw opportunities.
pub(super) struct Tasks {
    pending: FuturesUnordered<Pin<Box<dyn Future<Output = Event>>>>,
    wake: Arc<WakeMain>,
}

impl Tasks {
    pub fn new(events: EventLoopProxy<Event>) -> Self {
        Self {
            pending: FuturesUnordered::new(),
            wake: Arc::new(WakeMain {
                events,
                queued: AtomicBool::new(false),
            }),
        }
    }

    pub fn push(&self, task: impl Future<Output = Event> + 'static) {
        self.pending.push(Box::pin(task));
    }

    pub fn poll(&mut self) -> Option<Event> {
        let waker = Waker::from(self.wake.clone());
        match Pin::new(&mut self.pending).poll_next(&mut Context::from_waker(&waker)) {
            Poll::Ready(event) => event,
            Poll::Pending => None,
        }
    }

    pub fn wake_delivered(&self) {
        self.wake.queued.store(false, Ordering::Release);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
