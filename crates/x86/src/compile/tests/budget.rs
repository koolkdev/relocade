use crate::{
    support::{
        execution::{test_frontends, Frontend},
        step::{Argument, CallPatches, Event, Input, Outcome, Snapshot, TestModule},
    },
    Compiler, CpuState, ExecutionContext, ExecutionExit, SegmentProfile,
};

fn interpreter() -> &'static TestModule {
    TestModule::interpreter_run_with_profile(SegmentProfile::Flat32)
}

fn execute(
    frontend: Frontend,
    code: &[u8],
    instructions: u32,
    cpu: CpuState,
    target: u64,
) -> (u64, CpuState, Snapshot) {
    let mut input = Input::new(&cpu.to_bytes());
    input.guest.push((0x1000, code.to_vec()));
    input.guest.push((0x2000, vec![1, 2, 3, 4]));
    for page in 1..4u32 {
        input
            .machine
            .push((page * 4, ((page << 12) | 3).to_le_bytes().to_vec()));
    }
    input.patches_before_calls.push(CallPatches::cpu(vec![(
        std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
        target.to_le_bytes().to_vec(),
    )]));
    input.observe_guest = true;
    input.dispatch_return = 0;
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
    let [Argument::I64(word)] = values.as_slice() else {
        panic!("expected one exit word")
    };
    if ExecutionExit::from_bits(*word as u64) == Some(ExecutionExit::BudgetExhausted) {
        assert_eq!(observation.events.len(), 1, "yield must precede dispatch");
    }
    (
        *word as u64,
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
        for target in [41, 43, 44] {
            let mut cpu = initial(40);
            cpu.registers.eax = if taken { u32::MAX - 1 } else { 0 };
            let (word, after, _) = execute(frontend, &[0x40, 0x40, 0x74, 8], 3, cpu, target);
            assert_eq!(after.instruction_count, 43);
            assert_eq!(after.eip, if taken { 0x100c } else { 0x1004 });
            assert_eq!(
                ExecutionExit::from_bits(word),
                (target <= 43).then_some(ExecutionExit::BudgetExhausted)
            );
        }
    }
}
test_frontends!(conditional_branches, conditional_boundary);

fn long_block(frontend: Frontend) {
    let mut code = vec![0x90; 1024];
    code.extend([0xeb, 0]);
    let (word, cpu, _) = execute(frontend, &code, 1025, initial(0), 1);
    assert_eq!(
        ExecutionExit::from_bits(word),
        Some(ExecutionExit::BudgetExhausted)
    );
    assert_eq!(cpu.instruction_count, 1025);
    assert_eq!(cpu.eip, 0x1402);
}
test_frontends!(long_straight_line_block, long_block);

fn memory_continuations(frontend: Frontend) {
    let mut code = [0x66, 0x8b, 0x05, 0, 0x20, 0, 0].repeat(40);
    code.extend([0xeb, 0]);
    let (word, cpu, _) = execute(frontend, &code, 41, initial(0), 1);
    assert_eq!(
        ExecutionExit::from_bits(word),
        Some(ExecutionExit::BudgetExhausted)
    );
    assert_eq!(cpu.instruction_count, 41);
    assert_eq!(cpu.registers.eax, 0x0201);
    assert_eq!(cpu.eip, 0x111a);
}
test_frontends!(memory_decoder_continuations, memory_continuations);

fn fault_before_boundary(frontend: Frontend) {
    let (word, cpu, _) = execute(frontend, &[0x40, 0x40, 0xf7, 0xf1], 3, initial(0), 1);
    assert_eq!(
        ExecutionExit::from_bits(word),
        Some(ExecutionExit::Exception(crate::Exception::DivideError))
    );
    assert_eq!(cpu.instruction_count, 2);
    assert_eq!(cpu.eip, 0x1002);
    assert_eq!(cpu.registers.eax, 2);
}
test_frontends!(fault_keeps_its_exit, fault_before_boundary);

fn unsigned_target(frontend: Frontend) {
    for (count, target, exhausted) in [
        (0, u64::MAX, false),
        (0, 1 << 63, false),
        ((1 << 63) - 2, 1 << 63, true),
        ((1 << 63) + 1, 1, true),
        (u64::MAX - 4, u64::MAX, false),
        (u64::MAX - 3, u64::MAX, true),
    ] {
        let (word, cpu, _) = execute(frontend, &[0x90, 0x90, 0xeb, 0], 3, initial(count), target);
        assert_eq!(cpu.instruction_count, count + 3);
        assert_eq!(cpu.eip, 0x1004);
        assert_eq!(
            ExecutionExit::from_bits(word),
            exhausted.then_some(ExecutionExit::BudgetExhausted)
        );
    }
}
test_frontends!(unsigned_instruction_targets, unsigned_target);

fn rep_then_boundary(frontend: Frontend) {
    let mut cpu = initial(0);
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    cpu.registers.ecx = 4;
    let (word, cpu, snapshot) = execute(frontend, &[0xf3, 0xa4, 0x40, 0xeb, 0], 3, cpu, 1);
    assert_eq!(
        ExecutionExit::from_bits(word),
        Some(ExecutionExit::BudgetExhausted)
    );
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
test_frontends!(rep_retires_once_before_block_boundary, rep_then_boundary);

#[test]
fn step_checks_budget_after_one_instruction_before_dispatch() {
    for (code, next_eip) in [
        (&[0x40, 0x40, 0xeb, 0][..], 0x1001),
        (&[0x66, 0x40, 0x40, 0xeb, 0][..], 0x1002),
    ] {
        for target in [41u64, 42] {
            let mut input = Input::new(&initial(40).to_bytes());
            input.guest.push((0x1000, code.to_vec()));
            input.machine.push((4, 0x1003u32.to_le_bytes().to_vec()));
            input.patches_before_calls.push(CallPatches::cpu(vec![(
                std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
                target.to_le_bytes().to_vec(),
            )]));
            input.dispatch_return = 0;
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
            let [Argument::I64(word)] = values.as_slice() else {
                panic!("expected exit word")
            };
            assert_eq!(
                ExecutionExit::from_bits(*word as u64),
                (target == 41).then_some(ExecutionExit::BudgetExhausted)
            );
            assert_eq!(observation.events.len(), if target == 41 { 1 } else { 2 });
        }
    }
}
