use super::{input_flags, FORMS};
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::{Eax, Ecx};

#[rustfmt::skip]
fn arithmetic_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (producer, before, result, flags, outcomes) in [
        (&[0x83, 0xe9, 1][..], 1, 0,
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
            [(0, true), (0xffff_ffff, true), (0xffff_ffff, true), (0xffff_ffff, false)]),
        (&[0x83, 0xe9, 1][..], 2, 1,
            Flags::all(Clear),
            [(1, false), (0, false), (0, false), (0, false)]),
        (&[0x83, 0xe9, 1][..], 3, 2,
            Flags::all(Clear),
            [(2, false), (1, true), (1, false), (1, true)]),
        (&[0x83, 0xc1, 1][..], 0xffff_ffff, 0,
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear },
            [(0, true), (0xffff_ffff, true), (0xffff_ffff, true), (0xffff_ffff, false)]),
    ] {
        for ((opcode, name), (after, taken)) in FORMS.into_iter().zip(outcomes) {
            for (prefix, target, fallthrough) in [
                (&[][..], 0x1084, 0x1005), (&[0x66][..], 0x1085, 0x1006),
            ] {
                let branch = [prefix, &[opcode, 0x7f]].concat();
                cases.push(Case::from_opaque_flags(format!("{producer:02x?} produces ECX={result} before {name} {branch:02x?}"))
                    .instruction_count(0xffff_fffe).initial_register(Ecx, before)
                    .step(Step::new(producer, flags).register(Ecx, result))
                    .step(Step::preserving_flags(&branch).register(Ecx, after)
                        .dispatch(if taken { target } else { fallthrough }))
                    .trailing_code(&[0xb9, 0x78, 0x56, 0x34, 0x12], 1));
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn register_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (write, initial, written, branch, after, zero, target) in [
        (&[0xb9, 0, 0, 0, 0][..], 0x1234_5678, 0, &[0xe3, 0x7f][..], 0, false, 0x1086),
        (&[0xb9, 2, 0, 0, 0], 0, 2, &[0xe2, 0x7f], 1, false, 0x1086),
        (&[0xb9, 1, 0, 0, 0], 2, 1, &[0xe2, 0x7f], 0, true, 0x1007),
        (&[0x66, 0xb9, 0, 0], 0x0001_1234, 0x0001_0000, &[0x66, 0xe3, 0x7f], 0x0001_0000, false, 0x1007),
        (&[0x66, 0xb9, 1, 0], 0x0001_1234, 0x0001_0001, &[0x66, 0xe2, 0x7f], 0x0001_0000, true, 0x1086),
        (&[0xb1, 1], 0x0001_1200, 0x0001_1201, &[0x66, 0xe1, 0x7f], 0x0001_1200, true, 0x1084),
        (&[0xb5, 0], 0x0001_0100, 0x0001_0000, &[0x66, 0xe0, 0x7f], 0x0000_ffff, false, 0x1084),
    ] {
        cases.push(Case::new(format!("ECX write {write:02x?} feeds {branch:02x?}"), input_flags(zero))
            .instruction_count(0xffff_fffe).initial_register(Ecx, initial)
            .step(Step::preserving_flags(write).register(Ecx, written))
            .step(Step::preserving_flags(branch).register(Ecx, after).dispatch(target))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1));
    }
    cases
}

#[rustfmt::skip]
fn separate_flag_producer_cases() -> Vec<Case> {
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
        cases.push(Case::new(format!("arithmetic {producer:02x?}, MOV ECX, then {branch:02x?}"), Flags::all(true))
            .instruction_count(0xffff_fffd).initial_registers(&[(Eax, initial), (Ecx, 1)])
            .step(Step::new(producer, flags).register(Eax, result))
            .step(Step::preserving_flags(&[0xb9, 2, 0, 0, 0]).register(Ecx, count))
            .step(Step::preserving_flags(branch).register(Ecx, after).dispatch(target))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1));
    }
    cases
}

test_sequences!(
    counter_arithmetic_keeps_its_original_flags,
    arithmetic_cases()
);
test_sequences!(prior_full_and_partial_register_writes, register_cases());
test_sequences!(
    flags_survive_an_intervening_counter_write,
    separate_flag_producer_cases()
);
