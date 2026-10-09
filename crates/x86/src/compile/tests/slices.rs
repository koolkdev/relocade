use std::sync::OnceLock;

use crate::support::execution::Frontend;
use crate::{
    support::{
        execution::test_frontends,
        step::{Argument, Engine, Event, Input, Outcome, TestModule},
    },
    Compiler, CpuState, SegmentProfile, SLICE_EXHAUSTED,
};

fn interpreter() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| {
        TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .with_execution_budget()
                .compile_interpreter()
                .unwrap(),
        )
    })
}

fn step() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| {
        TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .with_execution_budget()
                .compile_interpreter_step()
                .unwrap(),
        )
    })
}

fn input(code: &[u8], cpu: CpuState, budgets: &[u32]) -> Input {
    let mut input = Input::new(&cpu.to_bytes());
    input.guest.push((0x1000, code.to_vec()));
    for page in 1..4u32 {
        input
            .machine
            .push((page * 4, ((page << 12) | 3).to_le_bytes().to_vec()));
    }
    input.budgets = budgets.to_vec();
    input.observe_guest = true;
    input
}

fn run(
    engine: Engine,
    frontend: Frontend,
    code: &[u8],
    limit: u32,
    input: &Input,
) -> Vec<(u64, CpuState)> {
    let block;
    let module = match frontend {
        Frontend::Block => {
            block = TestModule::new(
                &Compiler::new(SegmentProfile::Flat32)
                    .with_execution_budget()
                    .compile_block(0x1000, code, limit)
                    .unwrap(),
            )
            .with_interpreter(interpreter());
            &block
        }
        Frontend::Interpreter => interpreter(),
    };
    let observation = engine.observe(module, input, input.budgets.len());
    observation
        .events
        .into_iter()
        .filter_map(|event| match event {
            Event::Return {
                outcome: Outcome::Returned(values),
                snapshot,
            } => {
                let [Argument::I64(result)] = values.as_slice() else {
                    panic!("expected i64 return")
                };
                Some((
                    *result as u64,
                    CpuState::from_bytes(snapshot.cpu.try_into().unwrap()),
                ))
            }
            Event::Dispatch { .. } => None,
            other => panic!("unexpected event {other:?}"),
        })
        .collect()
}

fn initial() -> CpuState {
    CpuState {
        eip: 0x1000,
        ..CpuState::default()
    }
}

fn check_straight_line(engine: Engine, frontend: Frontend) {
    let code = [0x40, 0x40, 0x40, 0x40, 0x40]; // INC EAX
    let observations = run(engine, frontend, &code, 5, &input(&code, initial(), &[3]));
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    assert_eq!(observations[0].1.eip, 0x1003);
    assert_eq!(observations[0].1.instruction_count, 3);
    assert_eq!(observations[0].1.registers.eax, 3);
}
test_frontends!(straight_line, check_straight_line);

fn check_rep_load(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xac, 0xeb, 0]; // REP LODSB; JMP +0
    let mut cpu = initial();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0x2000;
    cpu.registers.eax = 0x12345600;
    let mut input = input(&code, cpu, &[1, 1, 1]);
    input.guest.push((0x2000, vec![7, 8, 9]));
    let observations = run(engine, frontend, &code, 1, &input);
    for (index, (exit, state)) in observations.iter().enumerate() {
        assert_eq!(state.registers.eax, 0x12345607 + index as u32);
        assert_eq!(state.registers.ecx, 2 - index as u32);
        assert_eq!(state.registers.esi, 0x2001 + index as u32);
        assert_eq!(state.eip, if index == 2 { 0x1002 } else { 0x1000 });
        assert_eq!(state.instruction_count, u32::from(index == 2));
        if index < 2 {
            assert_eq!(*exit, SLICE_EXHAUSTED);
        }
    }
}
test_frontends!(rep_load, check_rep_load);

