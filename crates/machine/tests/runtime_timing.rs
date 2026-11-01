mod support;
use futures_util::future::join;
use relocade_machine::{real_mode_interrupt, Clock, Runner};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{
    cell::{Cell, RefCell},
    future::{pending, poll_fn, Future},
    num::NonZeroU64,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use support::clock::ManualClock;

#[derive(Default)]
struct Input {
    ready: Cell<bool>,
    calls: Cell<u32>,
}

async fn check_timing() {
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
    let input = Rc::new(Input::default());
    let mut cpu = support::create_cpu(config).await.unwrap();
    cpu.context().cpu.set_sp(0xff00);
    cpu.context()
        .memory
        .write(0x100, &[0xcd, 0x21, 0x40, 0xeb, 0xfb])
        .unwrap();
    cpu.context().memory.write(0x84, &[0, 2, 0, 0]).unwrap();
    cpu.context()
        .memory
        .write(0x200, &real_mode_interrupt::HOOK_STUB)
        .unwrap();
    let captured = input.clone();
    cpu.install_hooks([real_mode_interrupt::hook(0x200, {
        let resources = cpu.memory();
        async move || {
            let mut memory = resources.view();

            let input = &captured;
            input.calls.set(input.calls.get() + 1);
            let value = input.calls.get() as u16;
            let saved = &value;
            poll_fn(|_| {
                if input.ready.replace(false) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            memory.write_u16(0x300, *saved).unwrap();
        }
    })])
    .await
    .unwrap();
    let clock = ManualClock::default();
    let mut runner = Runner::with_clock(clock.clone());
    runner.instruction_quantum = NonZeroU64::new(6).unwrap();
    let times = RefCell::new(Vec::new());
    let mut cx = Context::from_waker(Waker::noop());
    {
        let timer = async {
            for deadline in [10, 20].map(Duration::from_millis) {
                clock.wait_until(deadline).await;
                times.borrow_mut().push(clock.now());
            }
            pending::<()>().await;
        };
        let run = join(runner.run_cpu(&mut cpu), timer);
        let mut run = pin!(run);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(input.calls.get(), 1);
        clock.advance(Duration::from_millis(10));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(*times.borrow(), [Duration::from_millis(10)]);
        input.ready.set(true);
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(input.calls.get(), 2);
        clock.advance(Duration::from_millis(20));
        assert!(run.as_mut().poll(&mut cx).is_pending());
        input.ready.set(true);
        // The quantum ends and the runner cooperates with the other Rust futures.
        assert!(run.as_mut().poll(&mut cx).is_pending());
    }
    // Timer wakes must not replace the run waiter and replenish its budget.
    assert_eq!(cpu.context().cpu.instruction_count(), 6);
    assert_eq!(cpu.context().cpu.eip(), 0x102);
    assert_eq!(cpu.context().cpu.ax(), 1);
    assert_eq!(cpu.context().memory.read_u16(0x300).unwrap(), 2);
    assert_eq!(*times.borrow(), [10, 20].map(Duration::from_millis));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn deadlines_progress_during_pending_services_without_replenishing_the_cpu_budget() {
    pollster::block_on(check_timing());
}
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn deadlines_progress_during_pending_services_without_replenishing_the_cpu_budget() {
    check_timing().await;
}

async fn check_cooperation() {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x1000,
        PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut cpu = support::create_cpu(config).await.unwrap();
    let memory = cpu.memory();
    // INC word [0300]; JMP 0100. Warm compilation before polling synchronously.
    memory
        .view()
        .write(0x100, &[0xff, 0x06, 0, 3, 0xeb, 0xfa])
        .unwrap();
    assert_eq!(cpu.run(20).await, ExecutionExit::BudgetExhausted);
    let before = memory.view().read_u16(0x300).unwrap();
    let mut runner = Runner::with_clock(ManualClock::default());
    runner.instruction_quantum = NonZeroU64::new(2).unwrap();
    let mut run = pin!(runner.run_cpu(&mut cpu));
    let mut cx = Context::from_waker(Waker::noop());
    // No executor/browser turn occurs between these polls.
    for quantum in 1..=3 {
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert_eq!(memory.view().read_u16(0x300).unwrap(), before + quantum);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn busy_cpu_cooperates_without_waiting_for_a_host_turn() {
    pollster::block_on(check_cooperation());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn busy_cpu_cooperates_without_waiting_for_a_host_turn() {
    check_cooperation().await;
}
