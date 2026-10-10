use crate::{
    support::{
        execution::{test_frontends, Frontend},
        step::{Argument, Engine, Event, Input, Outcome, TestModule},
    },
    Compiler, CpuState, ExecutionProfile, SegmentProfile, Segments, SLICE_EXHAUSTED,
};
use std::sync::OnceLock;

fn interpreter(real: bool) -> &'static TestModule {
    static FLAT: OnceLock<TestModule> = OnceLock::new();
    static REAL: OnceLock<TestModule> = OnceLock::new();
    let (slot, profile) = if real {
        (&REAL, ExecutionProfile::Real16)
    } else {
        (&FLAT, SegmentProfile::Flat32.into())
    };
    slot.get_or_init(|| {
        TestModule::new(
            &Compiler::new(profile)
                .with_code_tracking()
                .compile_interpreter()
                .unwrap(),
        )
    })
}
fn input(code: &[u8], real: bool, budget: u32) -> Input {
    let cpu = CpuState {
        eip: 0x1000,
        segments: if real {
            Segments::real_mode()
        } else {
            Segments::flat32()
        },
        ..CpuState::default()
    };
    let mut input = Input::new(&cpu.to_bytes());
    input.budgets = vec![budget];
    input.guest = vec![(0x1000, code.to_vec())];
    input.observe_guest = true;
    for page in 1..5 {
        input
            .machine
            .push((page * 4, ((page << 12) | 3u32).to_le_bytes().to_vec()));
        input.physical_pages.push((page, page << 12, true));
    }
    input
}
fn returned(events: &[Event]) -> CpuState {
    let Some(Event::Return { snapshot, .. }) = events.last() else {
        panic!("expected return")
    };
    CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap())
}
fn compiler(real: bool) -> Compiler {
    Compiler::new(if real {
        ExecutionProfile::Real16
    } else {
        SegmentProfile::Flat32.into()
    })
    .with_code_tracking()
}

fn check_self_modification(engine: Engine, frontend: Frontend) {
    // INC EBX; MOV byte [next], DEC EAX; captured INC EAX.
    let code = [0x43, 0xc6, 0x05, 8, 0x10, 0, 0, 0x48, 0x40];
    let mut input = input(&code, false, 3);
    input.code_pages = vec![1];
    let block = TestModule::new(&compiler(false).compile_block(0x1000, &code, 3).unwrap())
        .with_interpreter(interpreter(false));
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter(false),
    };
    let result = engine.observe(module, &input, 1);
    let state = returned(&result.events);
    assert_eq!(
        (
            state.registers.eax,
            state.registers.ebx,
            state.eip,
            state.instruction_count
        ),
        (u32::MAX, 1, 0x1009, 3)
    );
    assert!(matches!(
        result.events[0],
        Event::CodeWrite {
            address: 0x1008,
            bytes: 1
        }
    ));
}
test_frontends!(self_modification, check_self_modification);

fn check_rep_redecodes_after_code_write(engine: Engine, frontend: Frontend) {
    // The first MOVSB overwrites its own REP prefix with NOP. The checked element
    // commits progress at EIP 1000, then live decoding executes that new NOP.
    let code = [0xf3, 0xa4];
    let mut input = input(&code, false, 2);
    let mut cpu = CpuState::from_bytes(input.cpu.as_slice().try_into().unwrap());
    cpu.registers.ecx = 5;
    cpu.registers.esi = 0x2000;
    cpu.registers.edi = 0x1000;
    input.cpu = cpu.to_bytes().to_vec();
    input.guest.push((0x2000, vec![0x90, 1, 2, 3, 4]));
    input.code_pages = vec![1];
    let block = TestModule::new(&compiler(false).compile_block(0x1000, &code, 1).unwrap())
        .with_interpreter(interpreter(false));
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter(false),
    };
    let result = engine.observe(module, &input, 1);
    let state = returned(&result.events);
    assert_eq!(
        (
            state.eip,
            state.instruction_count,
            state.registers.ecx,
            state.registers.esi,
            state.registers.edi
        ),
        (0x1001, 1, 4, 0x2001, 0x1001)
    );
    assert!(matches!(
        result.events[0],
        Event::CodeWrite {
            address: 0x1000,
            bytes: 1
        }
    ));
}
test_frontends!(
    rep_redecodes_after_code_write,
    check_rep_redecodes_after_code_write
);