fn check_backward_narrow_rep(engine: Engine, frontend: Frontend) {
    let code = [0x67, 0xf3, 0xac]; // Address-size override; REP LODSB
    let mut cpu = initial();
    cpu.registers.ecx = 0xaaaa_0002;
    cpu.registers.esi = 0xbbbb_2001;
    cpu.registers.eax = 0x12345600;
    cpu.flags.bytes.df = 1;
    let mut input = input(&code, cpu, &[1, 1]);
    input.guest.push((0x2000, vec![7, 8]));
    let observations = run(engine, frontend, &code, 1, &input);
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    for (index, (_, state)) in observations.iter().enumerate() {
        assert_eq!(state.registers.eax, 0x12345608 - index as u32);
        assert_eq!(state.registers.ecx, 0xaaaa_0001 - index as u32);
        assert_eq!(state.registers.esi, 0xbbbb_2000 - index as u32);
        assert_eq!(state.flags, cpu.flags);
        assert_eq!(state.instruction_count, index as u32);
        assert_eq!(state.eip, if index == 0 { 0x1000 } else { 0x1003 });
    }
}
test_frontends!(backward_narrow_rep, check_backward_narrow_rep);

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

fn check_rep_copy(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4];
    let mut cpu = initial();
    cpu.registers.ecx = u32::MAX;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    let mut input = input(&code, cpu, &[2]);
    input.guest.push((0x2000, vec![7, 8, 9]));
    let block = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    );
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter(),
    };
    let observation = engine.observe(module, &input, 1);
    let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
        panic!("yield without dispatch")
    };
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!(state.eip, 0x1000);
    assert_eq!(state.instruction_count, 0);
    assert_eq!(state.registers.ecx, u32::MAX - 2);
    assert_eq!(state.registers.esi, 0x2002);
    assert_eq!(state.registers.edi, 0x3002);
    assert_eq!(snapshot.guest, Some(vec![(0x3000, 7), (0x3001, 8)]));
}
test_frontends!(rep_copy, check_rep_copy);

fn check_rep_chunk_fallback(engine: Engine, frontend: Frontend) {
    // The first dword straddles nonconsecutive backing pages. One checked
    // element crosses the boundary; the remaining three fit a direct chunk.
    let code = [0xf3, 0xa5];
    let mut cpu = initial();
    cpu.registers.ecx = 4;
    cpu.registers.esi = 0x2ffe;
    cpu.registers.edi = 0x4000;
    let mut input = input(&code, cpu, &[10, 10]);
    input.machine.push((12, 0x6003u32.to_le_bytes().to_vec()));
    input.machine.push((16, 0x4003u32.to_le_bytes().to_vec()));
    input.guest.push((0x2ffe, vec![1, 2]));
    input.guest.push((0x6000, (3..=16).collect()));
    let block = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 1)
            .unwrap(),
    )
    .with_interpreter(step());
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => step(),
    };
    let observation = engine.observe(module, &input, 2);
    let returned: Vec<_> = observation
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Return { snapshot, .. } => Some(snapshot),
            _ => None,
        })
        .collect();
    let first = CpuState::from_bytes(returned[0].cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (first.eip, first.instruction_count, first.registers.ecx),
        (0x1000, 0, 3)
    );
    assert_eq!((first.registers.esi, first.registers.edi), (0x3002, 0x4004));
    assert_eq!(
        returned[0].guest,
        Some((0..4).map(|i| (0x4000 + i, (i + 1) as u8)).collect())
    );
    let last = CpuState::from_bytes(returned[1].cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (last.eip, last.instruction_count, last.registers.ecx),
        (0x1002, 1, 0)
    );
    assert_eq!((last.registers.esi, last.registers.edi), (0x300e, 0x4010));
    assert_eq!(
        returned[1].guest,
        Some((0..16).map(|i| (0x4000 + i, (i + 1) as u8)).collect())
    );
}
test_frontends!(rep_chunk_fallback, check_rep_chunk_fallback);

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

fn check_zero_count(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xaa, 0x40];
    let mut cpu = initial();
    cpu.registers.edi = 0xffffffff;
    let observations = run(engine, frontend, &code, 2, &input(&code, cpu, &[1]));
    assert_eq!(observations[0].0, SLICE_EXHAUSTED);
    assert_eq!(observations[0].1.eip, 0x1002);
    assert_eq!(observations[0].1.instruction_count, 1);
    assert_eq!(observations[0].1.registers, cpu.registers);
}
test_frontends!(zero_count, check_zero_count);

