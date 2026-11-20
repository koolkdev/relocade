//! Compiler deliveries and modified installations for one CPU.

use crate::{memory::code::ModuleId, worker::WakeReasons, Cpu};
use futures_util::task::{ArcWake, AtomicWaker};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    task::Waker,
};

const RETIRE: u8 = 1 << 0;
const COMPILER_REPLY: u8 = 1 << 1;

pub(in crate::runtime) struct CodeWork(u8);

impl CodeWork {
    pub(super) fn retirements(&self) -> bool {
        self.0 & RETIRE != 0
    }

    pub(super) fn compiler_reply(&self) -> bool {
        self.0 & COMPILER_REPLY != 0
    }
}

/// Memory writers, the reply channel and the interruptible worker share the reason word.
/// The run registration also reaches a CPU future nested in a caller's ready queue.
#[derive(Default)]
pub(crate) struct CodeWake {
    reasons: Arc<WakeReasons>,
    run: AtomicWaker,
    modified: Mutex<BTreeSet<ModuleId>>,
}

impl Cpu {
    pub(crate) fn code_wake(&self) -> &Arc<CodeWake> {
        &self.instance.code.wake
    }
}

impl CodeWake {
    pub(crate) fn reasons(&self) -> Arc<WakeReasons> {
        self.reasons.clone()
    }

    pub(in crate::runtime) fn register(&self, task: &Waker) {
        self.run.register(task);
    }

    pub(in crate::runtime) fn unregister(&self) {
        self.run.take();
    }

    pub(crate) fn wake_run(&self) {
        self.run.wake();
    }

    pub(crate) fn record_retirement(&self, id: ModuleId) -> Option<Waker> {
        self.modified.lock().unwrap().insert(id);
        self.reasons.record(RETIRE);
        self.run.take()
    }

    pub(crate) fn forget_retirement(&self, id: ModuleId) {
        self.modified.lock().unwrap().remove(&id);
    }

    pub(super) fn drain_modified_modules(&self) -> BTreeSet<ModuleId> {
        std::mem::take(&mut *self.modified.lock().unwrap())
    }

    pub(crate) fn take_modified_modules(&self) -> BTreeSet<ModuleId> {
        if self.take_retirements() {
            self.drain_modified_modules()
        } else {
            BTreeSet::new()
        }
    }

    pub(crate) fn compiler_connected(&self) {
        // The first receive arms the channel even if no batch has finished yet.
        self.reasons.record(COMPILER_REPLY);
    }

    pub(super) fn take_retirements(&self) -> bool {
        self.reasons.take(RETIRE) != 0
    }

    pub(in crate::runtime) fn pending(&self) -> bool {
        self.reasons.pending(RETIRE | COMPILER_REPLY)
    }

    pub(in crate::runtime) fn take_ready(&self) -> Option<CodeWork> {
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
