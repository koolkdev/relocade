//! Counter conditions, operand-size target truncation, and pending ZF consumers.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, CpuState,
    Gpr32::{Eax, Ecx},
};

const OPCODES: [u8; 4] = [0xe3, 0xe2, 0xe1, 0xe0];

fn input_flags(zero: bool) -> Flags<bool> {
    Flags {
        zf: zero,
        ..Flags::all(true)
    }
}

fn count_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    // JECXZ and LOOP must also work without interpreting the incoming flags.
    for (opcode, before, after, taken) in [
        (0xe3, 0, 0, true),
        (0xe3, 1, 1, false),
        (0xe3, 0x0001_0000, 0x0001_0000, false),
        (0xe2, 0, u32::MAX, true),
        (0xe2, 1, 0, false),
        (0xe2, 2, 1, true),
        (0xe2, 0x0001_0000, 0xffff, true),
    ] {
        let code = if before == 0x0001_0000 {
            vec![0x66, opcode, 0x7f]
        } else {
            vec![opcode, 0x7f]
        };
        cases.push(
            Case::preserving_flags(format!("{code:02x?}, ECX={before:08x}"), &code)
                .at(0x2000 - code.len() as u32)
                .register(Ecx, before, after)
                .dispatch(if taken { 0x207f } else { 0x2000 }),
        );
    }
    // The decrement is independent of ZF, and neither branch may replace that ZF.
    for (before, after, zero, loope, loopne) in [
        (0, u32::MAX, false, false, true),
        (0, u32::MAX, true, true, false),
        (1, 0, false, false, false),
        (1, 0, true, false, false),
        (2, 1, false, false, true),
        (2, 1, true, true, false),
        (0x0001_0000, 0xffff, false, false, true),
        (0x0001_0000, 0xffff, true, true, false),
    ] {
        for (opcode, taken) in [(0xe1, loope), (0xe0, loopne)] {
            let code = if before == 0x0001_0000 {
                vec![0x66, opcode, 0x7f]
            } else {
                vec![opcode, 0x7f]
            };
            cases.push(
                Case::new(
                    format!("{code:02x?}, ECX={before:08x}, ZF={zero}"),
                    &code,
                    input_flags(zero),
                    Flags::all(Preserved),
                )
                .at(0x2000 - code.len() as u32)
                .register(Ecx, before, after)
                .preserve_flag_record()
                .dispatch(if taken { 0x207f } else { 0x2000 }),
            );
        }
    }
    cases
}

struct BranchInput {
    count: u32,
    after_count: u32,
    zero_flag: bool,
    taken: bool,
}

// Each opcode supplies one taken and one untaken input for target and fetch checks.
#[rustfmt::skip]
fn branch_inputs(opcode: u8) -> [BranchInput; 2] {
    match opcode {
        0xe3 => [
            BranchInput { count: 0, after_count: 0, zero_flag: false, taken: true },
            BranchInput { count: 1, after_count: 1, zero_flag: false, taken: false },
        ],
        0xe2 => [
            BranchInput { count: 2, after_count: 1, zero_flag: false, taken: true },
            BranchInput { count: 1, after_count: 0, zero_flag: false, taken: false },
        ],
        0xe1 => [
            BranchInput { count: 2, after_count: 1, zero_flag: true, taken: true },
            BranchInput { count: 2, after_count: 1, zero_flag: false, taken: false },
        ],
        0xe0 => [
            BranchInput { count: 2, after_count: 1, zero_flag: false, taken: true },
            BranchInput { count: 2, after_count: 1, zero_flag: true, taken: false },
        ],
        _ => unreachable!(),
    }
}

