mod support;
use relocade_machine::{real_mode_interrupt, Hook, HookResult};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, HostExit, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Default)]
struct Input {
    values: VecDeque<u16>,
    waker: Option<Waker>,
}

#[derive(Default)]
struct State {
    input: RefCell<Input>,
    started: Cell<u32>,
    first_awaits: Cell<u32>,
    completed: Cell<u32>,
    dropped: Cell<u32>,
}

impl State {
    async fn read(&self) -> u16 {
        poll_fn(|cx| {
            let mut input = self.input.borrow_mut();
            match input.values.pop_front() {
                Some(value) => Poll::Ready(value),
                None => {
                    input.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }

    fn send(&self, values: &[u16]) {
        let waker = {
            let mut input = self.input.borrow_mut();
            input.values.extend(values);
            input.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct Operation(Rc<State>);

impl Drop for Operation {
    fn drop(&mut self) {
        self.0.dropped.set(self.0.dropped.get() + 1);
    }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn assert_pending(future: impl Future) {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
}

async fn check_pending_calls() {
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
    let state = Rc::new(State::default());
    let mut runtime = support::create_cpu(config).await.unwrap();
    runtime.context().cpu.set_sp(0xff00);
    runtime.context().cpu.set_eflags(0x202);
    runtime
        .context()
        .memory
        .write(0x100, &[0xcd, 0x21, 0x40, 0xeb, 0xfb]) // INT 21h; INC AX; JMP 0100
        .unwrap();
    runtime.context().memory.write(0x84, &[0, 2, 0, 0]).unwrap();
    runtime
        .context()
        .memory
        .write(0x200, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    let captured = state.clone();
    runtime
        .install_hooks([Hook::new(0x200, {
            let resources = runtime.memory();
            let cpu = runtime.cpu_state();
            async move || {
                let mut memory = resources.view();

                // Distinct fields stay borrowed together through both awaits.
                let state = &captured;
                state.started.set(state.started.get() + 1);
                let operation = Operation(state.clone());
                let first = operation.0.read().await;
                cpu.set_bx(first);
                operation
                    .0
                    .first_awaits
                    .set(operation.0.first_awaits.get() + 1);
                // This reference to a local remains valid while the future is pinned.
                let values = [first];
                let saved = &values[0];
                let second = operation.0.read().await;
                assert_eq!(cpu.bx(), *saved);
                let sum = *saved + second;
                cpu.set_bx(sum);
                drop(operation);
                state.completed.set(state.completed.get() + 1);
                memory.write(0x300, &sum.to_le_bytes()).unwrap();
                real_mode_interrupt::return_from_interrupt(&cpu)
            }
        })])
        .await
        .unwrap();

    assert_pending(runtime.run(6));
    let shared = runtime.memory();
    let reader = shared.view();
    assert_pending(runtime.run(100));
    assert_eq!(state.started.get(), 1);
    assert_eq!(runtime.run(0).await, ExecutionExit::BudgetExhausted);
    // An unpolled run has no effect on the retained service.
    drop(runtime.run(5));
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    {
        let mut run = pin!(runtime.run(5));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[10]);
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(state.first_awaits.get(), 1);
        assert_eq!(state.started.get(), 1);
        assert_eq!(state.dropped.get(), 0);
        state.send(&[20]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        // Independent readers see writes from the completed operation while
        // the next service waits.
        assert_eq!(reader.read_u16(0x300).unwrap(), 30);
        assert_eq!(state.started.get(), 2);
        assert_eq!(state.completed.get(), 1);
        state.send(&[30, 40]);
        assert!(matches!(
            run.as_mut().poll(&mut cx),
            Poll::Ready(ExecutionExit::BudgetExhausted)
        ));
    }
    // Both suspensions shared the original target, ending at the second IRET.
    assert_eq!(state.started.get(), 2);
    assert_eq!(state.first_awaits.get(), 2);
    assert_eq!(state.completed.get(), 2);
    assert_eq!(state.dropped.get(), 2);
    let cpu = runtime.context().cpu.snapshot();
    assert_eq!(cpu.instruction_count, 6);
    assert_eq!(cpu.eip, 0x102);
    assert_eq!(cpu.registers.eax, 1);
    assert_eq!(cpu.registers.ebx, 70);
    assert_eq!(cpu.registers.esp, 0xff00);
    assert_eq!(cpu.flags.eflags(), 0x202);
    let mut bytes = [0; 2];
    runtime.context().memory.read(0x300, &mut bytes).unwrap();
    assert_eq!(bytes, [70, 0]);

    // Dropping a waiter leaves the future, its local references and partial
    // effects intact. The service retains exclusive access until it returns.
    {
        let mut run = pin!(runtime.run(10));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        state.send(&[55]);
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(state.dropped.get(), 2);
    assert_eq!(state.started.get(), 3);
    assert_eq!(state.completed.get(), 2);
    state.send(&[65]);
    assert_eq!(runtime.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(state.completed.get(), 2);
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.context().cpu.bx(), 120);
    assert_eq!(state.started.get(), 3);
    assert_eq!(state.completed.get(), 3);
    assert_eq!(state.dropped.get(), 3);

    // Once the service completes, guest execution can continue without a host call.
    runtime.context().cpu.set_eip(0x400);
    runtime
        .context()
        .memory
        .write(0x400, &[0xeb, 0xfe])
        .unwrap();
    assert_eq!(runtime.run(1).await, ExecutionExit::BudgetExhausted);
    let count = runtime.context().cpu.instruction_count();
    runtime
        .context()
        .memory
        .write(0x500, &[0x0f, 0x0b]) // UD2: completing must not enter the guest.
        .unwrap();
    runtime
        .install_hooks([Hook::new(0x400, {
            let cpu = runtime.cpu_state();
            async move || {
                // Completion returns directly even if the continuation names guest code.
                cpu.set_eip(0x500);
                let value = std::future::ready(99).await;
                cpu.set_bx(value);
                let value = std::future::ready(value + 1).await;
                assert_eq!(cpu.bx(), 99);
                cpu.set_cx(value);
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }
        })])
        .await
        .unwrap();
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runtime.context().cpu.cx(), 100);
    assert_eq!(runtime.context().cpu.eip(), 0x500);
    assert_eq!(runtime.context().cpu.instruction_count(), count);

    runtime.context().cpu.set_eip(0x100);
    assert_pending(runtime.run(10));
    assert_eq!(state.dropped.get(), 3);
    drop(runtime);
    assert_eq!(state.dropped.get(), 4);
    assert_eq!(state.completed.get(), 3);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_services_survive_waiter_changes_and_preserve_each_run_budget() {
    pollster::block_on(check_pending_calls());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pending_services_survive_waiter_changes_and_preserve_each_run_budget() {
    check_pending_calls().await;
}

async fn check_completed_replacement() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x400,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let state = Rc::new(State::default());
    let mut runtime = support::create_cpu(config).await.unwrap();
    let registration = Rc::new(());
    let released = Rc::downgrade(&registration);
    let input = state.clone();
    runtime
        .install_hooks([Hook::new(0x400, {
            let cpu = runtime.cpu_state();
            async move || {
                let registration = registration.clone();
                let value = input.read().await;
                cpu.set_ax(value);
                drop(registration);
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }
        })])
        .await
        .unwrap();
    assert_pending(runtime.run(1));
    assert!(released.upgrade().is_some());
    state.send(&[42]);
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runtime.context().cpu.ax(), 42);
    // Completion drops the invocation; replacing the registration releases
    // the final owner of its captures.
    assert!(released.upgrade().is_some());
    runtime
        .install_hooks([Hook::new(0x400, {
            let cpu = runtime.cpu_state();
            async move || {
                cpu.set_ax(99);
                HookResult::Exit(ExecutionExit::Host(HostExit::Return))
            }
        })])
        .await
        .unwrap();
    assert!(released.upgrade().is_none());
    assert_eq!(runtime.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runtime.context().cpu.ax(), 99);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completion_allows_replacement_and_releases_the_old_registration() {
    pollster::block_on(check_completed_replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn completion_allows_replacement_and_releases_the_old_registration() {
    check_completed_replacement().await;
}

struct Completion {
    cpu: relocade_machine::CpuStateAccess,
    first: bool,
}

impl Future for Completion {
    type Output = HookResult;
    fn poll(mut self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<HookResult> {
        if std::mem::replace(&mut self.first, false) {
            Poll::Pending
        } else {
            self.cpu.set_ax(55);
            Poll::Ready(HookResult::Exit(ExecutionExit::Host(HostExit::Return)))
        }
    }
}

fn custom_completion(cpu: relocade_machine::CpuStateAccess) -> Completion {
    cpu.set_bx(99);
    Completion { cpu, first: true }
}

async fn check_custom_completion() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x400,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu(config).await.unwrap();
    cpu.install_hooks([Hook::new(0x400, {
        let state = cpu.cpu_state();
        move || custom_completion(state.clone())
    })])
    .await
    .unwrap();
    assert_pending(cpu.run(1));
    // Captured resource storage stays stable when its runner moves.
    let mut moved = Box::new(cpu);
    assert_eq!(moved.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(moved.context().cpu.ax(), 55);
    assert_eq!(moved.context().cpu.bx(), 99);
    assert_pending(moved.run(1));
    let state = moved.cpu_state();
    state.set_ax(77);
    // Abandonment releases the future without running its completion effects.
    drop(moved);
    assert_eq!(state.ax(), 77);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn custom_future_keeps_access_through_completion_after_the_cpu_moves() {
    pollster::block_on(check_custom_completion());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn custom_future_keeps_access_through_completion_after_the_cpu_moves() {
    check_custom_completion().await;
}

async fn check_completion_and_abandonment() {
    for complete in [false, true] {
        let config = support::configuration(
            Compiler::new(ExecutionProfile::Real16).target(),
            CpuState {
                eip: 0x400,
                segments: Segments::real_mode(),
                ..CpuState::default()
            },
            0x10000,
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap();
        let mut runner = support::create_cpu(config).await.unwrap();
        let state = runner.cpu_state();
        state.set_eflags(2);
        let input = Rc::new(State::default());
        runner
            .install_hooks([Hook::new(0x400, {
                let cpu = state.clone();
                let input = input.clone();
                async move || {
                    let _operation = Operation(input.clone());
                    let saved_if = cpu.eflags() & 0x200;
                    cpu.set_eflags(cpu.eflags() | 0x200);
                    cpu.set_ax(input.read().await);
                    // Normal completion restores only this service's temporary IF.
                    // Abandonment destroys its future without executing this code.
                    cpu.set_eflags((cpu.eflags() & !0x200) | saved_if);
                    input.completed.set(input.completed.get() + 1);
                    HookResult::Exit(ExecutionExit::Host(HostExit::Return))
                }
            })])
            .await
            .unwrap();
        assert_pending(runner.run(1));
        state.set_eflags(0x603); // Live IF, DF and carry can change while parked.
        input.send(&[42]);
        assert_eq!(input.completed.get(), 0); // Readiness alone does not poll it.
        if complete {
            assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(state.eflags(), 0x403);
            assert_eq!(state.ax(), 42);
        }
        drop(runner);
        assert_eq!(input.completed.get(), u32::from(complete));
        assert_eq!(input.dropped.get(), 1);
        assert_eq!(state.eflags(), if complete { 0x403 } else { 0x603 });
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn abandonment_releases_the_future_without_running_cpu_completion() {
    pollster::block_on(check_completion_and_abandonment());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn abandonment_releases_the_future_without_running_cpu_completion() {
    check_completion_and_abandonment().await;
}
