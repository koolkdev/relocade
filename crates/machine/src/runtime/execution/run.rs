//! Bind the active CPU future to work published by the code cache and exit handle.

use super::{Active, Cpu, ExitHandle};
use crate::runtime::CodeWake;
use futures_util::task::waker_ref;
use std::{
    sync::{Arc, Weak},
    task::{ready, Context, Poll},
};

pub(crate) struct CpuRun {
    work: Arc<CodeWake>,
    exit: ExitHandle,
}

impl Cpu {
    pub(crate) fn listen_for_run(&self) -> CpuRun {
        let work = self.code_wake().clone();
        self.exit.0.waiter.replace(Arc::downgrade(&work));
        CpuRun {
            work,
            exit: self.exit.clone(),
        }
    }
}

impl CpuRun {
    pub(crate) fn poll_enter(&self, cpu: &mut Cpu, cx: &mut Context<'_>) -> Poll<Option<Active>> {
        // A synchronous host call can patch code and return straight to the guest.
        // The entry path repairs those links itself, without interrupting this run.
        self.work.unregister();
        let permit = ready!(cpu.poll_enter(cx));
        if let Some(ready) = self.work.take_ready() {
            let waker = waker_ref(&self.work);
            cpu.instance
                .code
                .service(ready, &mut Context::from_waker(&waker));
        }
        Poll::Ready(permit)
    }

    pub(crate) fn park<T>(&self, cx: &Context<'_>) -> Poll<T> {
        self.work.register(cx.waker());
        // A producer may have published work while this run was still polling.
        if self.work.pending() || self.exit.is_requested() {
            self.work.wake_run();
        }
        Poll::Pending
    }
}

impl Drop for CpuRun {
    fn drop(&mut self) {
        self.exit.0.waiter.replace(Weak::new());
        self.work.unregister();
    }
}