#[rustfmt::skip]
fn target_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (opcode, (origin, prefix, displacement, target, fallthrough)) in OPCODES.into_iter().cycle().zip([
        (0x1000, &[][..], 0, 0x1002, 0x1002),
        (0x1000, &[], 0x7f, 0x1081, 0x1002),
        (0x1000, &[], 0x80, 0x0f82, 0x1002),
        (0x1000, &[], 0xfe, 0x1000, 0x1002),
        (0, &[], 0x80, 0xffff_ff82, 2),
        (0xffff_fffe, &[], 0x7f, 0x7f, 0),
        (0xffff_ffff, &[], 0x80, 0xffff_ff81, 1),
        (0x1234_1000, &[0x66], 0x7f, 0x1082, 0x1234_1003),
        (0x1234_1000, &[0x66], 0x80, 0x0f83, 0x1234_1003),
        (0x1234_fffe, &[0x66], 0, 1, 0x1235_0001),
        (0xffff_fffe, &[0x66], 0xff, 0, 1),
        (0x1234_1000, &[0x66, 0x66], 0xfc, 0x1000, 0x1234_1004),
    ]) {
        for BranchInput { count, after_count, zero_flag, taken } in branch_inputs(opcode) {
            let code = [prefix, &[opcode, displacement]].concat();
            cases.push(Case::new(format!("counted branch {code:02x?} at {origin:08x}, taken={taken}"),
                &code, input_flags(zero_flag), Flags::all(Preserved))
                .at(origin).register(Ecx, count, after_count).preserve_flag_record()
                .dispatch(if taken { target } else { fallthrough }));
        }
    }
    cases
}

fn noncanonical_zero() -> Vec<Case> {
    [false, true]
        .into_iter()
        .map(|zero| {
            let mut record = CpuState::filled(0xa5).flags;
            record.status_source.kind = 0;
            record.bytes.zf = if zero { 0x81 } else { 0xfe };
            Case::new(
                format!("LOOPE reads only the low ZF bit, ZF={zero}"),
                &[0xe1, 0x7f],
                input_flags(zero),
                Flags::all(Preserved),
            )
            .stored_flags(record)
            .preserve_flag_record()
            .register(Ecx, 2, 1)
            .dispatch(if zero { 0x1081 } else { 0x1002 })
        })
        .collect()
}

#[rustfmt::skip]
fn counter_producers() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("SUB ECX produces zero before JECXZ")
            .initial_register(Ecx, 1)
            .step(Step::new(&[0x83, 0xe9, 1], Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Ecx, 0))
            .step(Step::preserving_flags(&[0xe3, 0x7f]).dispatch(0x1084)),
        Sequence::from_opaque_flags("LOOP reaches zero without changing the preceding SUB flags")
            .initial_register(Ecx, 2)
            .step(Step::new(&[0x83, 0xe9, 1], Flags::all(Clear)).register(Ecx, 1))
            .step(Step::preserving_flags(&[0xe2, 0x7f]).register(Ecx, 0).dispatch(0x1005)),
        Sequence::from_opaque_flags("LOOPE consumes pending ADD zero while its decrement wraps")
            .initial_register(Ecx, u32::MAX)
            .step(Step::new(&[0x83, 0xc1, 1], Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Ecx, 0))
            .step(Step::preserving_flags(&[0xe1, 0x7f]).register(Ecx, u32::MAX).dispatch(0x1084)),
        Sequence::from_opaque_flags("LOOPNE uses pending nonzero after counter arithmetic")
            .initial_register(Ecx, 3)
            .step(Step::new(&[0x83, 0xe9, 1], Flags::all(Clear)).register(Ecx, 2))
            .step(Step::preserving_flags(&[0xe0, 0x7f]).register(Ecx, 1).dispatch(0x1084)),
        Sequence::preserving_flags("JECXZ reads a preceding full ECX write")
            .initial_register(Ecx, 0x1234_5678)
            .step(Step::preserving_flags(&[0xb9, 0, 0, 0, 0]).register(Ecx, 0))
            .step(Step::preserving_flags(&[0xe3, 0x7f]).dispatch(0x1086)),
        Sequence::preserving_flags("operand size does not narrow the counter after a CX write")
            .initial_register(Ecx, 0x0001_1234)
            .step(Step::preserving_flags(&[0x66, 0xb9, 1, 0]).register(Ecx, 0x0001_0001))
            .step(Step::preserving_flags(&[0x66, 0xe2, 0x7f]).register(Ecx, 0x0001_0000).dispatch(0x1086)),
        Sequence::new("LOOPNE reads a preceding CH write and borrows across the low word", input_flags(false))
            .initial_register(Ecx, 0x0001_0100)
            .step(Step::preserving_flags(&[0xb5, 0]).register(Ecx, 0x0001_0000))
            .step(Step::preserving_flags(&[0x66, 0xe0, 0x7f]).register(Ecx, 0x0000_ffff).dispatch(0x1084)),
    ]
}

