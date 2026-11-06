use super::*;
use relocade_machine::FarPointer;
use relocade_x86::HostExit;

async fn address(cpu: &mut Cpu) -> FarPointer {
    cpu.cpu_state().set_ax(0x3400);
    assert_eq!(
        execution::run_until(cpu, 0x102).await,
        ExecutionExit::Interrupted
    );
    let state = cpu.cpu_state();
    FarPointer {
        segment: state.segment(Segment::Es).selector,
        offset: state.bx(),
    }
}

#[test]
fn indos_counts_suspended_and_nested_calls_until_they_return() {
    pollster::block_on(async {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let (mut cpu, dos) = session(&irq).await;
        let address = address(&mut cpu).await;
        let memory = cpu.memory();
        let count = || memory.view().read_u8(address.address()).unwrap();
        assert_eq!(count(), 0);

        // Reenter DOS from an IRQ and wait for a second, independent input call.
        memory
            .view()
            .write(0x300, &[0xb4, 7, 0xcd, 0x21, 0xcf])
            .unwrap();
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_ah(7);
        pending(cpu.run());
        assert_eq!(count(), 1, "dropping a run waiter retains the DOS call");
        irq.borrow_mut().raise();
        pending(cpu.run());
        assert_eq!(count(), 2);
        dos.stdio().input.clone().write_all(b"A").unwrap();
        pending(cpu.run());
        assert_eq!(count(), 1);
        dos.stdio().input.clone().write_all(b"B").unwrap();
        assert_eq!(
            execution::run_until(&mut cpu, 0x102).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(count(), 0);
        assert_eq!(cpu.cpu_state().al(), b'B');
    });
}

#[test]
fn indos_releases_abandoned_calls_and_keeps_its_address_across_loads() {
    pollster::block_on(async {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let (mut cpu, dos) = session(&irq).await;
        let pointer = address(&mut cpu).await;
        let memory = cpu.memory();
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_ah(7);
        pending(cpu.run());
        // Termination inside an IRQ abandons the pending outer input request.
        memory
            .view()
            .write(0x300, &[0xb8, 0, 0x4c, 0xcd, 0x21])
            .unwrap();
        irq.borrow_mut().raise();
        assert_eq!(cpu.run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(memory.view().read_u8(pointer.address()).unwrap(), 0);
        let psp = com::load(
            &mut cpu.context(),
            &[0xcd, 0x21, 0xeb, 0xfe],
            &Default::default(),
        )
        .unwrap();
        dos.start_process(psp);
        assert_eq!(address(&mut cpu).await, pointer);
        cpu.cpu_state().set_eip(0x100);
        cpu.cpu_state().set_ah(7);
        pending(cpu.run());
        assert_eq!(memory.view().read_u8(pointer.address()).unwrap(), 1);
        drop(cpu);
        assert_eq!(memory.view().read_u8(pointer.address()).unwrap(), 0);
    });
}
