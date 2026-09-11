use super::input_flags;
use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case,
};
use wasm86_x86::{Gpr32::Ecx, StatusFlags, StoredFlags};

const CANARY: StoredFlags = StoredFlags {
    kind: 0,
    reserved: [0xa5; 3],
    left: 0x1234_5678,
    right: 0x8765_4321,
    status: StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
    },
    non_status: [0, 1, 0, 0, 0, 0x5a],
};

#[rustfmt::skip]
fn pending_flag_cases() -> Vec<Case> {
    let zero_flags = Flags { cf: false, pf: true, af: false, zf: true, sf: false, of: false };
    let borrow_flags = Flags { cf: true, pf: true, af: true, zf: false, sf: true, of: false };
    let one_flags = Flags::all(false);
    // Narrow records carry dirty upper bits; the low byte or word determines ZF.
    let records = [
        (1, 0xa5a5_a501, 0x5a5a_5a01, zero_flags),
        (1, 0xa5a5_a500, 0x5a5a_5a01, borrow_flags),
        (5, 0xa5a5_0001, 0x5a5a_0001, zero_flags),
        (5, 0xa5a5_0000, 0x5a5a_0001, borrow_flags),
        (9, 1, 1, zero_flags),
        (9, 0, 1, borrow_flags),
        (2, 0xa5a5_a500, 0x5a5a_5a00, zero_flags),
        (2, 0xa5a5_a500, 0x5a5a_5a01, one_flags),
        (6, 0xa5a5_0000, 0x5a5a_0000, zero_flags),
        (6, 0xa5a5_0000, 0x5a5a_0001, one_flags),
        (10, 0, 0, zero_flags),
        (10, 0, 1, one_flags),
        (3, 0xa5a5_a500, 0x8765_4321, zero_flags),
        (3, 0xa5a5_a501, 0x8765_4321, one_flags),
        (7, 0xa5a5_0000, 0x8765_4321, zero_flags),
        (7, 0xa5a5_0001, 0x8765_4321, one_flags),
        (11, 0, 0x8765_4321, zero_flags),
        (11, 1, 0x8765_4321, one_flags),
    ];
    let mut cases = Vec::new();
    for (kind, left, right, flags) in records {
        let record = StoredFlags {
            kind, left, right,
            status: StatusFlags { zf: u8::from(!flags.zf), ..CANARY.status },
            ..CANARY
        };
        for (opcode, condition_met) in [(0xe1, flags.zf), (0xe0, !flags.zf)] {
            for (count, after, target) in [
                (1, 0, 0x1234_1003),
                (2, 1, if condition_met { 0x1082 } else { 0x1234_1003 }),
            ] {
                cases.push(Case::new(format!("pending kind {kind}, ZF={}, opcode {opcode:02x}, ECX={count}", flags.zf),
                    &[0x66, opcode, 0x7f], flags, Flags::all(Preserved))
                    .at(0x1234_1000).stored_flags(record).preserve_flag_record()
                    .register(Ecx, count, after).dispatch(target));
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn flag_independent_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (opcode, count, after, taken) in [
        (0xe3, 0, 0, true), (0xe3, 0x0001_0000, 0x0001_0000, false),
        (0xe2, 0, 0xffff_ffff, true), (0xe2, 1, 0, false),
    ] {
        for (prefix, target, fallthrough) in [
            (&[][..], 0x1081, 0x1002), (&[0x66][..], 0x1082, 0x1003),
        ] {
            let code = [prefix, &[opcode, 0x7f]].concat();
            cases.push(Case::preserving_flags(format!("{code:02x?} preserves opaque flags, ECX={count:08x}"), &code)
                .register(Ecx, count, after).dispatch(if taken { target } else { fallthrough }));
        }
    }
    for zero in [false, true] {
        let record = StoredFlags {
            // Concrete status bytes expose logical low bits and preserve spare bits.
            status: StatusFlags { cf: 0xff, pf: 0x7f, af: 0x81, zf: if zero { 0x55 } else { 0xaa }, sf: 3, of: 0x5b },
            ..CANARY
        };
        for (opcode, target) in [
            (0xe1, if zero { 0x1081 } else { 0x1002 }),
            (0xe0, if zero { 0x1002 } else { 0x1081 }),
        ] {
            cases.push(Case::new(format!("concrete flags with spare bits, opcode {opcode:02x}, ZF={zero}"),
                &[opcode, 0x7f], input_flags(zero), Flags::all(Preserved))
                .stored_flags(record).preserve_flag_record().register(Ecx, 2, 1).dispatch(target));
        }
    }
    cases
}

test_cases!(pending_records_keep_every_byte, pending_flag_cases());
test_cases!(
    opaque_and_concrete_record_preservation,
    flag_independent_cases()
);
