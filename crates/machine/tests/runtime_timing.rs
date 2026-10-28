mod support;
use futures_util::future::join;
use relocade_machine::{real_mode_interrupt, Clock, Hook, Runner};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
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

#[derive(Clone, Default)]
struct ManualClock(Rc<ClockState>);
#[derive(Default)]
struct ClockState {
    now: Cell<Duration>,
    waker: RefCell<Option<Waker>>,
}
impl ManualClock {
    fn advance(&self, now: Duration) {
        assert!(now >= self.0.now.get());
        self.0.now.set(now);
        let waker = self.0.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.now.get()
    }
    async fn wait_until(&self, deadline: Duration) {
        let mut yielded = false;
        poll_fn(|cx| {
            if yielded && self.now() >= deadline {
                return Poll::Ready(());
            }
            yielded = true;
            *self.0.waker.borrow_mut() = Some(cx.waker().clone());
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await;
    }
}
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
    let mut cpu = support::create_cpu(input.clone(), config).await.unwrap();
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
    cpu.install_hooks([Hook::<Rc<Input>>::new(0x200, async |call| {
        let memory = &mut call.memory;
        let input = &call.state;
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
        real_mode_interrupt::return_from_interrupt(call)
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
        let run = join(runner.run(&mut cpu), timer);
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
        // The quantum ends and the runner awaits its host yield.
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
fn deadlines_progress_during_borrowed_services_without_replenishing_the_cpu_budget() {
    pollster::block_on(check_timing());
}
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn deadlines_progress_during_borrowed_services_without_replenishing_the_cpu_budget() {
    check_timing().await;
}
