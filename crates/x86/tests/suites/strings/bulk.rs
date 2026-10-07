//! Complete-range transfers and the element fallback share architectural results.

use super::record;
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::*;

fn long_transfers() -> Vec<Case> {
    let mut cases = Vec::new();
    let payload: Vec<_> = (0..4100).map(|i| (i % 251) as u8).collect();
    for backward in [false, true] {
        let start = if backward { 0x6001 } else { 0x4ffe };
        let destination = if backward { 0x9001 } else { 0x7ffe };
        let next_source = if backward { 0x4ffd } else { 0x6002 };
        let next_destination = if backward { 0x7ffd } else { 0x9002 };
        cases.push(
            Case::preserving_flags(
                format!("REP MOVSB spans three consecutive frames, DF {backward}"),
                &[0xf3, 0xa4],
            )
            .stored_flags(record(u8::from(backward)))
            .register(Ecx, 4100, 0)
            .register(Esi, start, next_source)
            .register(Edi, destination, next_destination)
            .map_page(4, 0x8000, ReadOnly)
            .map_page(5, 0x9000, ReadOnly)
            .map_page(6, 0xa000, ReadOnly)
            .map_page(7, 0xc000, ReadWrite)
            .map_page(8, 0xd000, ReadWrite)
            .map_page(9, 0xe000, ReadWrite)
            .memory(0x4ffe, &payload, ReadOnly)
            .memory(0x7ffe, &vec![0xa5; 4100], ReadWrite)
            .expect_memory(0x7ffe, &payload),
        );
    }
    for (code, width, value, pattern) in [
        (&[0xf3, 0xaa][..], 1u32, 0x1234_56abu32, &[0xab][..]),
        (
            &[0xf3, 0x66, 0xab][..],
            2,
            0x1234_abcdu32,
            &[0xcd, 0xab][..],
        ),
        (
            &[0xf3, 0xab][..],
            4,
            0x7856_3412u32,
            &[0x12, 0x34, 0x56, 0x78][..],
        ),
        (
            &[0xf3, 0x66, 0xab][..],
            2,
            0x1234_ababu32,
            &[0xab, 0xab][..],
        ),
        (&[0xf3, 0xab][..], 4, 0xabab_ababu32, &[0xab; 4][..]),
    ] {
        let expected: Vec<_> = pattern.iter().copied().cycle().take(4100).collect();
        cases.push(
            Case::preserving_flags(
                format!("REP STOS {width}-byte pattern {value:x} spans three frames"),
                code,
            )
            .stored_flags(record(1))
            .initial_register(Eax, value)
            .register(Ecx, 4100 / width, 0)
            .register(Edi, 0x9002 - width, 0x7ffe - width)
            .map_page(7, 0xc000, ReadWrite)
            .map_page(8, 0xd000, ReadWrite)
            .map_page(9, 0xe000, ReadWrite)
            .memory(0x7ffe, &vec![0xa5; 4100], ReadWrite)
            .expect_memory(0x7ffe, &expected),
        );
    }
    cases
}

fn fallbacks() -> Vec<Case> {
    let source: Vec<_> = (0..4100).map(|i| (i % 251) as u8).collect();
    vec![
        Case::preserving_flags(
            "REP MOVSB checks the middle frame and copies scattered backing",
            &[0xf3, 0xa4],
        )
        .stored_flags(record(0))
        .register(Ecx, 4100, 0)
        .register(Esi, 0x4ffe, 0x6002)
        .register(Edi, 0x7ffe, 0x9002)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(5, 0xb000, ReadOnly)
        .map_page(6, 0xa000, ReadOnly)
        .map_page(7, 0xc000, ReadWrite)
        .map_page(8, 0xd000, ReadWrite)
        .map_page(9, 0xe000, ReadWrite)
        .memory(0x4ffe, &source, ReadOnly)
        .memory(0x7ffe, &vec![0xa5; 4100], ReadWrite)
        .expect_memory(0x7ffe, &source),
        Case::preserving_flags(
            "REP STOSB cannot skip a read-only middle page",
            &[0xf3, 0xaa],
        )
        .stored_flags(record(0))
        .initial_register(Eax, 0x12)
        .register(Ecx, 4100, 4098)
        .register(Edi, 0x7ffe, 0x8000)
        .map_page(7, 0xc000, ReadWrite)
        .map_page(8, 0xd000, ReadOnly)
        .map_page(9, 0xe000, ReadWrite)
        .memory(0x7ffe, &[0xa5; 2], ReadWrite)
        .memory(0x8000, &vec![0xa5; 4096], ReadOnly)
        .memory(0x9000, &[0xa5; 2], ReadWrite)
        .expect_memory(0x7ffe, &[0x12; 2])
        .fault(0x8000, 3),
        Case::preserving_flags(
            "REP STOSD oversized byte count retains prefix before fault",
            &[0xf3, 0xab],
        )
        .stored_flags(record(0))
        .initial_register(Eax, 0x12)
        .register(Ecx, 0x4000_0001, 0x4000_0000)
        .register(Edi, 0x7ffc, 0x8000)
        .memory(0x7ffc, &[0xa5; 4], ReadWrite)
        .expect_memory(0x7ffc, &[0x12, 0, 0, 0])
        .fault(0x8000, 2),
        Case::preserving_flags(
            "REP MOVSB physically identical ranges keep their bytes",
            &[0xf3, 0xa4],
        )
        .stored_flags(record(0))
        .register(Ecx, 4, 0)
        .register(Esi, 0x4000, 0x4004)
        .register(Edi, 0x7000, 0x7004)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(7, 0x8000, ReadWrite)
        .backing(0x8000, &[1, 2, 3, 4]),
    ]
}

