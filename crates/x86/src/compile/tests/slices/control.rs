use super::*;

fn check_shared_budget(engine: Engine) {
    let code = [0xf3, 0xac, 0x40, 0x0f, 0x0b]; // REP LODSB; INC EAX; UD2
    let mut cpu = initial();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0x2000;
    let mut input = input(&code, cpu, &[4]);
    input.guest.push((0x2000, vec![7, 8, 9]));
    input.observe_budget = true;
    let observed = engine.observe(step(), &input, 3);
    let budgets: Vec<_> = observed
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Budget { remaining } => Some(*remaining),
            _ => None,
        })
        .collect();
    assert_eq!(budgets, [1, 0, 0]);
    let [.., Event::Return { outcome, snapshot }, Event::Budget { .. }] =
        observed.events.as_slice()
    else {
        panic!("expected a yield before UD2");
    };
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(0x0400_0000_0000_0000)])
    );
    let cpu = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (cpu.eip, cpu.instruction_count, cpu.registers.eax),
        (0x1003, 2, 10)
    );
}

#[test]
fn entries_share_work_across_a_completed_rep() {
    check_shared_budget(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_entries_share_work_across_a_completed_rep() {
    check_shared_budget(Engine::V8);
}

fn check_straight_line(engine: Engine, frontend: Frontend) {
    let code = [0x40, 0x40, 0x40, 0x40, 0x40]; // INC EAX
    let observations = run(engine, frontend, &code, 5, &input(&code, initial(), &[3]));
    let completed = match frontend {
        Frontend::Block => 5,
        Frontend::Interpreter => {
            assert_eq!(observations[0].0, SLICE_EXHAUSTED);
            3
        }
    };
    assert_eq!(observations[0].1.eip, 0x1000 + completed);
    assert_eq!(observations[0].1.instruction_count, u64::from(completed));
    assert_eq!(observations[0].1.registers.eax, completed);
}
test_frontends!(straight_line, check_straight_line);

fn check_taken_interrupt(engine: Engine) {
    let module = TestModule::new(
        &Compiler::new(crate::ExecutionProfile::Real16)
            .with_execution_budget()
            .compile_interpreter()
            .unwrap(),
    );
    let mut cpu = initial();
    cpu.segments = crate::Segments::real_mode();
    cpu.registers.esp = 0x3006;
    cpu.flags.bytes.of = 1;
    let mut input = Input::new(&cpu.to_bytes());
    input.budgets = vec![1]; // The second entry must see the first entry's debit.
    input.physical_pages = vec![
        (0, 0, true),
        (1, 0x1000, true),
        (2, 0x2000, true),
        (3, 0x3000, true),
    ];
    input.guest = vec![
        (0x1000, vec![0xce]),
        (16, vec![0, 0x20, 0, 0]),
        (0x2000, vec![0x40]),
    ];
    let result = engine.observe(&module, &input, 2);
    let returns: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Return { outcome, snapshot } => Some((
                outcome,
                CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap()),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(returns[0].1.instruction_count, 1);
    assert_eq!(returns[0].1.eip, 0x2000);
    assert_eq!(returns[0].1.registers.esp, 0x3000);
    assert_eq!(
        *returns[1].0,
        Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    assert_eq!(returns[0].1, returns[1].1);
}

#[test]
fn taken_interrupt_charges_its_slice() {
    check_taken_interrupt(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_taken_interrupt_charges_its_slice() {
    check_taken_interrupt(Engine::V8);
}

fn check_budget_handoff(engine: Engine) {
    let code = [0x40, 0xf3, 0xa5]; // INC EAX; REP MOVSD
    let mut cpu = initial();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0x2ffe;
    cpu.registers.edi = 0x4000;
    let mut input = input(&code, cpu, &[2]);
    input.machine.push((12, 0x6003u32.to_le_bytes().to_vec()));
    input.machine.push((16, 0x4003u32.to_le_bytes().to_vec()));
    input.guest.push((0x2ffe, vec![1, 2]));
    input.guest.push((0x6000, vec![3, 4]));
    let result = run(engine, Frontend::Block, &code, 2, &input);
    assert_eq!(result[0].0, SLICE_EXHAUSTED);
    let state = result[0].1;
    assert_eq!(
        (state.eip, state.instruction_count, state.registers.eax),
        (0x1001, 1, 1)
    );
    assert_eq!(
        (
            state.registers.ecx,
            state.registers.esi,
            state.registers.edi
        ),
        (1, 0x3002, 0x4004)
    );
}

#[test]
fn handoff_preserves_remaining_budget() {
    check_budget_handoff(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_handoff_preserves_remaining_budget() {
    check_budget_handoff(Engine::V8);
}

fn check_zero_budget(engine: Engine) {
    let cpu = initial();
    let mut input = Input::new(&cpu.to_bytes());
    input.budgets = vec![0];
    let observations = run(engine, Frontend::Interpreter, &[], 1, &input);
    assert_eq!(observations, vec![(SLICE_EXHAUSTED, cpu)]);
    // An exhausted direct decoder cycle must not decode the unsupported successor.
    let code = [0x40, 0x0f, 0xff, 0, 0];
    let observations = run(
        engine,
        Frontend::Interpreter,
        &code,
        1,
        &self::input(&code, cpu, &[1]),
    );
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    assert_eq!(observations[0].1.eip, 0x1001);
    // The exact fetch path also stops before the next unmapped page.
    let mut cpu = cpu;
    cpu.eip = 0x1fff;
    let mut input = input_with_last_byte(cpu);
    input.budgets = vec![1];
    let observations = run(engine, Frontend::Interpreter, &[], 1, &input);
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    assert_eq!(observations[0].1.eip, 0x2000);
    assert_eq!(observations[0].1.instruction_count, 1);
}

fn input_with_last_byte(cpu: CpuState) -> Input {
    let mut input = Input::new(&cpu.to_bytes());
    input.guest.push((0x1fff, vec![0x40]));
    input.machine.push((4, 0x1001u32.to_le_bytes().to_vec()));
    input
}

#[test]
fn exhausted_interpreter_does_not_fetch() {
    check_zero_budget(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_exhausted_interpreter_does_not_fetch() {
    check_zero_budget(Engine::V8);
}

fn check_block_debt(engine: Engine) {
    let code = [0x40; 5];
    let module = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 5)
            .unwrap(),
    );
    let mut cpu = initial();
    cpu.instruction_count = u64::MAX - 1;
    let mut input = input(&code, cpu, &[3]);
    input.observe_budget = true;
    let observation = engine.observe(&module, &input, 2);
    let balances: Vec<_> = observation
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Budget { remaining } => Some(*remaining),
            _ => None,
        })
        .collect();
    assert_eq!(balances, [-2, -2]);
    let returns: Vec<_> = observation
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Return { outcome, snapshot } => Some((outcome, snapshot)),
            _ => None,
        })
        .collect();
    assert_eq!(
        returns[1].0,
        &Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    assert_eq!(returns[0].1.cpu, returns[1].1.cpu);
    let cpu = CpuState::from_bytes(returns[0].1.cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (cpu.eip, cpu.instruction_count, cpu.registers.eax),
        (0x1005, 3, 5)
    );
}

#[test]
fn block_overshoot_is_reported_and_stops_the_next_entry() {
    check_block_debt(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_block_overshoot_is_reported_and_stops_the_next_entry() {
    check_block_debt(Engine::V8);
}

fn check_block_cap(engine: Engine) {
    let code = [0x40; 300];
    let observations = run(
        engine,
        Frontend::Block,
        &code,
        300,
        &input(&code, initial(), &[1]),
    );
    assert_eq!(
        (observations[0].1.eip, observations[0].1.instruction_count),
        (0x1100, 256)
    );
}
#[test]
fn budgeted_block_size_bounds_overshoot() {
    check_block_cap(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_budgeted_block_size_bounds_overshoot() {
    check_block_cap(Engine::V8);
}

#[test]
fn ordinary_block_budget_checks_do_not_grow_with_instruction_count() {
    for count in [1, 32, 256] {
        let block = Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &[0xb8, 42, 0, 0, 0].repeat(count), count as u32)
            .unwrap();
        let mut branches = 0;
        for payload in wasmparser::Parser::new(0).parse_all(&block.bytes) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                for instruction in body.get_operators_reader().unwrap() {
                    if matches!(instruction.unwrap(), wasmparser::Operator::If { .. }) {
                        branches += 1;
                    }
                }
            }
        }
        assert_eq!(branches, 1, "ordinary instructions add no budget branches");
    }
}
