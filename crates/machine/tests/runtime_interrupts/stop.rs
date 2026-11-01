use super::*;
use relocade_machine::{Device, StopHandle};
use std::{
    future::{pending, Future},
    pin::pin,
    task::{Context, Waker},
};

#[derive(Default)]
struct StackDevice {
    stop: Option<StopHandle>,
    writes: usize,
}

impl Device for StackDevice {
    fn write_mmio(&mut self, _: u32, _: u8, _: u64) {
        self.writes += 1;
        self.stop.as_ref().unwrap().stop();
    }
}

async fn stop_during_entry() {
    for waiting in [false, true] {
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
        stack.borrow_mut().stop = Some(runner.stop_handle());
        let cpu = runner.cpu_state();
        cpu.set_eflags(0x202);
        cpu.set_sp(0x8000);
        if waiting {
            runner
                .install_hooks([Hook::new(0x100, async || pending::<()>().await)
                    .with_continuations(0x102, 0x101)])
                .await
                .unwrap();
            let mut run = pin!(runner.run(10));
            assert!(run
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
        }
        irq.borrow_mut().raise();
        assert_eq!(runner.run(10).await, ExecutionExit::Host(HostExit::Return));
        assert!(stack.borrow().writes > 0);
        assert_eq!(irq.borrow().acknowledged, 1);
        assert_eq!(cpu.instruction_count(), 0);
        assert_eq!(cpu.ax(), 0, "stop must precede ISR execution");
        assert_eq!(cpu.eip(), 0x300);
    }
}

test_backends!(
    stop_requested_during_irq_entry_precedes_isr_execution,
    stop_during_entry
);
