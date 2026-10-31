//! Retained Rust futures and their invocation access lifecycle.

use super::{
    access::{InvocationId, Scope},
    hooks::Handler,
    machine::Machine,
    CallContext, HookResult,
};
mod future;
use future::OwnerFuture;
use relocade_wasm_runtime::WasmMemoryAccess;
use std::{
    rc::Rc,
    task::{Context, Poll},
};

pub(super) struct Call<T: 'static> {
    future: OwnerFuture<dyn Handler<T>, HookResult>,
    invocation: InvocationId,
}

pub(super) enum CallPoll<T: 'static> {
    Ready(HookResult),
    Pending(Call<T>),
}

impl<T: 'static> Call<T> {
    pub fn start(
        machine: &mut Machine<T>,
        scope: &Rc<Scope<T>>,
        handler: Rc<dyn Handler<T>>,
        cx: &mut Context<'_>,
    ) -> CallPoll<T> {
        let (invocation, context) = CallContext::new(scope, machine);
        let memory = machine.backend.memory(&machine.bindings.context);
        scope.bind(invocation, memory, &mut machine.state, || {
            // Ordinary Fn bodies may use CPU access while constructing a future.
            // Construction and the first poll share this one backend borrow.
            let future = OwnerFuture::new(handler, |handler| handler.call(context));
            Self { future, invocation }.poll_bound(cx)
        })
    }

    pub fn resume(
        self,
        machine: &mut Machine<T>,
        scope: &Scope<T>,
        cx: &mut Context<'_>,
    ) -> CallPoll<T> {
        let memory = machine.backend.memory(&machine.bindings.context);
        scope.bind(self.invocation, memory, &mut machine.state, || {
            self.poll_bound(cx)
        })
    }

    fn poll_bound(mut self, cx: &mut Context<'_>) -> CallPoll<T> {
        match self.future.poll(cx) {
            Poll::Pending => CallPoll::Pending(self),
            Poll::Ready(result) => {
                // Completion destruction uses this invocation's live binding.
                drop(self);
                CallPoll::Ready(result)
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{Cpu, CpuConfig, MachineConfig, Memory};
    use futures_util::future::LocalBoxFuture;
    use relocade_x86::{
        Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
        PhysicalMemoryMap, Segments,
    };
    use std::{future::Future, pin::Pin, task::Waker};

    struct Complete(CallContext);

    impl Future for Complete {
        type Output = HookResult;

        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<HookResult> {
            Poll::Ready(HookResult::Exit(ExecutionExit::Host(HostExit::Return)))
        }
    }

    impl Drop for Complete {
        fn drop(&mut self) {
            self.0.cpu.set_ax(0x1234);
        }
    }

    struct DirectHandler;

    impl Handler<()> for DirectHandler {
        fn call(&self, context: CallContext) -> LocalBoxFuture<'_, HookResult> {
            // Return the custom future itself so its destructor runs only when
            // Call releases it, after poll returns Ready.
            Box::pin(Complete(context))
        }
    }

    #[test]
    fn ready_future_is_destroyed_before_its_binding_ends() {
        pollster::block_on(async {
            let target = Compiler::new(ExecutionProfile::Real16).target();
            let config = CpuConfig::new(
                target,
                CpuState {
                    segments: Segments::real_mode(),
                    ..CpuState::default()
                },
            )
            .unwrap();
            let map =
                PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                    .unwrap();
            let memory = Memory::new(MachineConfig::new(0x10000, map).unwrap()).unwrap();
            let mut cpu = Cpu::new(memory, config, Default::default(), ())
                .await
                .unwrap();
            let scope = Rc::new(Scope::new());
            assert!(matches!(
                Call::start(
                    &mut cpu.execution.machine,
                    &scope,
                    Rc::new(DirectHandler),
                    &mut Context::from_waker(Waker::noop())
                ),
                CallPoll::Ready(HookResult::Exit(ExecutionExit::Host(HostExit::Return)))
            ));
            assert_eq!(cpu.context().cpu.ax(), 0x1234);
        });
    }
}
