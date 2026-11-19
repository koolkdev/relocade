//! A FIFO of task IDs. Producers, including browser UI callbacks, never lock the owner.

use super::WorkerWake;
use futures_channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Weak,
    },
    task::{Wake, Waker},
};

pub(super) struct ReadyQueue {
    sender: Arc<UnboundedSender<Arc<Task>>>,
    receiver: UnboundedReceiver<Arc<Task>>,
    polling: Option<Arc<Task>>,
}

impl Default for ReadyQueue {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded();
        Self {
            sender: Arc::new(sender),
            receiver,
            polling: None,
        }
    }
}

impl ReadyQueue {
    pub(super) fn task(&self, id: usize, worker: Arc<WorkerWake>) -> Waker {
        Waker::from(Arc::new(Task {
            id,
            sender: Arc::downgrade(&self.sender),
            state: AtomicU8::new(0),
            worker,
        }))
    }

    pub(super) fn has_ready(&self) -> bool {
        !self.sender.is_empty()
            || self
                .polling
                .as_ref()
                .is_some_and(|task| task.state.load(Ordering::SeqCst) & NOTIFIED != 0)
    }

    pub(super) fn begin_turn(&mut self) -> Option<(usize, Waker)> {
        let task = self.receiver.try_recv().ok()?;
        // All wakes up to this point are consumed by the upcoming poll.
        task.state.store(POLLING, Ordering::SeqCst);
        let id = task.id;
        let waker = Waker::from(task.clone());
        self.polling = Some(task);
        Some((id, waker))
    }

    pub(super) fn end_turn(&mut self) {
        let task = self.polling.take().expect("active task");
        // If a wake arrived during the poll, append the task *after* all the
        // work its poll woke. A concurrent wake after the exchange can enqueue
        // an idle task itself. Only one side sends a queue entry.
        if task.state.fetch_and(!POLLING, Ordering::SeqCst) & NOTIFIED != 0 {
            self.sender
                .unbounded_send(task)
                .expect("ready queue is open");
        }
    }
}

// An idle task has neither bit. NOTIFIED alone owns one queue entry. While
// POLLING, NOTIFIED reserves that entry for the end of the current turn.
const POLLING: u8 = 1;
const NOTIFIED: u8 = 2;

struct Task {
    id: usize,
    sender: Weak<UnboundedSender<Arc<Task>>>,
    state: AtomicU8,
    worker: Arc<WorkerWake>,
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let Some(sender) = self.sender.upgrade() else {
            return;
        };
        if self.state.fetch_or(NOTIFIED, Ordering::SeqCst) == 0
            && sender.unbounded_send(self.clone()).is_err()
        {
            return;
        }
        // Another event for an already queued task can arrive during a CPU
        // turn. Queue membership coalesces polls; it does not suppress a break.
        self.worker.wake_by_ref();
    }
}

pub(super) struct Session<'a>(pub(super) &'a RefCell<ReadyQueue>);

impl Drop for Session<'_> {
    fn drop(&mut self) {
        self.0.take();
    }
}
