mod support;

use relocade_machine::{CallContext, CpuView, Hook, HookResult, InvocationAccess};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::{pin, Pin},
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn configuration() -> support::Configuration {
    support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap()
}

struct Environment {
    destroyed: Rc<Cell<bool>>,
    cpu: Option<CpuView<InvocationAccess<Environment>>>,
}

impl Drop for Environment {
    fn drop(&mut self) {
        self.destroyed.set(true);
    }
}

type Access = Rc<RefCell<CallContext<Environment>>>;

#[derive(Default)]
struct Observation {
    ready: Cell<bool>,
    dropped: Cell<u32>,
    calls: RefCell<Vec<Access>>,
}

struct Work {
    call: Access,
    observation: Rc<Observation>,
}

impl Future for Work {
    type Output = HookResult;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<HookResult> {
        let mut call = self.call.borrow_mut();
        let polls = call.cpu.bx();
        call.cpu.set_bx(polls + 1);
        if self.observation.ready.get() {
            Poll::Ready(HookResult::Exit(ExecutionExit::Host(HostExit::Return)))
        } else {
            Poll::Pending
        }
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        if self.observation.ready.get() {
            // Even a completed custom future's destructor uses its own binding.
            self.call.borrow_mut().cpu.set_dx(0xabcd);
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            rejects(|| {
                self.call.borrow().cpu.ax();
            });
        }
        self.observation
            .dropped
            .set(self.observation.dropped.get() + 1);
    }
}

async fn check_poll_access() {
    let destroyed = Rc::new(Cell::new(false));
    let mut cpu = support::create_cpu(
        Environment {
            destroyed: destroyed.clone(),
            cpu: None,
        },
        configuration(),
    )
    .await
    .unwrap();
    let observation = Rc::new(Observation::default());
    let capture = observation.clone();
    cpu.install_hooks([Hook::new(
        0x100,
        move |mut call: CallContext<Environment>| {
            // This is an ordinary Fn body, before the returned future is first polled.
            let previous = capture.calls.borrow().last().cloned();
            if let Some(previous) = previous {
                #[cfg(not(target_arch = "wasm32"))]
                rejects(|| {
                    previous.borrow().cpu.ax();
                });
                assert_eq!(previous.borrow().memory.view().read_u16(0x200).unwrap(), 17);
            }
            call.state.with_mut(|state| {
                assert!(!state.destroyed.get());
                call.cpu.with(|mut cpu| cpu.set_ax(0x1234));
            });
            call.cpu.with(|cpu| {
                call.state.with_mut(|state| {
                    assert!(!state.destroyed.get());
                    assert_eq!(cpu.ax(), 0x1234);
                });
            });
            call.state.with(|state| {
                call.state
                    .with(|nested| assert!(Rc::ptr_eq(&state.destroyed, &nested.destroyed)));
            });
            assert_eq!(call.cpu.ax(), 0x1234);
            // Moving the CPU capability into the environment preserves its
            // uniqueness, and using it must not reborrow the environment itself.
            call.cpu = call.state.with_mut(|state| {
                state.cpu = Some(call.cpu);
                state.cpu.as_mut().unwrap().set_cx(23);
                assert_eq!(state.cpu.as_ref().unwrap().cx(), 23);
                state.cpu.take().unwrap()
            });
            call.cpu.set_bx(0);
            call.memory.view().write_u16(0x200, 17).unwrap();
            let call = Rc::new(RefCell::new(call));
            capture.calls.borrow_mut().push(call.clone());
            Work {
                call,
                observation: capture.clone(),
            }
        },
    )])
    .await
    .unwrap();

    let poll_pending = |cpu: &mut relocade_machine::Cpu<Environment>| {
        assert!(pin!(cpu.run(1))
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
    };
    poll_pending(&mut cpu);
    let first = observation.calls.borrow()[0].clone();
    assert_eq!(first.borrow().memory.view().read_u16(0x200).unwrap(), 17);
    #[cfg(not(target_arch = "wasm32"))]
    {
        rejects(|| {
            first.borrow().cpu.ax();
        });
        rejects(|| first.borrow().state.with(|_| ()));
    }
    poll_pending(&mut cpu);
    assert_eq!(observation.calls.borrow().len(), 1);
    assert_eq!(observation.dropped.get(), 0);
    observation.ready.set(true);
    assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(cpu.context().cpu.bx(), 3);
    assert_eq!(cpu.context().cpu.dx(), 0xabcd);
    assert_eq!(observation.dropped.get(), 1);
    #[cfg(not(target_arch = "wasm32"))]
    rejects(|| {
        first.borrow().cpu.ax();
    });

    // Entering the same hook creates a distinct invocation, with a fresh identity.
    assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(observation.calls.borrow().len(), 2);
    assert_eq!(cpu.context().cpu.bx(), 1);
    assert_eq!(observation.dropped.get(), 2);
    observation.ready.set(false);
    poll_pending(&mut cpu);
    drop(cpu);
    assert!(destroyed.get());
    assert_eq!(observation.dropped.get(), 3);
    // Escaped handles keep resources, but neither the backend nor environment.
    for call in observation.calls.borrow().iter() {
        assert_eq!(call.borrow().memory.view().read_u16(0x200).unwrap(), 17);
        #[cfg(not(target_arch = "wasm32"))]
        rejects(|| {
            call.borrow().cpu.ax();
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn rejects(operation: impl FnOnce()) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn invocation_access_covers_construction_polls_completion_and_cancellation() {
    pollster::block_on(check_poll_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn invocation_access_covers_construction_polls_completion_and_cancellation() {
    check_poll_access().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unwinding_revokes_access_and_releases_callback_borrows() {
    pollster::block_on(async {
        let mut cpu = support::create_cpu((), configuration()).await.unwrap();
        let escaped = Rc::new(RefCell::new(None));
        let capture = escaped.clone();
        cpu.install_hooks([Hook::new(0x100, async move |call| {
            let call = Rc::new(RefCell::new(call));
            *capture.borrow_mut() = Some(call.clone());
            rejects(|| call.borrow_mut().cpu.with(|_| panic!("callback unwinds")));
            rejects(|| {
                call.borrow_mut()
                    .state
                    .with_mut(|_| panic!("state callback unwinds"))
            });
            call.borrow_mut().cpu.with(|mut cpu| cpu.set_ax(17));
            assert_eq!(call.borrow().cpu.ax(), 17);
            panic!("poll unwinds");
        })])
        .await
        .unwrap();
        rejects(|| {
            pollster::block_on(cpu.run(1));
        });
        let call = escaped.borrow_mut().take().unwrap();
        rejects(|| {
            call.borrow().cpu.ax();
        });
        rejects(|| call.borrow().state.with(|_| ()));
        drop(cpu);
        rejects(|| {
            call.borrow().cpu.ax();
        });
    });
}
