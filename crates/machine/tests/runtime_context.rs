mod support;
use relocade_machine::{Hook, HookResult};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

// Deliberately not Clone. A scoped handler edits the machine's actual state.
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
    let mut runtime = support::create_cpu(
        Environment {
            calls: 0,
            dropped: dropped.clone(),
        },
        config,
    )
    .await
    .unwrap();
    let ready = Rc::new(Cell::new(false));
    let complete = ready.clone();
    runtime
        .install_hooks([Hook::<Environment>::new(0x100, async move |mut call| {
            call.with(|context| {
                context.state.calls += 1;
                assert!(context.state.calls > 0);
            });
            call.cpu.set_eax(0xabcd_1234);
            call.cpu.set_ah(0x56);
            assert_eq!(call.cpu.eax(), 0xabcd_5634);
            // The retained view borrows resources, leaving the backend available.
            let mut memory = call.memory.view();
            memory.write_u32(0x200, 0x5678).unwrap();
            call.with(|context| {
                assert_eq!(context.cpu.ax(), 0x5634);
            });
            poll_fn(|_| {
                if ready.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            call.cpu.set_ax(0x1234);
            assert_eq!(memory.read_u32(0x200).unwrap(), 0x5678);
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        rejects(|| {
            runtime.with(|_| ());
        });
    }
    complete.set(true);
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runtime.with(|call| call.cpu.eax()), 0xabcd_1234);
    assert_eq!(runtime.with(|call| call.state.calls), 1);
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
fn escaped_invocation_access_is_invalid_between_polls_and_after_release() {
    use relocade_machine::CallContext;
    use relocade_x86::{DescriptorTableRegister, SystemSegmentRegister};
    use std::cell::RefCell;

    pollster::block_on(async {
        let config = support::configuration(
            Compiler::new(ExecutionProfile::Real16).target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..Default::default()
            },
            0x10000,
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap();
        let mut cpu = support::create_cpu((), config).await.unwrap();
        let escaped = Rc::new(RefCell::new(None));
        let capture = escaped.clone();
        let ready = Rc::new(Cell::new(false));
        let complete = ready.clone();
        cpu.install_hooks([Hook::new(0x100, async move |call: CallContext| {
            let call = Rc::new(RefCell::new(call));
            *capture.borrow_mut() = Some(call.clone());
            poll_fn(|_| {
                if ready.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            call.borrow_mut().cpu.set_ax(42);
            HookResult::Exit(ExecutionExit::Host(HostExit::Return))
        })])
        .await
        .unwrap();
        assert!(pin!(cpu.run(1))
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let call = escaped.borrow_mut().take().unwrap();
        let reject_access = || {
            rejects(|| call.borrow().with(|_| ()));
            rejects(|| {
                call.borrow().cpu.ax();
            });
            rejects(|| call.borrow_mut().cpu.set_ax(7));
            // Access validation precedes early validation errors as well as storage I/O.
            rejects(|| {
                let _ = call.borrow_mut().cpu.set_idtr(DescriptorTableRegister {
                    base: 1,
                    limit: 1,
                    ..Default::default()
                });
            });
            rejects(|| {
                let _ = call.borrow_mut().cpu.restore(&CpuState::default());
            });
            let invalid = SystemSegmentRegister {
                usable: 1,
                ..Default::default()
            };
            rejects(|| {
                let _ = call.borrow_mut().cpu.set_ldtr(invalid);
            });
            rejects(|| {
                let _ = call.borrow_mut().cpu.set_tr(invalid);
            });
        };
        reject_access();
        complete.set(true);
        assert_eq!(cpu.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(cpu.with(|context| context.cpu.ax()), 42);
        reject_access();
        let memory = call.borrow().memory.clone();
        drop(cpu);
        reject_access();
        memory.view().write_u16(0x200, 17).unwrap();
        assert_eq!(call.borrow().memory.view().read_u16(0x200).unwrap(), 17);
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completion_releases_scoped_state_and_cpu_drop_releases_pending_work() {
    pollster::block_on(check_context_lifetime());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn completion_releases_scoped_state_and_cpu_drop_releases_pending_work() {
    check_context_lifetime().await;
}
