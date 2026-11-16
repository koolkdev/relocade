//! A stopped boundary shared by the CPUs owned by one machine.

use futures_util::{lock::Mutex as AsyncMutex, task::AtomicWaker};
use std::{
    future::poll_fn,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
    task::{Context, Poll},
};

const PAUSED: usize = 1 << (usize::BITS - 1);

#[derive(Default)]
pub struct Execution {
    // Closing entry and counting active operations share one atomic so a CPU
    // cannot enter between the pause request and its drained check.
    state: AtomicUsize,
    pause: AsyncMutex<()>,
    drained: AtomicWaker,
    members: Mutex<Vec<Weak<AtomicWaker>>>,
}

impl Execution {
    pub(crate) fn member(self: &Arc<Self>) -> Member {
        let resumed = Arc::new(AtomicWaker::new());
        let mut members = self.members.lock().unwrap();
        members.retain(|member| member.strong_count() != 0);
        members.push(Arc::downgrade(&resumed));
        Member {
            execution: self.clone(),
            resumed,
        }
    }

    pub(crate) async fn pause(&self, interrupt: impl FnOnce()) -> impl Drop + '_ {
        let serial = self.pause.lock().await;
        self.state.fetch_or(PAUSED, Ordering::SeqCst);
        // The guard reopens entry even if waiting for an active CPU is cancelled.
        let paused = Paused {
            execution: self,
            _serial: serial,
        };
        self.wake_members();
        interrupt();
        poll_fn(|cx| {
            self.drained.register(cx.waker());
            if self.state.load(Ordering::SeqCst) == PAUSED {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        self.drained.take();
        paused
    }

    fn wake_members(&self) {
        let members: Vec<_> = self
            .members
            .lock()
            .unwrap()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for member in members {
            member.wake();
        }
    }
}

/// Travels with the CPU's execution capability, independently of engine resources.
#[derive(crate::worker::Transfer)]
pub struct Member {
    execution: Arc<Execution>,
    resumed: Arc<AtomicWaker>,
}

impl Member {
    /// Attempts entry immediately, without registering a waiter when paused.
    pub(crate) fn try_enter(&self) -> Option<Active> {
        let mut state = self.execution.state.load(Ordering::SeqCst);
        loop {
            if state & PAUSED != 0 {
                return None;
            }
            match self.execution.state.compare_exchange_weak(
                state,
                state + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Some(Active {
                        execution: self.execution.clone(),
                        wake: self.resumed.clone(),
                    })
                }
                Err(current) => state = current,
            }
        }
    }

    /// Attempts entry and arranges a wakeup on resumption if paused.
    pub(crate) fn poll_enter(&self, cx: &mut Context<'_>) -> Poll<Active> {
        // Retain the executor waker through the turn so a pause request can
        // interrupt busy execution through its existing worker connection.
        self.resumed.register(cx.waker());
        match self.try_enter() {
            Some(active) => Poll::Ready(active),
            None => Poll::Pending,
        }
    }
}

pub(crate) struct Active {
    execution: Arc<Execution>,
    wake: Arc<AtomicWaker>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.wake.take();
        if self.execution.state.fetch_sub(1, Ordering::SeqCst) == (PAUSED | 1) {
            self.execution.drained.wake();
        }
    }
}

struct Paused<'a> {
    execution: &'a Execution,
    _serial: futures_util::lock::MutexGuard<'a, ()>,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        self.execution.drained.take();
        self.execution.state.fetch_and(!PAUSED, Ordering::SeqCst);
        self.execution.wake_members();
    }
}

#[cfg(test)]
mod tests;
