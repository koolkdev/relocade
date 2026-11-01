use super::*;
use relocade_machine::{Device, ExitHandle};
use std::task::{Context, Poll};

struct ExitPollingDevice {
    exit: Option<ExitHandle>,
    ready: bool,
    armed: bool,
}

impl Device for ExitPollingDevice {
    fn poll_interrupt(&mut self, _: &mut Context<'_>) -> Poll<u8> {
        if !self.armed {
            return Poll::Pending;
        }
        self.exit.as_ref().unwrap().request();
        if self.ready {
            Poll::Ready(0x60)
        } else {
            Poll::Pending
        }
    }
}

async fn exit_during_poll() {
    for pending_call in [false, true] {
        for ready in [false, true] {
            let device = Rc::new(RefCell::new(ExitPollingDevice {
                exit: None,
                ready,
                armed: !pending_call,
            }));
            let mut runner = cpu(&[0xeb, 0xfe], 0x202, &device).await;
            device.borrow_mut().exit = Some(runner.exit_handle());
            if pending_call {
                runner
                    .install([Hook::new(0x100, {
                        let device = device.clone();
                        async move || {
                            // Device exit abandons this call before guest resumption.
                            device.borrow_mut().armed = true;
                            std::future::pending::<()>().await;
                        }
                    })])
                    .await
                    .unwrap();
            }
            let before = runner.cpu_state().snapshot();
            assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(
                runner.cpu_state().snapshot(),
                before,
                "exit precedes IRQ entry and guest dispatch"
            );
        }
    }
}

#[derive(Default)]
struct StackDevice {
    exit: Option<ExitHandle>,
    writes: usize,
}

impl Device for StackDevice {
    fn write_mmio(&mut self, _: u32, _: u8, _: u64) {
        self.writes += 1;
        self.exit.as_ref().unwrap().request();
    }
}

async fn exit_during_entry() {
    for pending_call in [false, true] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let stack = Rc::new(RefCell::new(StackDevice::default()));
        let config = support::configuration(
            Compiler::new(ExecutionProfile::Real16).target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..CpuState::default()
            },
            0x7000,
            PhysicalMemoryMap::new([
                (0..=0x6fff, PhysicalMapping::Ram { backing_offset: 0 }),
                (0x7000..=0x7fff, PhysicalMapping::Mmio),
            ])
            .unwrap(),
        )
        .unwrap();
        let memory = Memory::new(config.machine).unwrap();
        memory.view().write(0x180, &[0, 3, 0, 0]).unwrap();
        memory.view().write(0x300, &[0x40, 0xeb, 0xfe]).unwrap(); // INC AX; JMP to self.
        let mut runner = Cpu::new(
            memory,
            config.cpu,
            IoBus::new([
                DeviceConnection {
                    device: irq.clone(),
                    mmio: vec![],
                    ports: vec![],
                },
                DeviceConnection {
                    device: stack.clone(),
                    mmio: std::iter::once(0x7000..=0x7fff).collect(),
                    ports: vec![],
                },
            ]),
        )
        .await
        .unwrap();
        stack.borrow_mut().exit = Some(runner.exit_handle());
        let cpu = runner.cpu_state();
        cpu.set_eflags(0x202);
        cpu.set_sp(0x8000);
        if pending_call {
            use std::{future::Future, pin::pin, task::Waker};
            runner
                .install([Hook::new(0x100, {
                    let cpu = cpu.clone();
                    async move || {
                        relocade_machine::real_mode_interrupt::with_interrupts_enabled(
                            &cpu,
                            std::future::pending::<()>(),
                        )
                        .await;
                    }
                })])
                .await
                .unwrap();
            assert!(pin!(runner.run(10))
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
        }
        irq.borrow_mut().raise();
        assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
        assert!(stack.borrow().writes > 0);
        assert_eq!(irq.borrow().acknowledged, 1);
        assert_eq!(cpu.instruction_count(), 0);
        assert_eq!(cpu.ax(), 0, "exit must precede ISR execution");
        assert_eq!(cpu.eip(), 0x300);
        assert!(
            !cpu.interrupts_enabled(),
            "abandonment preserves the IRQ entry IF"
        );
    }
}

test_backends!(
    exit_requested_during_irq_entry_precedes_isr_execution,
    exit_during_entry
);
test_backends!(
    exit_requested_during_irq_poll_precedes_entry,
    exit_during_poll
);
