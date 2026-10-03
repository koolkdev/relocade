//! A full-range proof covers repeated accesses; failed proofs restart in the interpreter.

use super::{flags, record};
use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly},
    guest::{Exit, Machine, Mapping, Permissions::ReadWrite},
    step::{Argument, Engine, Event, Input, Outcome, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, Gpr32::*};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef};

fn long_reads() -> Vec<Case> {
    let payload = [0x12, 0x34, 0x56, 0x78].repeat(1026);
    let mut cases = Vec::new();
    for backward in [false, true] {
        cases.push(
            Case::preserving_flags(
                format!("REP LODSD reads three contiguous frames, DF {backward}"),
                &[0xf3, 0xad],
            )
            .stored_flags(record(u8::from(backward)))
            .register(Ecx, 1026, 0)
            .register(Eax, 0xaabb_ccdd, 0x7856_3412)
            .register(
                Esi,
                if backward { 0x6002 } else { 0x4ffe },
                if backward { 0x4ffa } else { 0x6006 },
            )
            .map_page(4, 0x8000, ReadOnly)
            .map_page(5, 0x9000, ReadOnly)
            .map_page(6, 0xa000, ReadOnly)
            .memory(0x4ffe, &payload, ReadOnly),
        );
    }
    cases.push(
        Case::replacing_flags(
            "REPNE SCASB scans three frames until the final match",
            &[0xf2, 0xae],
            flags(10),
        )
        .stored_flags(record(0))
        .initial_register(Eax, 1)
        .register(Ecx, 4100, 0)
        .register(Edi, 0x4ffe, 0x6002)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(5, 0x9000, ReadOnly)
        .map_page(6, 0xa000, ReadOnly)
        .memory(0x4ffe, &[vec![0; 4099], vec![1]].concat(), ReadOnly),
    );
    cases
}

test_cases!(complete_ranges_cover_repeated_reads, long_reads());

fn check_handoff_completion(engine: Engine) {
    let code = [0xf3, 0xac];
    let linked = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap())
        .with_interpreter(TestModule::interpreter());
    let mut machine = Machine::new(&code);
    machine.cpu.flags = record(0);
    machine.cpu.registers.eax = 0xaabb_ccdd;
    machine.cpu.registers.ecx = 3;
    machine.cpu.registers.esi = 0x4ffe;
    // The future page is missing. The preflight must not fault or consume any
    // element; the checked interpreter retains both completed loads.
    machine.memory(0x4ffe, &[0x12, 0x34], ReadOnly);
    let mut expected = machine.state();
    expected.cpu.registers.eax = 0xaabb_cc34;
    expected.cpu.registers.ecx = 1;
    expected.cpu.registers.esi = 0x5000;
    let actual = machine.run(&linked, engine);
    assert_eq!(actual.state, expected);
    assert_eq!(
        actual.exit,
        Exit::PageFault {
            address: 0x5000,
            error: 0
        }
    );
    assert!(actual.machine_unchanged && actual.dispatches.is_empty());
}

fn check_direct_completion(engine: Engine) {
    let code = [0xf3, 0xad];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for backward in [false, true] {
        let mut machine = Machine::new(&code);
        machine.cpu.flags = record(u8::from(backward));
        machine.cpu.registers.ecx = 1026;
        machine.cpu.registers.esi = if backward { 0x6002 } else { 0x4ffe };
        machine.memory(0x4ffe, &[0x12, 0x34, 0x56, 0x78].repeat(1026), ReadOnly);
        let mut expected = machine.state();
        expected.cpu.registers.eax = 0x7856_3412;
        expected.cpu.registers.ecx = 0;
        expected.cpu.registers.esi = if backward { 0x4ffa } else { 0x6006 };
        expected.cpu.eip = 0x1002;
        expected.cpu.instruction_count = expected.cpu.instruction_count.wrapping_add(1);
        let actual = machine.run(&block, engine);
        assert_eq!(actual.state, expected);
        assert_eq!(actual.exit, Exit::Dispatch(0x1002));
        assert_eq!(actual.dispatches, [(0x1002, expected)]);
        assert!(actual.machine_unchanged);
    }
}

