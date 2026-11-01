use super::*;
use relocade_machine::{Cpu, Device, DeviceConnection, IoBus, Memory, StopHandle};

async fn cpu(io: IoBus) -> Cpu {
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
    let memory = Memory::new(config.machine).unwrap();
    let mut runner = Cpu::new(memory, config.cpu, io).await.unwrap();
    runner
        .memory()
        .view()
        .write(0x401, &[0x40, 0xeb, 0xfe])
        .unwrap();
    runner
        .install_hooks([Hook::new(0x400, {
            let cpu = runner.cpu_state();
            async move || cpu.set_eip(0x401)
        })])
        .await
        .unwrap();
    runner
}

async fn stop_lifetime() {
    let mut runner = cpu(IoBus::default()).await;
    let stop = runner.stop_handle();
    stop.stop();
    drop(runner.run(1));
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(
        runner.cpu_state().ax(),
        1,
        "idle requests do not reach a later run"
    );

    runner
        .install_hooks([Hook::new(0x400, {
            let cpu = runner.cpu_state();
            let stop = stop.clone();
            async move || {
                stop.stop();
                cpu.set_eip(0x401); // The current poll still completes normally.
            }
        })])
        .await
        .unwrap();
    runner.cpu_state().set_eip(0x400);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runner.cpu_state().eip(), 0x401);
    assert_eq!(runner.cpu_state().ax(), 1, "the guest has not resumed");
    stop.stop();
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.cpu_state().ax(), 2);
}

async fn stop_pending_waiter() {
    let mut runner = cpu(IoBus::default()).await;
    let stop = runner.stop_handle();
    let input = Rc::new(State::default());
    let cpu = runner.cpu_state();
    cpu.set_eflags(2);
    runner
        .install_hooks([Hook::new(0x400, {
            let input = input.clone();
            let cpu = cpu.clone();
            async move || {
                let _operation = Operation(input.clone());
                let saved_if = cpu.eflags() & 0x200;
                cpu.set_eflags(cpu.eflags() | 0x200);
                let value = input.read().await;
                cpu.set_eflags((cpu.eflags() & !0x200) | saved_if);
                cpu.set_ax(value);
                cpu.set_eip(0x401);
                input.completed.set(input.completed.get() + 1);
            }
        })])
        .await
        .unwrap();
    let old = Arc::new(WakeCount::default());
    let new = Arc::new(WakeCount::default());
    let old_waker = Waker::from(old.clone());
    let new_waker = Waker::from(new.clone());
    {
        let mut run = pin!(runner.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(&old_waker))
            .is_pending());
        stop.stop();
        assert_eq!(old.0.load(Ordering::Relaxed), 1);
        // Drop the waiter before it observes the request; retain the call.
    }
    stop.stop();
    assert_eq!(old.0.load(Ordering::Relaxed), 1);
    assert_eq!(input.dropped.get(), 0);
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    {
        let mut run = pin!(runner.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(&new_waker))
            .is_pending());
        cpu.set_eflags(0x603);
        input.send(&[42]);
        new.0.store(0, Ordering::Relaxed);
        stop.stop();
        stop.stop();
        assert_eq!(old.0.load(Ordering::Relaxed), 1);
        assert_eq!(new.0.load(Ordering::Relaxed), 1);
        assert_eq!(
            run.as_mut().poll(&mut Context::from_waker(&new_waker)),
            Poll::Ready(ExecutionExit::Host(HostExit::Return))
        );
    }
    stop.stop();
    assert_eq!(new.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        input.completed.get(),
        0,
        "ready input must not resume a stopped call"
    );
    assert_eq!(input.dropped.get(), 1);
    assert_eq!(cpu.ax(), 0);
    assert_eq!(cpu.eip(), 0x400);
    assert_eq!(cpu.eflags(), 0x603, "abandonment must not restore saved IF");
}

struct StopOnDrop {
    stop: StopHandle,
    dropped: Rc<Cell<u32>>,
    pending: bool,
}

impl Future for StopOnDrop {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.pending {
            self.stop.stop();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
        self.stop.stop();
    }
}

async fn stop_during_poll() {
    for pending in [false, true] {
        let mut runner = cpu(IoBus::default()).await;
        let dropped = Rc::new(Cell::new(0));
        runner
            .install_hooks([Hook::new(0x400, {
                let stop = runner.stop_handle();
                let dropped = dropped.clone();
                move || StopOnDrop {
                    stop: stop.clone(),
                    dropped: dropped.clone(),
                    pending,
                }
            })])
            .await
            .unwrap();
        assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(dropped.get(), 1);
        assert_eq!(runner.cpu_state().instruction_count(), 0);
    }
}

#[derive(Default)]
struct StopDevice(Option<StopHandle>);
impl Device for StopDevice {
    fn write_port(&mut self, _: u16, _: u8, _: u32) {
        self.0.as_ref().unwrap().stop();
    }
}

async fn stop_during_guest() {
    let device = Rc::new(RefCell::new(StopDevice::default()));
    let io = IoBus::new([DeviceConnection {
        device: device.clone(),
        mmio: Vec::new(),
        ports: std::iter::once(0x80..=0x80).collect(),
    }]);
    let mut runner = cpu(io).await;
    device.borrow_mut().0 = Some(runner.stop_handle());
    // OUT 80h,AL; JMP 0500 (host entry).
    runner
        .memory()
        .view()
        .write(0x100, &[0xe6, 0x80, 0xe9, 0xfb, 0x03])
        .unwrap();
    runner.cpu_state().set_eip(0x100);
    let started = Rc::new(Cell::new(false));
    runner
        .install_hooks([Hook::new(0x500, {
            let started = started.clone();
            let cpu = runner.cpu_state();
            async move || {
                started.set(true);
                cpu.set_eip(0x401);
            }
        })])
        .await
        .unwrap();
    assert_eq!(runner.run(20).await, ExecutionExit::Host(HostExit::Return));
    assert!(
        !started.get(),
        "observe the stop before entering another handler"
    );
}

macro_rules! test_backends {
    ($name:ident, $check:ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            pollster::block_on($check());
        }
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            $check().await;
        }
    };
}

test_backends!(stop_requests_belong_to_the_active_run, stop_lifetime);
test_backends!(
    stop_wakes_current_waiter_without_resuming_cancelled_work,
    stop_pending_waiter
);
test_backends!(
    stop_from_pending_or_destroyed_ready_future_ends_the_run,
    stop_during_poll
);
test_backends!(device_stop_prevents_the_next_host_entry, stop_during_guest);