fn check_comparison_fault(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa6]; // REPE CMPSB
    let mut cpu = initial();
    cpu.registers.ecx = 3;
    cpu.registers.esi = 0x2ffe;
    cpu.registers.edi = 0x3ffe;
    cpu.flags.bytes.cf = 1;
    let mut input = input(&code, cpu, &[1, 1, 1]);
    input.guest.push((0x2ffe, vec![7, 8]));
    input.guest.push((0x3ffe, vec![7, 8]));
    let observations = run(engine, frontend, &code, 1, &input);
    for (_, state) in &observations {
        assert_eq!(state.flags, cpu.flags);
    }
    assert_eq!(observations[2].0, (4 << 48) | 0x4000);
    assert_eq!(observations[2].1.registers.ecx, 1);
    assert_eq!(observations[2].1.registers.esi, 0x3000);
    assert_eq!(observations[2].1.registers.edi, 0x4000);
    assert_eq!(observations[2].1.instruction_count, 0);
}
test_frontends!(comparison_fault, check_comparison_fault);

fn check_comparison_completion(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa6];
    let mut cpu = initial();
    cpu.registers.ecx = 2;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x3000;
    let mut input = input(&code, cpu, &[1]);
    input.guest.push((0x2000, vec![2]));
    input.guest.push((0x3000, vec![1]));
    let observations = run(engine, frontend, &code, 1, &input);
    let state = observations[0].1;
    assert_eq!(state.eip, 0x1002);
    assert_eq!(state.instruction_count, 1);
    assert_eq!(state.registers.ecx, 1);
    assert_eq!(state.flags.status_source.left, 2);
    assert_eq!(state.flags.status_source.right, 1);
}
test_frontends!(comparison_completion, check_comparison_completion);

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

fn check_backward_fill_chunks(engine: Engine) {
    let code = [0xf3, 0xab];
    let mut cpu = initial();
    cpu.registers.ecx = 1026;
    cpu.registers.edi = 0x3008;
    cpu.registers.eax = 0x44332211;
    cpu.flags.bytes.df = 1;
    let input = input(&code, cpu, &[1026]);
    let observation = engine.observe(interpreter(), &input, 1);
    let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
        panic!("run resumes chunks internally until exhaustion");
    };
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (
            state.eip,
            state.instruction_count,
            state.registers.ecx,
            state.registers.edi
        ),
        (0x1002, 1, 0, 0x2000)
    );
    assert_eq!(state.flags, cpu.flags);
    assert_eq!(
        snapshot.guest,
        Some(
            (0..4104)
                .map(|i| (0x2004 + i, [0x11, 0x22, 0x33, 0x44][i as usize % 4]))
                .collect()
        )
    );
}
#[test]
fn backward_fill_resumes_chunks_internally() {
    check_backward_fill_chunks(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_backward_fill_resumes_chunks_internally() {
    check_backward_fill_chunks(Engine::V8);
}

fn real_interpreter() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| {
        TestModule::new(
            &Compiler::new(crate::ExecutionProfile::Real16)
                .with_execution_budget()
                .compile_interpreter()
                .unwrap(),
        )
    })
}

