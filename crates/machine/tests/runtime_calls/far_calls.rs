use super::{cpu, support, Irq};
use relocade_machine::{real_mode_interrupt::Interrupts, Cpu, FarPointer, IoBus, Memory};
use relocade_x86::{
    Compiler, CpuState, Exception, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap, Segment, SegmentRegister,
};
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::Poll,
};

const TARGET: FarPointer = FarPointer {
    segment: 0x10,
    offset: 0x300,
};

async fn registered_procedure() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    let memory = runner.memory();
    memory.view().write_u32(0x61 * 4, 0x600).unwrap();
    // A guest interrupt called by the Rust far procedure pauses before IRET.
    memory.view().write(0x600, &[0x43, 0xeb, 0, 0xcf]).unwrap();
    let mut entries = Interrupts::new(&mut runner);
    entries.register_interrupt(
        0x21,
        FarPointer {
            segment: 0,
            offset: 0x200,
        },
        async |call| {
            call.cpu.set_eflags(0x643);
            call.call_far(TARGET, &[0x1122, 0x3344]).await;
            assert_eq!(call.cpu.sp(), 0x7ffa);
            assert_eq!(call.cpu.eflags(), 0x643);
            assert_eq!(call.cpu.ax(), 7);
        },
    );
    entries.register_far_procedure(TARGET, async |call| {
        let stack = call.cpu.segment(Segment::Ss);
        let mut frame = [0; 8];
        call.memory
            .read_segment(&stack, u32::from(call.cpu.sp()), &mut frame)
            .unwrap();
        assert_eq!(frame, [1, 2, 0, 0, 0x44, 0x33, 0x22, 0x11]);
        assert_eq!(call.cpu.eflags(), 0x643);
        call.call_interrupt(0x61).await;
        call.cpu.set_ax(7);
    });
    entries.install().await.unwrap();
    assert_eq!(
        support::run_until(&mut runner, 0x603).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(
        support::run_until(&mut runner, 0x102).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runner.cpu_state().sp(), 0x8000);
    assert_eq!(runner.cpu_state().ax(), 7);
}

async fn frame_and_fault_recovery() {
    for arguments in [
        &[][..],
        &[0x1122, 0x3344, 0x5566, 0x7788, 0x99aa, 0xbbcc][..],
    ] {
        let irq = Rc::new(RefCell::new(Irq::default()));
        let mut runner = cpu(&irq, 0x300).await;
        let state = runner.cpu_state();
        let memory = runner.memory();
        memory.view().write(0xffe0, &[0xa5; 32]).unwrap();
        // Record entry flags, change a result register and return with RETF.
        memory
            .view()
            .write(TARGET.address(), &[0x9c, 0x5f, 0xbb, 0x34, 0x12, 0xcb])
            .unwrap();
        let mut interrupts = Interrupts::new(&mut runner);
        interrupts.register_interrupt(
            0x21,
            FarPointer {
                segment: 0,
                offset: 0x200,
            },
            async move |call| {
                let instructions = call.cpu.instruction_count();
                call.cpu.set_sp(2); // A four-byte far-call frame crosses the stack limit.
                call.cpu.set_eflags(0x443); // DF and arithmetic flags, with IF clear.
                call.call_far(TARGET, arguments).await;
                assert_eq!(call.cpu.instruction_count(), instructions + 4);
                assert_eq!(call.cpu.sp(), 0x7ffa);
                assert_eq!(call.cpu.segment(Segment::Cs).selector, 0);
                assert_eq!(call.cpu.eip(), 0x201);
                assert_eq!(call.cpu.eflags(), 0x443);
                assert_eq!(call.cpu.bx(), 0x1234);
            },
        );
        interrupts.install().await.unwrap();
        assert_eq!(
            runner.run().await,
            ExecutionExit::Exception(Exception::StackFault { error_code: 0 })
        );
        assert_eq!(state.sp(), 2);
        let mut untouched = [0; 32];
        memory.view().read(0xffe0, &mut untouched).unwrap();
        assert_eq!(
            untouched, [0xa5; 32],
            "fault must not partially write arguments"
        );
        assert_eq!(state.eip(), 0x201);
        assert_eq!(state.instruction_count(), 1, "only the outer INT retired");
        state.set_sp(0x7ffa);
        assert_eq!(
            support::run_until(&mut runner, 0x102).await,
            ExecutionExit::Interrupted
        );
        let frame = 0x7ffa - 4 - arguments.len() as u32 * 2;
        assert_eq!(memory.view().read_u16(frame).unwrap(), 0x201);
        assert_eq!(memory.view().read_u16(frame + 2).unwrap(), 0);
        for (index, word) in arguments.iter().rev().enumerate() {
            assert_eq!(
                memory
                    .view()
                    .read_u16(frame + 4 + index as u32 * 2)
                    .unwrap(),
                *word
            );
        }
        assert_eq!(state.di(), 0x443);
        assert_eq!(state.sp(), 0x8000);
        assert_eq!(state.eflags(), 0x243);
    }
}