#[rustfmt::skip]
fn separate_flag_producer_cases() -> Vec<Sequence> {
    let mut cases = Vec::new();
    for (producer, initial, result, flags, branch, count, after, target) in [
        (&[0x83, 0xe8, 1][..], 1, 0,
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
            &[0xe1, 0x7f][..], 2, 1, 0x1089),
        (&[0x83, 0xe8, 1][..], 2, 1, Flags::all(Clear),
            &[0xe0, 0x7f][..], 2, 1, 0x1089),
        (&[0x83, 0xd0, 0][..], 0x7fff_ffff, 0x8000_0000,
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set },
            &[0xe1, 0x7f][..], 2, 1, 0x100a),
        (&[0x83, 0xd0, 0][..], 0x7fff_ffff, 0x8000_0000,
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set },
            &[0xe0, 0x7f][..], 2, 1, 0x1089),
    ] {
        cases.push(Sequence::new(format!("arithmetic {producer:02x?}, MOV ECX, then {branch:02x?}"), Flags::all(true))
            .initial_registers(&[(Eax, initial), (Ecx, 1)])
            .step(Step::new(producer, flags).register(Eax, result))
            .step(Step::preserving_flags(&[0xb9, 2, 0, 0, 0]).register(Ecx, count))
            .step(Step::preserving_flags(branch).register(Ecx, after).dispatch(target))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1));
    }
    cases
}

#[test]
fn forms_consume_one_displacement_and_end_the_block() {
    for opcode in OPCODES {
        for code in [vec![opcode, 0x66], vec![0x66, opcode, 0xe3]] {
            let complete = check_length(&code);
            let trailing = [&code[..], &[0xf4, 0x66, 0x0f]].concat();
            assert_eq!(
                compile_block_from_bytes(0x1000, &trailing, u32::MAX)
                    .unwrap()
                    .bytes,
                complete.bytes
            );
            let preceding = [&[0xb1, 7][..], &code].concat();
            assert_eq!(
                compile_block_from_bytes(0x1000, &preceding, u32::MAX)
                    .unwrap()
                    .bytes,
                compile_block_from_bytes(0x1000, &preceding, 2)
                    .unwrap()
                    .bytes
            );
        }
    }
}

fn missing_displacement(engine: Engine) {
    for opcode in OPCODES {
        for BranchInput {
            count, zero_flag, ..
        } in branch_inputs(opcode)
        {
            let mut image = Image::new(&[]);
            image.cpu.eip = 0x1fff;
            image.cpu.registers.ecx = count;
            image.cpu.flags.status_source.kind = 0;
            image.cpu.flags.bytes.zf = u8::from(zero_flag);
            image.data(0x3fff, &[opcode]);
            image.check_unchanged_exit(
                engine,
                TestModule::interpreter(),
                &format!("missing displacement for {opcode:02x}, ECX={count}, ZF={zero_flag}"),
                Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            );
        }
    }
}

#[test]
fn missing_displacement_precedes_the_counter_change() {
    missing_displacement(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn missing_displacement_precedes_the_counter_change_in_v8() {
    missing_displacement(Engine::V8);
}

test_cases!(counts_and_conditions_without_successor_fetch, count_cases());
test_cases!(signed_targets_and_operand_size, target_cases());
test_cases!(logical_zero_preserves_the_flag_record, noncanonical_zero());
test_sequences!(
    counter_producers_and_partial_register_writes,
    counter_producers()
);
test_sequences!(
    flags_survive_an_intervening_counter_write,
    separate_flag_producer_cases()
);
