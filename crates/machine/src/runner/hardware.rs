//! Input delivery and replaceable device deadlines on the execution worker.

use crate::{Clock, Sleep, Timestamp};
use futures_util::future::{maybe_done, MaybeDone};
use futures_util::{stream::LocalBoxStream, Stream, StreamExt};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

/// A device's single pending deadline. Dropping the handle cancels it.
pub struct Alarm {
    clock: Clock,
    hardware: Weak<Hardware>,
    entry: Rc<Entry>,
}

struct Entry {
    armed: RefCell<Option<(Timestamp, MaybeDone<Sleep>)>>,
    fire: RefCell<Box<dyn FnMut()>>,
}

impl Alarm {
    /// Reprograms or disarms the timer. The clock sees the change immediately,
    /// even inside guest I/O. The callback runs later on a stopped worker turn.
    pub fn replace(&self, deadline: Option<Timestamp>) {
        let mut armed = self.entry.armed.borrow_mut();
        if armed.as_ref().map(|(at, _)| *at) == deadline {
            return;
        }
        armed.take();
        if let (Some(at), Some(hardware)) = (deadline, self.hardware.upgrade()) {
            let mut sleep = if at <= self.clock.now() {
                MaybeDone::Done(())
            } else {
                maybe_done(self.clock.sleep_until(at))
            };
            let wake = Waker::from(hardware.alarms_ready.clone());
            let due = Pin::new(&mut sleep)
                .poll(&mut Context::from_waker(&wake))
                .is_ready();
            *armed = Some((at, sleep));
            if due {
                wake.wake_by_ref();
            }
        }
    }
}

pub(super) struct Hardware {
    alarms_ready: Arc<SourceWake>,
    alarms: RefCell<Vec<Weak<Entry>>>,
    input_ready: Arc<SourceWake>,
    inputs: RefCell<Vec<LocalBoxStream<'static, ()>>>,
}

// Sources can wake on another worker. All device callbacks run on this one.
struct SourceWake {
    worker: Waker,
    pending: AtomicBool,
}

impl SourceWake {
    fn new(worker: Waker) -> Arc<Self> {
        Arc::new(Self {
            worker,
            pending: AtomicBool::new(false),
        })
    }

    fn take_notification(self: &Arc<Self>) -> Option<Waker> {
        self.pending
            .swap(false, Ordering::Acquire)
            .then(|| Waker::from(self.clone()))
    }
}

impl Wake for SourceWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.pending.store(true, Ordering::Release);
        self.worker.wake_by_ref();
    }
}

impl Hardware {
    pub(super) fn new(wake: Waker) -> Rc<Self> {
        Rc::new(Self {
            alarms_ready: SourceWake::new(wake.clone()),
            alarms: RefCell::default(),
            input_ready: SourceWake::new(wake),
            inputs: RefCell::default(),
        })
    }

    pub(super) fn alarm(self: &Rc<Self>, clock: Clock, fire: impl FnMut() + 'static) -> Alarm {
        let entry = Rc::new(Entry {
            armed: RefCell::default(),
            fire: RefCell::new(Box::new(fire)),
        });
        let mut entries = self.alarms.borrow_mut();
        entries.retain(|entry| entry.strong_count() != 0);
        entries.push(Rc::downgrade(&entry));
        Alarm {
            clock,
            hardware: Rc::downgrade(self),
            entry,
        }
    }

    pub(super) fn on_input<T>(
        &self,
        source: impl Stream<Item = T> + 'static,
        receive: impl FnMut(T) + 'static,
    ) {
        self.inputs
            .borrow_mut()
            .push(source.map(receive).boxed_local());
        self.input_ready.wake_by_ref();
    }

    pub(super) fn deliver_input(&self) {
        let Some(wake) = self.input_ready.take_notification() else {
            return;
        };
        let mut cx = Context::from_waker(&wake);
        // One event per source gives keyboard, mouse and CPU each a turn
        // while a producer has buffered input. A callback can register more sources.
        let mut inputs = self.inputs.take();
        inputs.retain_mut(|input| match input.poll_next_unpin(&mut cx) {
            Poll::Ready(Some(())) => {
                wake.wake_by_ref();
                true
            }
            Poll::Pending => true,
            Poll::Ready(None) => false,
        });
        inputs.append(&mut self.inputs.borrow_mut());
        self.inputs.replace(inputs);
    }

    pub(super) fn fire_alarms(&self) {
        let Some(wake) = self.alarms_ready.take_notification() else {
            return;
        };
        let mut cx = Context::from_waker(&wake);
        // Callbacks may create, cancel, or rearm alarms. Visit each handle
        // at most once per turn and release all timer borrows before firing.
        let entries = self.alarms.borrow().clone();
        for entry in entries.into_iter().filter_map(|entry| entry.upgrade()) {
            let due = entry
                .armed
                .borrow_mut()
                .as_mut()
                .is_some_and(|(_, sleep)| Pin::new(sleep).poll(&mut cx).is_ready());
            if due {
                entry.armed.take();
                (entry.fire.borrow_mut())();
            }
        }
    }
}

impl Drop for Hardware {
    fn drop(&mut self) {
        // A device handle can outlive its execution session.
        for entry in self.alarms.get_mut().iter().filter_map(Weak::upgrade) {
            entry.armed.take();
        }
    }
}
