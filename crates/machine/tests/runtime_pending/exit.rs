use super::*;
use relocade_machine::{Cpu, Device, DeviceConnection, ExitHandle, IoBus, Memory};

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

async fn exit_lifetime() {
    let mut runner = cpu(IoBus::default()).await;
    let exit = runner.exit_handle();
    exit.request();
    drop(runner.run(1));
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runner.cpu_state().eip(), 0x400);
    assert_eq!(runner.cpu_state().instruction_count(), 0);
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.cpu_state().ax(), 1, "the exit request was consumed");

    runner
        .install_hooks([Hook::new(0x400, {
            let cpu = runner.cpu_state();
            let exit = exit.clone();
            async move || {
                exit.request();
                cpu.set_eip(0x401); // The current poll still completes normally.
            }
        })])
        .await
        .unwrap();
    runner.cpu_state().set_eip(0x400);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(runner.cpu_state().eip(), 0x401);
    assert_eq!(runner.cpu_state().ax(), 1, "the guest has not resumed");
    assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.cpu_state().ax(), 2);
}

async fn exit_pending_waiter() {
    let mut runner = cpu(IoBus::default()).await;
    let exit = runner.exit_handle();
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
    {
        let mut run = pin!(runner.run(1));
        assert!(run
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        exit.request();
        // Drop the waiter before it observes the request; retain the call.
    }
    assert_eq!(input.dropped.get(), 0);
    cpu.set_eflags(0x603);
    input.send(&[42]);
    assert_eq!(runner.run(0).await, ExecutionExit::BudgetExhausted);
    assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
    assert_eq!(
        input.completed.get(),
        0,
        "ready input must not resume an abandoned call"
    );
    assert_eq!(input.dropped.get(), 1);
    assert_eq!(cpu.ax(), 0);
    assert_eq!(cpu.eip(), 0x400);
    assert_eq!(cpu.eflags(), 0x603, "abandonment must not restore saved IF");
}

struct ExitOnDrop {
    exit: ExitHandle,
    dropped: Rc<Cell<u32>>,
    pending: bool,
}

impl Future for ExitOnDrop {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.pending {
            self.exit.request();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for ExitOnDrop {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
        self.exit.request();
    }
}

async fn exit_during_poll() {
    for pending in [false, true] {
        let mut runner = cpu(IoBus::default()).await;
        let dropped = Rc::new(Cell::new(0));
        runner
            .install_hooks([Hook::new(0x400, {
                let exit = runner.exit_handle();
                let dropped = dropped.clone();
                move || ExitOnDrop {
                    exit: exit.clone(),
                    dropped: dropped.clone(),
                    pending,
                }
            })])
            .await
            .unwrap();
        assert_eq!(runner.run(1).await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(dropped.get(), 1);
        assert_eq!(runner.cpu_state().instruction_count(), 0);
        runner.cpu_state().set_eip(0x401);
        assert_eq!(runner.run(1).await, ExecutionExit::BudgetExhausted);
        assert_eq!(
            runner.cpu_state().ax(),
            1,
            "destruction left no stale request"
        );
    }
}

#[derive(Default)]
struct ExitDevice(Option<ExitHandle>);
impl Device for ExitDevice {
    fn write_port(&mut self, _: u16, _: u8, _: u32) {
        self.0.as_ref().unwrap().request();
    }
}

async fn exit_during_guest() {
    let device = Rc::new(RefCell::new(ExitDevice::default()));
    let io = IoBus::new([DeviceConnection {
        device: device.clone(),
        mmio: Vec::new(),
        ports: std::iter::once(0x80..=0x80).collect(),
    }]);
    let mut runner = cpu(io).await;
    device.borrow_mut().0 = Some(runner.exit_handle());
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
        "observe the exit before entering another handler"
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

test_backends!(
    exit_requests_are_consumed_only_when_execution_returns,
    exit_lifetime
);
test_backends!(
    exit_survives_waiter_replacement_without_resuming_ready_work,
    exit_pending_waiter
);
test_backends!(
    exit_from_pending_or_destroyed_ready_future_ends_execution,
    exit_during_poll
);
test_backends!(device_exit_prevents_the_next_host_entry, exit_during_guest);
