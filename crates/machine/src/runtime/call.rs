//! Retained Rust futures and their invocation access lifecycle.

use super::{access::Access, hooks::Handler, machine::Machine, HookResult};
use futures_util::future::LocalBoxFuture;
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll},
};

pub(super) struct Call<T> {
    future: Option<LocalBoxFuture<'static, HookResult>>,
    access: Rc<Access<T>>,
}

impl<T: 'static> Call<T> {
    pub fn start(machine: &Rc<RefCell<Machine<T>>>, handler: Rc<dyn Handler<T>>) -> Self {
        let (access, context) = Access::new(machine);
        let future = handler.call(context);
        Self {
            future: Some(future),
            access,
        }
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<HookResult> {
        self.access.active.set(true);
        let result = self
            .future
            .as_mut()
            .expect("call is still running")
            .as_mut()
            .poll(cx);
        if result.is_ready() {
            // Completion cleanup still has access to this invocation.
            drop(self.future.take());
        }
        self.access.active.set(false);
        result
    }
}
