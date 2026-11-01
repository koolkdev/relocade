mod support;
use relocade_machine::{Hook, HookResult};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

// Captured mutable state is independent of CPU access and need not be Clone.
struct Environment {
    calls: u32,
    dropped: Rc<Cell<bool>>,
}

impl Drop for Environment {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

async fn check_context_lifetime() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let dropped = Rc::new(Cell::new(false));
    let environment = Rc::new(RefCell::new(Environment {
        calls: 0,
        dropped: dropped.clone(),
    }));
    let observer = Rc::downgrade(&environment);
    let mut runtime = support::create_cpu(config).await.unwrap();
    let ready = Rc::new(Cell::new(false));
    let complete = ready.clone();
    runtime
        .install_hooks([Hook::new(0x100, async move |call| {
            environment.borrow_mut().calls += 1;
            call.cpu.set_eax(0x1234);
            call.memory.write_u32(0x200, 0x5678).unwrap();
            poll_fn(|_| {
                if ready.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        })])
        .await
        .unwrap();
    {
        let mut run = pin!(runtime.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    assert_eq!(runtime.memory().view().read_u32(0x200).unwrap(), 0x5678);
    // A pending service retains resources, but no mutable application-state borrow.
    assert_eq!(observer.upgrade().unwrap().borrow_mut().calls, 1);
    #[cfg(not(target_arch = "wasm32"))]
    {
        rejects(|| {
            runtime.context();
        });
    }
    complete.set(true);
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runtime.context().cpu.eax(), 0x1234);
    assert_eq!(observer.upgrade().unwrap().borrow().calls, 1);
    assert!(!dropped.get());
    // Dropping the CPU while another invocation waits releases its owned machine.
    complete.set(false);
    {
        let mut run = pin!(runtime.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    }
    let memory = runtime.memory();
    drop(runtime);
    assert!(dropped.get());
    assert_eq!(memory.view().read_u32(0x200).unwrap(), 0x5678);
}

#[cfg(not(target_arch = "wasm32"))]
fn rejects(operation: impl FnOnce()) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn captured_state_outlives_pending_work_and_is_released_with_the_cpu() {
    pollster::block_on(check_context_lifetime());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn captured_state_outlives_pending_work_and_is_released_with_the_cpu() {
    check_context_lifetime().await;
}
