mod support;
use relocade_machine::{CpuContext, Hook, HookResult};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

// Deliberately not Clone. A saved context must neither require cloning its
// environment nor keep that environment alive through an ownership cycle.
struct Environment {
    saved: RefCell<Option<CpuContext<Environment>>>,
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
            saved: RefCell::new(None),
            dropped: dropped.clone(),
        },
        config,
    )
    .await
    .unwrap();
    let stopped = runtime.context();
    runtime
        .install_hooks([Hook::<Environment>::new(0x100, |call| async move {
            call.with_state(|state| *state.saved.borrow_mut() = Some(call.clone()));
            std::future::pending::<()>().await;
            HookResult::Exit(ExecutionExit::HostReturn)
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
    // Service and stopped handles share live state without retaining its borrow.
    let saved = stopped.with_state(|state| state.saved.borrow().as_ref().unwrap().clone());
    saved.with_state_mut(|_| saved.cpu().set_eax(0x1234));
    assert_eq!(stopped.cpu().eax(), 0x1234);
    saved.memory().write_u32(0x200, 0x5678).unwrap();
    assert_eq!(stopped.memory().read_u32(0x200).unwrap(), 0x5678);
    runtime.cancel_pending_call();
    #[cfg(not(target_arch = "wasm32"))]
    {
        rejects(|| {
            saved.cpu().eax();
        });
        rejects(|| {
            saved.memory().read_u8(0x200).unwrap();
        });
        rejects(|| saved.with_state(|_| ()));
    }
    // Dropping the runner revokes stopped handles immediately, even while this
    // scoped environment operation temporarily keeps the core allocation alive.
    stopped.with_state(|_| {
        drop(runtime);
        assert!(!dropped.get());
        #[cfg(not(target_arch = "wasm32"))]
        {
            rejects(|| {
                stopped.cpu().eax();
            });
            rejects(|| stopped.with_state(|_| ()));
        }
    });
    assert!(dropped.get());
    #[cfg(not(target_arch = "wasm32"))]
    rejects(|| {
        stopped.memory().read_u8(0x200).unwrap();
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn rejects(operation: impl FnOnce()) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn owned_contexts_are_revoked_without_retaining_the_environment() {
    pollster::block_on(check_context_lifetime());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn owned_contexts_are_revoked_without_retaining_the_environment() {
    check_context_lifetime().await;
}