fn check_guard_before_effects(engine: Engine) {
    for real in [false, true] {
        // Operand/address overrides retain the same test operands in Real16.
        let mut code = if real { vec![0x66, 0x67] } else { vec![] };
        code.extend([0x83, 0x05, 0xfe, 0x2f, 0, 0, 1]); // ADD dword [2ffe],1
        let mut input = input(&code, real, 1);
        input.code_pages = vec![3]; // only the second page of the operand is watched
        input.guest.push((0x2ffe, vec![0xff, 0, 0, 0]));
        let block = TestModule::new(&compiler(real).compile_block(0x1000, &code, 1).unwrap());
        let result = engine.observe(&block, &input, 1);
        let Event::Interpret { snapshot } = &result.events[0] else {
            panic!("write must restart before effects")
        };
        assert_eq!(snapshot.cpu, input.cpu);
        assert!(result.guest_unchanged);
        let block = block.with_interpreter(interpreter(real));
        let result = engine.observe(&block, &input, 1);
        assert_eq!(returned(&result.events).instruction_count, 1);
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::CodeWrite { .. })));
    }
}
#[test]
fn watched_span_handoff_precedes_effects() {
    check_guard_before_effects(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_watched_span_handoff_precedes_effects() {
    check_guard_before_effects(Engine::V8);
}

fn check_cached_proofs(engine: Engine) {
    // A read proof for the same watched page must not admit its later write.
    let code = [0xa1, 0, 0x20, 0, 0, 0xa3, 4, 0x20, 0, 0];
    let mut input = input(&code, false, 2);
    input.code_pages = vec![2];
    input.guest.push((0x2000, vec![0x78, 0x56, 0x34, 0x12]));
    let block = TestModule::new(&compiler(false).compile_block(0x1000, &code, 2).unwrap());
    let result = engine.observe(&block, &input, 1);
    let state = returned(&result.events);
    assert_eq!(
        (state.eip, state.instruction_count, state.registers.eax),
        (0x1005, 1, 0x12345678)
    );
    assert!(matches!(result.events[0], Event::Interpret { .. }));
    assert!(result.guest_unchanged);
    // Removing only the watch leaves an ordinary two-instruction block.
    input.code_pages.clear();
    let result = engine.observe(&block, &input, 1);
    assert_eq!(returned(&result.events).instruction_count, 2);
    assert!(!result
        .events
        .iter()
        .any(|event| matches!(event, Event::CodeWrite { .. } | Event::Interpret { .. })));
}
#[test]
fn cached_reads_do_not_authorize_watched_writes() {
    check_cached_proofs(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_cached_reads_do_not_authorize_watched_writes() {
    check_cached_proofs(Engine::V8);
}

fn check_locked_store(engine: Engine) {
    let code = [0xf0, 0x83, 0x05, 0, 0x20, 0, 0, 1]; // LOCK ADD dword [2000],1
    let mut input = input(&code, false, 1);
    input.code_pages = vec![2];
    let block = TestModule::new(&compiler(false).compile_block(0x1000, &code, 1).unwrap())
        .with_interpreter(interpreter(false));
    let result = engine.observe(&block, &input, 1);
    assert!(matches!(
        result.events[0],
        Event::CodeWrite {
            address: 0x2000,
            bytes: 4
        }
    ));
    assert_eq!(returned(&result.events).instruction_count, 1);
    let Some(Event::Return { outcome, snapshot }) = result.events.last() else {
        panic!()
    };
    assert_eq!(
        *outcome,
        Outcome::Returned(vec![Argument::I64(SLICE_EXHAUSTED as i64)])
    );
    assert_eq!(snapshot.guest, Some(vec![(0x2000, 1)]));
}
#[test]
fn native_atomic_notifies_before_watched_store() {
    check_locked_store(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_atomic_notifies_before_watched_store() {
    check_locked_store(Engine::V8);
}

fn check_probes_do_not_invalidate(engine: Engine, frontend: Frontend) {
    // FST m32 from an empty stack, with invalid-operation exceptions unmasked.
    // The destination is writable but the architectural store is suppressed.
    let code = [0xd9, 0x15, 0, 0x20, 0, 0];
    let mut input = input(&code, false, 1);
    let mut cpu = CpuState::from_bytes(input.cpu.as_slice().try_into().unwrap());
    cpu.x87.control.invalid_mask = 0;
    input.cpu = cpu.to_bytes().to_vec();
    input.code_pages = vec![2];
    let block = TestModule::new(&compiler(false).compile_block(0x1000, &code, 1).unwrap())
        .with_interpreter(interpreter(false));
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter(false),
    };
    let result = engine.observe(module, &input, 1);
    assert!(result.guest_unchanged);
    assert!(!result
        .events
        .iter()
        .any(|event| matches!(event, Event::CodeWrite { .. })));
    assert_eq!(returned(&result.events).x87.status.invalid, 1);

    // A watch does not replace an architectural write-permission denial.
    input.machine.push((8, 0x2001u32.to_le_bytes().to_vec()));
    let result = engine.observe(module, &input, 1);
    let state = returned(&result.events);
    assert_eq!(
        (state.eip, state.instruction_count, state.x87.status.invalid),
        (0x1000, 0, 0)
    );
    assert!(result.guest_unchanged);
    assert!(!result
        .events
        .iter()
        .any(|event| matches!(event, Event::CodeWrite { .. })));
}
test_frontends!(probes_do_not_invalidate, check_probes_do_not_invalidate);

fn check_partial_effect_handoff(engine: Engine) {
    for code in [&[0x60][..], &[0xc8, 8, 0, 2][..]] {
        // PUSHAD / ENTER 8,2
        let mut input = input(code, false, 1);
        let mut cpu = CpuState::from_bytes(input.cpu.as_slice().try_into().unwrap());
        cpu.registers.esp = 0x3004;
        cpu.registers.ebp = 0x3010;
        cpu.registers.eax = 0x12345678;
        input.cpu = cpu.to_bytes().to_vec();
        input.code_pages = vec![2];
        let block = TestModule::new(&compiler(false).compile_block(0x1000, code, 1).unwrap());
        let result = engine.observe(&block, &input, 1);
        let Event::Interpret { snapshot } = &result.events[0] else {
            panic!("entry handoff")
        };
        assert_eq!(snapshot.cpu, input.cpu);
        assert!(result.guest_unchanged);
        let block = block.with_interpreter(interpreter(false));
        let result = engine.observe(&block, &input, 1);
        assert_eq!(returned(&result.events).instruction_count, 1);
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::CodeWrite { .. })));
    }
}
#[test]
fn partial_effect_instructions_handoff_before_the_first_store() {
    check_partial_effect_handoff(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_partial_effect_instructions_handoff_before_the_first_store() {
    check_partial_effect_handoff(Engine::V8);
}