fn constant_counts() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("constant ECX uses a short range across a page boundary")
            .stored_flags(record(0))
            .initial_registers(&[(Esi, 0x4fff), (Edi, 0x7fff)])
            .map_page(4, 0x8000, ReadOnly)
            .map_page(5, 0x9000, ReadOnly)
            .map_page(7, 0xc000, ReadWrite)
            .map_page(8, 0xd000, ReadWrite)
            .memory(0x4fff, &[1, 2, 3], ReadOnly)
            .memory(0x7fff, &[0xa5; 3], ReadWrite)
            .step(Step::preserving_flags(&[0xb9, 3, 0, 0, 0]).register(Ecx, 3))
            .step(
                Step::preserving_flags(&[0xf3, 0xa4])
                    .register(Ecx, 0)
                    .register(Esi, 0x5002)
                    .register(Edi, 0x8002)
                    .expect_memory(0x7fff, &[1, 2, 3]),
            ),
    ]
}

test_cases!(complete_ranges_and_patterns, long_transfers());
test_cases!(unavailable_ranges_preserve_element_semantics, fallbacks());
test_sequences!(constant_count_and_successor_state, constant_counts());

fn check_patterned_completion(engine: super::Engine) {
    use crate::support::{
        guest::{Exit, Machine},
        step::TestModule,
    };
    use wasm86_x86::compile_block_from_bytes;

    for (code, width, value, pattern) in [
        (
            &[0xf3, 0x66, 0xab][..],
            2u32,
            0x1234_abcdu32,
            &[0xcd, 0xab][..],
        ),
        (
            &[0xf3, 0xab][..],
            4,
            0x7856_3412,
            &[0x12, 0x34, 0x56, 0x78][..],
        ),
    ] {
        let block = TestModule::new(&compile_block_from_bytes(0x1000, code, 1).unwrap());
        for backward in [false, true] {
            let mut machine = Machine::new(code);
            machine.cpu.flags = record(u8::from(backward));
            machine.cpu.registers.eax = value;
            machine.cpu.registers.ecx = 5;
            machine.cpu.registers.edi = 0x4ffb + if backward { 4 * width } else { 0 };
            machine.memory(0x4ffb, &vec![0xa5; (5 * width) as usize], ReadWrite);
            let mut expected = machine.state();
            expected.cpu.registers.ecx = 0;
            expected.cpu.registers.edi = if backward {
                0x4ffb - width
            } else {
                0x4ffb + 5 * width
            };
            expected.cpu.eip = 0x1000 + code.len() as u32;
            expected.cpu.instruction_count = expected.cpu.instruction_count.wrapping_add(1);
            expected.memory.write(0x4ffb, &pattern.repeat(5));
            let actual = machine.run(&block, engine);
            assert_eq!(actual.state, expected);
            assert_eq!(actual.exit, Exit::Dispatch(expected.cpu.eip));
            assert_eq!(actual.dispatches, [(expected.cpu.eip, expected)]);
            assert!(actual.machine_unchanged);
        }
    }
}

#[test]
fn patterned_stos_completes_in_the_jit_with_a_short_final_prefix_copy() {
    check_patterned_completion(super::Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_patterned_stos_completes_in_the_jit_with_a_short_final_prefix_copy() {
    check_patterned_completion(super::Engine::V8);
}

#[test]
fn native_transfers_reuse_the_range_checks_and_omit_the_stos_element_loop() {
    use wasm86_x86::compile_block_from_bytes;
    use wasmparser::{Operator, Parser, Payload, TypeRef};

    for (code, copies, fills, loops, probes) in [
        (&[0xf3, 0xa4][..], 1, 0, 1, 2),
        (&[0xf3, 0xa5][..], 1, 0, 1, 2),
        (&[0xf3, 0xaa][..], 0, 1, 0, 1),
        (&[0xf3, 0x66, 0xab][..], 1, 1, 1, 1),
        (&[0xf3, 0xab][..], 1, 1, 1, 1),
    ] {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let mut imported_functions = 0;
        let mut function = 0;
        let mut entry = None;
        let mut counts = (0, 0, 0, 0);
        for payload in Parser::new(0).parse_all(&module.bytes) {
            match payload.unwrap() {
                Payload::ImportSection(imports) => {
                    for import in imports {
                        if matches!(import.unwrap().ty, TypeRef::Func(_)) {
                            imported_functions += 1;
                            function += 1;
                        }
                    }
                }
                Payload::ExportSection(exports) => {
                    for export in exports {
                        let export = export.unwrap();
                        if export.name == module.entry {
                            entry = Some(export.index);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    if Some(function) == entry {
                        for operation in body.get_operators_reader().unwrap() {
                            match operation.unwrap() {
                                Operator::MemoryCopy { .. } => counts.0 += 1,
                                Operator::MemoryFill { .. } => counts.1 += 1,
                                Operator::Loop { .. } => counts.2 += 1,
                                // Memory declares its shared range helper before
                                // lazily requested instruction helpers.
                                Operator::Call { function_index }
                                    if function_index == imported_functions =>
                                {
                                    counts.3 += 1
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
        assert_eq!(counts, (copies, fills, loops, probes), "{code:x?}");
    }
}
