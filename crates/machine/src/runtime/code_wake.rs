//! Reasons for the execution worker to service this PC's compiled code.

use crate::worker::WakeReasons;
use futures_util::task::{ArcWake, AtomicWaker};
use std::{sync::Arc, task::Waker};

const RETIRE: u8 = 1 << 0;
const COMPILER_REPLY: u8 = 1 << 1;

pub(crate) struct CodeWork(u8);

impl CodeWork {
    pub(crate) fn retirements(&self) -> bool {
        self.0 & RETIRE != 0
    }

    pub(crate) fn compiler_reply(&self) -> bool {
        self.0 & COMPILER_REPLY != 0
    }
}

/// Memory writers, the reply channel and the interruptible worker share the reason word.
/// The run registration also reaches a CPU future nested in a caller's ready queue.
#[derive(Default)]
pub(crate) struct CodeWake {
    reasons: Arc<WakeReasons>,
    run: AtomicWaker,
}

impl CodeWake {
    pub(crate) fn reasons(&self) -> Arc<WakeReasons> {
        self.reasons.clone()
    }

    pub(super) fn register(&self, task: &Waker) {
        self.run.register(task);
    }

    pub(super) fn unregister(&self) {
        self.run.take();
    }

    pub(crate) fn wake_run(&self) {
        self.run.wake();
    }

    pub(crate) fn record_retirements(&self) {
        self.reasons.record(RETIRE);
    }

    pub(crate) fn compiler_connected(&self) {
        // The first receive arms the channel even if no batch has finished yet.
        self.reasons.record(COMPILER_REPLY);
    }

    pub(crate) fn take_retirements(&self) -> bool {
        self.reasons.take(RETIRE) != 0
    }

    pub(super) fn pending(&self) -> bool {
        self.reasons.pending(RETIRE | COMPILER_REPLY)
    }

    pub(super) fn take_ready(&self) -> Option<CodeWork> {
        let ready = self.reasons.take(RETIRE | COMPILER_REPLY);
        (ready != 0).then_some(CodeWork(ready))
    }
}

// Only the local compiler reply channel receives this waker.
impl ArcWake for CodeWake {
    fn wake_by_ref(work: &Arc<Self>) {
        work.reasons.record(COMPILER_REPLY);
        work.wake_run();
    }
}