#[test]
fn contiguous_ranges_complete_in_the_jit_without_handoff() {
    check_direct_completion(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_contiguous_ranges_complete_in_the_jit_without_handoff() {
    check_direct_completion(Engine::V8);
}

fn check_direct_writes_and_comparison(engine: Engine) {
    let code = [0xf3, 0xa4];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    let mut machine = Machine::with_mappings(
        0x1000,
        &code,
        &[
            Mapping {
                page: 4,
                frame: 0x8000,
                permissions: ReadOnly,
            },
            Mapping {
                page: 7,
                frame: 0x8000,
                permissions: ReadWrite,
            },
        ],
    );
    machine.cpu.flags = record(0);
    machine.cpu.registers.ecx = 5;
    machine.cpu.registers.esi = 0x4000;
    machine.cpu.registers.edi = 0x7001;
    machine.memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadOnly);
    let mut expected = machine.state();
    expected.cpu.registers.ecx = 0;
    expected.cpu.registers.esi = 0x4005;
    expected.cpu.registers.edi = 0x7006;
    expected.cpu.eip = 0x1002;
    expected.cpu.instruction_count = expected.cpu.instruction_count.wrapping_add(1);
    // Scalar forward copying observes earlier writes through the alias.
    expected.memory.write(0x7001, &[1; 5]);
    let actual = machine.run(&block, engine);
    assert_eq!(actual.state, expected);
    assert_eq!(actual.exit, Exit::Dispatch(0x1002));

    let code = [0xf3, 0xaa];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    let mut machine = Machine::new(&code);
    machine.cpu.flags = record(0);
    machine.cpu.registers.eax = 0x1234_56ab;
    machine.cpu.registers.ecx = 4100;
    machine.cpu.registers.edi = 0x4ffe;
    machine.memory(0x4ffe, &[0xa5; 4100], ReadWrite);
    let mut expected = machine.state();
    expected.cpu.registers.ecx = 0;
    expected.cpu.registers.edi = 0x6002;
    expected.cpu.eip = 0x1002;
    expected.cpu.instruction_count = expected.cpu.instruction_count.wrapping_add(1);
    expected.memory.write(0x4ffe, &[0xab; 4100]);
    let actual = machine.run(&block, engine);
    assert_eq!(actual.state, expected);
    assert_eq!(actual.exit, Exit::Dispatch(0x1002));

    let code = [0xf2, 0xae];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    let mut machine = Machine::new(&code);
    machine.cpu.flags = record(0);
    machine.cpu.registers.eax = 7;
    machine.cpu.registers.ecx = 4;
    machine.cpu.registers.edi = 0x4000;
    machine.memory(0x4000, &[2, 2, 7, 2], ReadOnly);
    let actual = machine.run(&block, engine);
    assert_eq!(actual.exit, Exit::Dispatch(0x1002));
    assert_eq!(actual.state.cpu.registers.ecx, 1);
    assert_eq!(actual.state.cpu.registers.edi, 0x4003);
    let cpu = actual.state.cpu.to_bytes().to_vec();
    let observer = TestModule::new(&crate::state::compile_flag_observer().unwrap());
    let observation = engine.observe(&observer, &Input::new(&cpu), 1);
    let [Event::Return {
        outcome: Outcome::Returned(values),
        snapshot,
    }] = observation.events.as_slice()
    else {
        panic!("the flag observer returns once");
    };
    assert_eq!(values, &[0, 1, 0, 1, 0, 0].map(Argument::I32));
    assert_eq!(snapshot.cpu, cpu);
}

#[test]
fn resolved_writes_preserve_alias_order_and_comparisons_stop_early_in_the_jit() {
    check_direct_writes_and_comparison(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_resolved_writes_preserve_alias_order_and_comparisons_stop_early_in_the_jit() {
    check_direct_writes_and_comparison(Engine::V8);
}

#[test]
fn failed_preflight_publishes_prior_instructions_at_the_rep_entry() {
    let code = [0xb9, 3, 0, 0, 0, 0xf3, 0xac, 0xb8, 99, 0, 0, 0];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
    let mut machine = Machine::new(&code);
    machine.cpu.flags = record(0);
    machine.cpu.registers.eax = 0xaabb_ccdd;
    machine.cpu.registers.esi = 0x4ffe;
    machine.memory(0x4ffe, &[0x12, 0x34], ReadOnly);
    let mut expected = machine.state();
    expected.cpu.registers.ecx = 3;
    expected.cpu.eip = 0x1005;
    expected.cpu.instruction_count = expected.cpu.instruction_count.wrapping_add(1);
    let actual = machine.run(&block, Engine::Wasmtime);
    assert_eq!(actual.state, expected);
    assert_eq!(actual.exit, Exit::Interpret);
    assert!(actual.machine_unchanged && actual.dispatches.is_empty());
}

#[test]
fn failed_range_preflight_completes_through_the_linked_interpreter() {
    check_handoff_completion(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_failed_range_preflight_completes_through_the_linked_interpreter() {
    check_handoff_completion(Engine::V8);
}

#[test]
fn jit_repeated_loops_have_no_page_checks_or_checked_fallback_loop() {
    for code in [
        &[0xf3, 0xa4][..],
        &[0xf3, 0xab],
        &[0xf3, 0xad],
        &[0xf3, 0xa7],
        &[0xf2, 0xaf],
        &[0xb9, 3, 0, 0, 0, 0xf3, 0xa4],
    ] {
        let module =
            compile_block_from_bytes(0x1000, code, if code[0] == 0xb9 { 2 } else { 1 }).unwrap();
        let mut machine_memory = None;
        let mut memories = 0;
        let mut function = 0;
        let mut entry = None;
        let mut loops = 0;
        let mut depth = 0usize;
        let mut loop_depth = None;
        for payload in Parser::new(0).parse_all(&module.bytes) {
            match payload.unwrap() {
                Payload::ImportSection(imports) => {
                    for import in imports {
                        let import = import.unwrap();
                        match import.ty {
                            TypeRef::Memory(_) => {
                                if import.name == "machine" {
                                    machine_memory = Some(memories);
                                }
                                memories += 1;
                            }
                            TypeRef::Func(_) => function += 1,
                            _ => {}
                        }
                    }
                }
                Payload::ExportSection(exports) => {
                    for export in exports {
                        let export = export.unwrap();
                        if export.name == module.entry && export.kind == ExternalKind::Func {
                            entry = Some(export.index);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    if Some(function) == entry {
                        for operation in body.get_operators_reader().unwrap() {
                            match operation.unwrap() {
                                Operator::Loop { .. } => {
                                    loops += 1;
                                    depth += 1;
                                    loop_depth = Some(depth);
                                }
                                Operator::I32Load { memarg }
                                    if loop_depth.is_some()
                                        && Some(memarg.memory) == machine_memory =>
                                {
                                    panic!("a resolved REP loop must not read page entries");
                                }
                                Operator::Call { .. } if loop_depth.is_some() => {
                                    panic!("a resolved REP loop must not call access helpers")
                                }
                                Operator::Block { .. } | Operator::If { .. } => depth += 1,
                                Operator::End => {
                                    if loop_depth == Some(depth) {
                                        loop_depth = None;
                                    }
                                    depth = depth.saturating_sub(1);
                                }
                                _ => {}
                            }
                        }
                    }
                    function += 1;
                }
                _ => {}
            }
        }
        assert_eq!(
            loops, 1,
            "a JIT block contains only the resolved loop: {code:x?}"
        );
    }
}
