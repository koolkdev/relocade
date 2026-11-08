use crate::{
    support::{
        execution::{test_frontends, Frontend},
        step::{decode, CallPatches, Event, Input, Outcome, Snapshot, TestModule},
    },
    Compiler, CpuState, ExecutionContext, ExecutionExit, HostExit, SegmentProfile,
};

fn interpreter() -> &'static TestModule {
    TestModule::interpreter_run_with_profile(SegmentProfile::Flat32)
}

fn execute(
    frontend: Frontend,
    code: &[u8],
    instructions: u32,
    cpu: CpuState,
    requested: bool,
    target: u64,
) -> (ExecutionExit, CpuState, Snapshot) {
    let mut input = Input::new(&cpu.to_bytes());
    input.guest.push((0x1000, code.to_vec()));
    input.guest.push((0x2000, vec![1, 2, 3, 4]));
    for page in 1..4u32 {
        input
            .machine
            .push((page * 4, ((page << 12) | 3).to_le_bytes().to_vec()));
    }
    input.patches_before_calls.push(CallPatches::cpu(vec![
        (
            std::mem::offset_of!(ExecutionContext, interrupt_requested) as u32,
            vec![u8::from(requested)],
        ),
        (
            std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
            target.to_le_bytes().to_vec(),
        ),
    ]));
    input.observe_guest = true;
    input.dispatch_return = ExecutionExit::Host(HostExit::Return);
    let block;
    let module = match frontend {
        Frontend::Block => {
            block = TestModule::new(
                &Compiler::new(SegmentProfile::Flat32)
                    .compile_block(cpu.eip, code, instructions)
                    .unwrap(),
            );
            &block
        }
        Frontend::Interpreter => interpreter(),
    };
    let observation = module.observe(&input, 1);
    assert!(observation.machine_unchanged);
    let Some(Event::Return {
        outcome: Outcome::Returned(values),
        snapshot,
    }) = observation.events.last()
    else {
        panic!("unexpected execution: {:?}", observation.events);
    };
    let exit = decode::<ExecutionExit>(values);
    if matches!(
        exit,
        ExecutionExit::Interrupted | ExecutionExit::BudgetExhausted
    ) {
        assert_eq!(observation.events.len(), 1, "yield must precede dispatch");
    }
    (
        exit,
        CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap()),
        snapshot.clone(),
    )
}

fn initial(count: u64) -> CpuState {
    CpuState {
        eip: 0x1000,
        instruction_count: count,
        ..CpuState::default()
    }
}

fn conditional_boundary(frontend: Frontend) {
    for taken in [false, true] {
        for requested in [false, true] {
            let mut cpu = initial(40);
            cpu.registers.eax = if taken { u32::MAX - 1 } else { 0 };
            let (exit, after, _) = execute(frontend, &[0x40, 0x40, 0x74, 8], 3, cpu, requested, 0);
            assert_eq!(after.instruction_count, 43);
            assert_eq!(after.eip, if taken { 0x100c } else { 0x1004 });
            assert_eq!(
                exit,
                if requested {
                    ExecutionExit::Interrupted
                } else {
                    ExecutionExit::Host(HostExit::Return)
                }
            );
        }
    }
}
test_frontends!(conditional_branches, conditional_boundary);

fn long_block(frontend: Frontend) {
    let mut code = vec![0x90; 1024];
    code.extend([0xeb, 0]);
    let (exit, cpu, _) = execute(frontend, &code, 1025, initial(0), true, 0);
    assert_eq!(exit, ExecutionExit::Interrupted);
    assert_eq!(cpu.instruction_count, 1025);
    assert_eq!(cpu.eip, 0x1402);
}
test_frontends!(long_straight_line_block, long_block);

fn memory_continuations(frontend: Frontend) {
    let mut code = [0x66, 0x8b, 0x05, 0, 0x20, 0, 0].repeat(40);
    code.extend([0xeb, 0]);
    let (exit, cpu, _) = execute(frontend, &code, 41, initial(0), true, 0);
    assert_eq!(exit, ExecutionExit::Interrupted);
    assert_eq!(cpu.instruction_count, 41);
    assert_eq!(cpu.registers.eax, 0x0201);
    assert_eq!(cpu.eip, 0x111a);
}
test_frontends!(memory_decoder_continuations, memory_continuations);

fn fault_before_boundary(frontend: Frontend) {
    let (exit, cpu, _) = execute(frontend, &[0x40, 0x40, 0xf7, 0xf1], 3, initial(0), true, 1);
    assert_eq!(
        exit,
        ExecutionExit::Exception(crate::Exception::DivideError)
    );
    assert_eq!(cpu.instruction_count, 2);
    assert_eq!(cpu.eip, 0x1002);
    assert_eq!(cpu.registers.eax, 2);
}
test_frontends!(fault_keeps_its_exit, fault_before_boundary);

