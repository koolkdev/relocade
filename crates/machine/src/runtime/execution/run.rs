//! Wake the active CPU run for an exit request or a local compiler reply.

use super::{Active, Cpu};
use futures_util::task::AtomicWaker;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{ready, Context, Poll, Wake, Waker},
};

pub(super) struct RunWake {
    pub(super) task: AtomicWaker,
    compiler_ready: AtomicBool,
}

// Only the compiler receiver gets this waker. Exit requests wake the task directly.
impl Wake for RunWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.compiler_ready.store(true, Ordering::Release);
        self.task.wake();
    }
}

pub(crate) struct CpuRun(Arc<RunWake>);

impl Cpu {
    pub(crate) fn listen_for_run(&self) -> CpuRun {
        let run = Arc::new(RunWake {
            task: AtomicWaker::new(),
            // The first turn consumes any buffered reply and registers the channel waker.
            compiler_ready: AtomicBool::new(true),
        });
        self.exit.0.waiter.replace(Arc::downgrade(&run));
        CpuRun(run)
    }
}

impl CpuRun {
    pub(crate) fn poll_enter(&self, cpu: &mut Cpu, cx: &mut Context<'_>) -> Poll<Option<Active>> {
        self.0.task.register(cx.waker());
        let permit = ready!(cpu.poll_enter(cx));
        if self.0.compiler_ready.load(Ordering::Acquire) {
            self.0.compiler_ready.store(false, Ordering::Relaxed);
            let waker = Waker::from(self.0.clone());
            cpu.instance
                .poll_compiler_replies(&mut Context::from_waker(&waker));
        }
        Poll::Ready(permit)
    }
}

impl Drop for CpuRun {
    fn drop(&mut self) {
        // The channel can retain its waker until a replacement run polls it.
        self.0.task.take();
    }
}
