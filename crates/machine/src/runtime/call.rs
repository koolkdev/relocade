//! Retained Rust futures and their invocation access lifecycle.

use super::{
    access::{InvocationId, Scope},
    hooks::Handler,
    machine::Machine,
    CallContext, HookResult,
};
use futures_util::future::LocalBoxFuture;
use relocade_wasm_runtime::WasmMemoryAccess;
use std::{
    rc::Rc,
    task::{Context, Poll},
};

pub(super) struct Call {
    future: Option<LocalBoxFuture<'static, HookResult>>,
    invocation: InvocationId,
}

impl Call {
    pub fn start<T: 'static>(
        machine: &Machine<T>,
        scope: &Rc<Scope<T>>,
        handler: Rc<dyn Handler<T>>,
    ) -> Self {
        let (invocation, context) = CallContext::new(scope, machine);
        let future = handler.call(context);
        Self {
            future: Some(future),
            invocation,
        }
    }

    pub fn poll<T>(
        &mut self,
        machine: &mut Machine<T>,
        scope: &Scope<T>,
        cx: &mut Context<'_>,
    ) -> Poll<HookResult> {
        let memory = machine.backend.memory(&machine.bindings.context);
        scope.bind(self.invocation, memory, &mut machine.state, || {
            let result = self
                .future
                .as_mut()
                .expect("call is still running")
                .as_mut()
                .poll(cx);
            if result.is_ready() {
                // Completion cleanup runs with this invocation's access still bound.
                drop(self.future.take());
            }
            result
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{Cpu, CpuConfig, MachineConfig, Memory};
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
        fn call(self: Rc<Self>, context: CallContext) -> LocalBoxFuture<'static, HookResult> {
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
            let mut call = Call::start(&cpu.execution.machine, &scope, Rc::new(DirectHandler));
            assert_eq!(
                call.poll(
                    &mut cpu.execution.machine,
                    &scope,
                    &mut Context::from_waker(Waker::noop())
                ),
                Poll::Ready(HookResult::Exit(ExecutionExit::Host(HostExit::Return)))
            );
            assert_eq!(cpu.context().cpu.ax(), 0x1234);
        });
    }
}