fn check_port_remapping(engine: Engine, frontend: Frontend) {
    use crate::support::step::DeviceUpdate;
    for opcode in [0x6c, 0x6e] {
        let code = [0xf3, opcode];
        let mut cpu = initial();
        cpu.segments = crate::Segments::real_mode();
        cpu.registers.ecx = 2;
        cpu.registers.edx = 0x80;
        cpu.registers.esi = 0x2000;
        cpu.registers.edi = 0x2000;
        let mut input = Input::new(&cpu.to_bytes());
        input.budgets = vec![2];
        input.observe_guest = true;
        input.physical_pages = vec![(1, 0x1000, true), (2, 0x2000, true)];
        input.guest = vec![
            (0x1000, code.to_vec()),
            (0x2000, vec![0x11, 0x12]),
            (0x3000, vec![0x21, 0x22]),
        ];
        input.port_updates = vec![DeviceUpdate {
            map: vec![(20, 0x3000u32.to_le_bytes().to_vec())],
            ..DeviceUpdate::default()
        }];
        if opcode == 0x6c {
            input.port_reads = vec![0x31, 0x32];
        }
        let block = TestModule::new(
            &Compiler::new(crate::ExecutionProfile::Real16)
                .with_execution_budget()
                .compile_block(0x1000, &code, 1)
                .unwrap(),
        )
        .with_interpreter(real_interpreter());
        let module = match frontend {
            Frontend::Block => &block,
            Frontend::Interpreter => real_interpreter(),
        };
        let observation = engine.observe(module, &input, 1);
        let Some(Event::Return { snapshot, .. }) = observation.events.last() else {
            panic!("expected return")
        };
        let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
        assert_eq!(
            (state.eip, state.instruction_count, state.registers.ecx),
            (0x1002, 1, 0)
        );
        if opcode == 0x6c {
            assert_eq!(snapshot.guest, Some(vec![(0x3000, 0x31), (0x3001, 0x32)]));
        } else {
            let values: Vec<_> = observation
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::PortWrite { value, .. } => Some(*value),
                    _ => None,
                })
                .collect();
            assert_eq!(values, vec![0x11, 0x22]);
            assert!(observation.guest_unchanged);
        }
    }
}
test_frontends!(port_remapping, check_port_remapping);

fn check_zero_count_port_rep(engine: Engine, frontend: Frontend) {
    for opcode in [0x6c, 0x6e] {
        let code = [0xf3, opcode];
        let mut cpu = initial();
        cpu.segments = crate::Segments::real_mode();
        let mut input = Input::new(&cpu.to_bytes());
        input.budgets = vec![1];
        input.physical_pages = vec![(1, 0x1000, true)];
        input.guest = vec![(0x1000, code.to_vec())];
        let block = TestModule::new(
            &Compiler::new(crate::ExecutionProfile::Real16)
                .with_execution_budget()
                .compile_block(0x1000, &code, 1)
                .unwrap(),
        )
        .with_interpreter(real_interpreter());
        let module = match frontend {
            Frontend::Block => &block,
            Frontend::Interpreter => real_interpreter(),
        };
        let observation = engine.observe(module, &input, 1);
        assert!(observation
            .events
            .iter()
            .all(|event| matches!(event, Event::Return { .. } | Event::Dispatch { .. })));
        let Some(Event::Return { snapshot, .. }) = observation.events.last() else {
            panic!("expected return")
        };
        let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
        assert_eq!(
            (state.eip, state.instruction_count, state.registers.ecx),
            (0x1002, 1, 0)
        );
    }
}
test_frontends!(zero_count_port_rep, check_zero_count_port_rep);

fn check_x87_fault_budget(engine: Engine) {
    let code = [0x40, 0x9b]; // INC EAX; FWAIT with a pending #MF
    let mut cpu = initial();
    cpu.x87.status.error_summary = 1;
    let mut input = input(&code, cpu, &[3]);
    input.observe_budget = true;
    let module = TestModule::new(
        &Compiler::new(SegmentProfile::Flat32)
            .with_execution_budget()
            .compile_block(0x1000, &code, 2)
            .unwrap(),
    );
    let observed = engine.observe(&module, &input, 1);
    let [Event::Return { snapshot, .. }, Event::Budget { remaining }] = observed.events.as_slice()
    else {
        panic!("expected guest fault and budget")
    };
    let state = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
    assert_eq!(
        (state.eip, state.instruction_count, state.registers.eax),
        (0x1001, 1, 1)
    );
    assert_eq!(*remaining, 2);
}
#[test]
fn pending_x87_fault_publishes_remaining_budget() {
    check_x87_fault_budget(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_pending_x87_fault_publishes_remaining_budget() {
    check_x87_fault_budget(Engine::V8);
}