fn count_wraps(frontend: Frontend) {
    let (exit, cpu, _) = execute(
        frontend,
        &[0x90, 0x90, 0xeb, 0],
        3,
        initial(u64::MAX - 1),
        false,
        0,
    );
    assert_eq!(exit, ExecutionExit::Host(HostExit::Return));
    assert_eq!(cpu.instruction_count, 1);
    assert_eq!(cpu.eip, 0x1004);
}
test_frontends!(retirement_wraps_without_stopping_execution, count_wraps);

fn rep_then_boundary(frontend: Frontend) {
    for (requested, target, expected) in [
        (true, 0, ExecutionExit::Interrupted),
        (false, 2, ExecutionExit::BudgetExhausted),
    ] {
        let mut cpu = initial(0);
        cpu.registers.esi = 0x2000;
        cpu.registers.edi = 0x3000;
        cpu.registers.ecx = 4;
        let (exit, cpu, snapshot) = execute(
            frontend,
            &[0xf3, 0xa4, 0x40, 0xeb, 0],
            3,
            cpu,
            requested,
            target,
        );
        assert_eq!(exit, expected);
        assert_eq!(cpu.instruction_count, 3);
        assert_eq!(cpu.registers.ecx, 0);
        assert_eq!(cpu.registers.esi, 0x2004);
        assert_eq!(cpu.registers.edi, 0x3004);
        assert_eq!(cpu.registers.eax, 1);
        assert_eq!(cpu.eip, 0x1005);
        assert_eq!(
            snapshot.guest.unwrap(),
            [(0x3000, 1), (0x3001, 2), (0x3002, 3), (0x3003, 4)]
        );
    }
}
test_frontends!(rep_retires_once_before_block_boundary, rep_then_boundary);

fn instruction_targets(frontend: Frontend) {
    for (start, target, count, expected) in [
        (40, 0, 43, ExecutionExit::Host(HostExit::Return)),
        (40, 42, 43, ExecutionExit::BudgetExhausted),
        (40, 44, 43, ExecutionExit::Host(HostExit::Return)),
        // A target across unsigned wrap is still in the future.
        (
            u64::MAX - 4,
            1,
            u64::MAX - 1,
            ExecutionExit::Host(HostExit::Return),
        ),
        // Reaching a target must survive both counter wrap and the signed-bit boundary.
        (u64::MAX - 1, u64::MAX, 1, ExecutionExit::BudgetExhausted),
        (
            0x7fff_ffff_ffff_fffd,
            0x7fff_ffff_ffff_ffff,
            0x8000_0000_0000_0000,
            ExecutionExit::BudgetExhausted,
        ),
    ] {
        // INC EAX; INC EAX; JMP next. The checkpoint includes all three instructions.
        let (exit, cpu, _) = execute(
            frontend,
            &[0x40, 0x40, 0xeb, 0],
            3,
            initial(start),
            false,
            target,
        );
        assert_eq!(exit, expected);
        assert_eq!(
            (cpu.instruction_count, cpu.eip, cpu.registers.eax),
            (count, 0x1004, 2)
        );
    }
}
test_frontends!(
    instruction_targets_use_signed_wrapping_distance,
    instruction_targets
);

#[test]
fn step_checks_controls_after_one_instruction_without_dispatch() {
    for (code, next_eip) in [
        (&[0x40, 0x40, 0xeb, 0][..], 0x1001),
        (&[0x66, 0x40, 0x40, 0xeb, 0][..], 0x1002),
    ] {
        for (requested, target, expected) in [
            (false, 0u64, ExecutionExit::StepComplete),
            (false, 41, ExecutionExit::BudgetExhausted),
            (true, 41, ExecutionExit::Interrupted),
        ] {
            let mut input = Input::new(&initial(40).to_bytes());
            input.guest.push((0x1000, code.to_vec()));
            input.machine.push((4, 0x1003u32.to_le_bytes().to_vec()));
            input.patches_before_calls.push(CallPatches::cpu(vec![
                (
                    std::mem::offset_of!(ExecutionContext, interrupt_requested) as u32,
                    vec![u8::from(requested)],
                ),
                (
                    std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
                    target.to_le_bytes().to_vec(),
                ),
            ]));
            input.dispatch_return = ExecutionExit::Host(HostExit::Return);
            let observation = TestModule::interpreter().observe(&input, 1);
            let Some(Event::Return {
                outcome: Outcome::Returned(values),
                snapshot,
            }) = observation.events.last()
            else {
                panic!("unexpected execution: {:?}", observation.events);
            };
            let cpu = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
            assert_eq!(cpu.instruction_count, 41);
            assert_eq!(cpu.eip, next_eip);
            assert_eq!(cpu.registers.eax, 1);

            assert_eq!(decode::<ExecutionExit>(values), expected);
            assert_eq!(observation.events.len(), 1);
        }
    }
}