async fn canceled_request() {
    let irq = Rc::new(RefCell::new(Irq::default()));
    let mut runner = cpu(&irq, 0x300).await;
    runner
        .memory()
        .view()
        .write(TARGET.address(), &[0x0f, 0x0b])
        .unwrap();
    runner.memory().view().write(0x7ff6, &[0xa5; 4]).unwrap();
    let mut interrupts = Interrupts::new(&mut runner);
    interrupts.register_interrupt(
        0x21,
        FarPointer {
            segment: 0,
            offset: 0x200,
        },
        async |call| {
            {
                let mut invocation = pin!(call.call_far(TARGET, &[0x1234, 0x5678]));
                poll_fn(|cx| {
                    assert!(invocation.as_mut().poll(cx).is_pending());
                    Poll::Ready(())
                })
                .await;
            }
            let mut untouched = [0; 4];
            call.memory.read(0x7ff6, &mut untouched).unwrap();
            assert_eq!(
                untouched, [0xa5; 4],
                "canceled calls must not write arguments"
            );
            call.call_interrupt(0x60).await;
            call.cpu.set_ax(7);
        },
    );
    interrupts.install().await.unwrap();
    assert_eq!(
        support::run_until(&mut runner, 0x102).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runner.cpu_state().ax(), 7);
    assert_eq!(runner.cpu_state().sp(), 0x8000);
    assert_eq!(runner.memory().view().read_u16(0x500).unwrap(), 1);
}

async fn mapped_arguments() {
    let mut state = CpuState {
        eip: 0x100,
        ..CpuState::real_mode()
    };
    state.segments.ss = SegmentRegister::real_mode(Segment::Ss, 0x100);
    state.registers.esp = 0x800a;
    // A nonzero SS places the arguments across two guest pages whose backing
    // order is reversed. Direct backing writes or ignoring SS cannot pass.
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        state,
        0x3000,
        PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x8000..=0x8fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x2000,
                },
            ),
            (
                0x9000..=0x9fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x1000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory
        .view()
        .write(0x100, &[0xcd, 0x21, 0xeb, 0xfe])
        .unwrap();
    memory
        .view()
        .write(
            TARGET.address(),
            &[
                0x89, 0xe5, // MOV BP,SP.
                0x8b, 0x46, 4, // MOV AX,[BP+4].
                0x8b, 0x5e, 6, // MOV BX,[BP+6].
                0x8b, 0x4e, 8,    // MOV CX,[BP+8].
                0xcb, // RETF.
            ],
        )
        .unwrap();
    let mut runner = Cpu::new(memory.clone(), config.cpu, IoBus::default())
        .await
        .unwrap();
    let mut interrupts = Interrupts::new(&mut runner);
    interrupts.register_interrupt(
        0x21,
        FarPointer {
            segment: 0,
            offset: 0x200,
        },
        async |call| {
            call.call_far(TARGET, &[0x1122, 0x3344, 0x5566]).await;
            assert_eq!(
                (call.cpu.ax(), call.cpu.bx(), call.cpu.cx()),
                (0x5566, 0x3344, 0x1122)
            );
            assert_eq!(call.cpu.sp(), 0x8004);
        },
    );
    interrupts.install().await.unwrap();
    assert_eq!(
        support::run_until(&mut runner, 0x102).await,
        ExecutionExit::Interrupted
    );
    assert_eq!(runner.cpu_state().sp(), 0x800a);
    let mut arguments = [0; 6];
    memory.view().read(0x8ffe, &mut arguments).unwrap();
    assert_eq!(arguments, [0x66, 0x55, 0x44, 0x33, 0x22, 0x11]);
}

test_backends!(
    registered_far_procedures_share_interrupt_resumption,
    registered_procedure
);
test_backends!(
    far_calls_preserve_flags_and_retry_stack_faults_before_retf,
    frame_and_fault_recovery
);
test_backends!(
    canceled_far_calls_can_be_replaced_by_interrupt_calls,
    canceled_request
);
test_backends!(
    far_calls_write_arguments_through_the_guest_stack_mapping,
    mapped_arguments
);
