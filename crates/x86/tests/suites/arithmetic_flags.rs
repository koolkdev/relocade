use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::Gpr32::{Eax, Ebx};

use wasm86_x86::{CpuState, StatusFlags, StoredFlags};

#[path = "arithmetic_flags/cases.rs"]
mod cases;

#[rustfmt::skip]
fn result_conditions() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new("byte nibble carry without unsigned or signed overflow", Flags::all(true))
            .initial_register(Eax, 0x4433_223f).initial_register(Ebx, 1)
            .step(Checkpoint::new(&[0x00, 0xd8],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x4433_2240))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("byte carry and signed overflow", Flags::all(true))
            .initial_register(Eax, 0x4433_2280).initial_register(Ebx, 0x80)
            .step(Checkpoint::new(&[0x02, 0xc3],
                Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set }).register(Eax, 0x4433_2200))
            .conditions([1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("word carry and signed overflow", Flags::all(true))
            .initial_register(Eax, 0x4433_8000).initial_register(Ebx, 0xdead_8000)
            .step(Checkpoint::new(&[0x66, 0x01, 0xd8],
                Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set }).register(Eax, 0x4433_0000))
            .conditions([1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("dword carry and signed overflow", Flags::all(true))
            .initial_register(Eax, 0x8000_0000).initial_register(Ebx, 0x8000_0000)
            .step(Checkpoint::new(&[0x03, 0xc3],
                Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set }).register(Eax, 0))
            .conditions([1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("parity uses the low byte", Flags::all(true))
            .initial_register(Eax, 0x100).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x05, 1, 0, 0, 0],
                Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x101))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("negative byte immediate sign extends for word ADD", Flags::all(true))
            .initial_register(Eax, 0x4433_0001).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x66, 0x83, 0xc0, 0xff],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x4433_0000))
            .conditions([0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("byte compare separates signed and unsigned order", Flags::all(true))
            .initial_register(Eax, 0x4433_227e).initial_register(Ebx, 0xfe)
            .step(Checkpoint::new(&[0x38, 0xd8],
                Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x4433_227e))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("word compare separates signed and unsigned order", Flags::all(true))
            .initial_register(Eax, 0x4433_7ffe).initial_register(Ebx, 0xdead_fffe)
            .step(Checkpoint::new(&[0x66, 0x3b, 0xc3],
                Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x4433_7ffe))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("dword compare separates signed and unsigned order", Flags::all(true))
            .initial_register(Eax, 0x7fff_fffe).initial_register(Ebx, 0xffff_fffe)
            .step(Checkpoint::new(&[0x39, 0xd8],
                Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x7fff_fffe))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("group compare sign extends negative immediate", Flags::all(true))
            .initial_register(Eax, 0xffff_ff80).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x83, 0xf8, 0x80],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0xffff_ff80))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("full group ADD carries across the dword", Flags::all(true))
            .initial_register(Eax, 0x997c_7e80).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x81, 0xc0, 0x80, 0x81, 0x83, 0x66],
                Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .conditions([0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("full group CMP leaves an equal dword unchanged", Flags::all(true))
            .initial_register(Eax, 0x6683_8180).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x81, 0xf8, 0x80, 0x81, 0x83, 0x66],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x6683_8180))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("byte group CMP preserves signed overflow", Flags::all(true))
            .initial_register(Eax, 0x4433_2280).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x80, 0xf8, 1],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Set }).register(Eax, 0x4433_2280))
            .conditions([1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0]),
        SequenceCase::new("accumulator byte CMP reports borrow", Flags::all(true))
            .initial_register(Eax, 0x4433_2200).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x3c, 0xff],
                Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x4433_2200))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("reverse byte CMP reads the register field first", Flags::all(true))
            .initial_register(Eax, 0x4433_22ff).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x3a, 0xc3],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_22ff))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("byte SUB borrows and preserves upper EAX", Flags::all(true))
            .initial_register(Eax, 0x4433_2200).initial_register(Ebx, 1)
            .step(Checkpoint::new(&[0x28, 0xd8],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x4433_22ff))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("reverse byte SUB reads old AL before replacing AH", Flags::all(true))
            .initial_register(Eax, 0x4433_8001).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x2a, 0xe0],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Set }).register(Eax, 0x4433_7f01))
            .conditions([1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0]),
        SequenceCase::new("word SUB retains signed overflow and low-byte parity", Flags::all(true))
            .initial_register(Eax, 0x4433_8000).initial_register(Ebx, 0xdead_0001)
            .step(Checkpoint::new(&[0x66, 0x29, 0xd8],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Set }).register(Eax, 0x4433_7fff))
            .conditions([1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("dword SUB separates signed and unsigned order", Flags::all(true))
            .initial_register(Eax, 0x7fff_fffe).initial_register(Ebx, 0xffff_fffe)
            .step(Checkpoint::new(&[0x2b, 0xc3],
                Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x8000_0000))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("group SUB sign extends its byte immediate to a word", Flags::all(true))
            .initial_register(Eax, 0x4433_0000).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x66, 0x83, 0xe8, 0xff],
                Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x4433_0001))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("group SUB sign extends its byte immediate to a dword", Flags::all(true))
            .initial_register(Eax, 0xffff_ff80).initial_register(Ebx, 0)
            .step(Checkpoint::new(&[0x83, 0xe8, 0x80],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
    ]
}

test_sequences!(results_and_conditions, result_conditions());

#[rustfmt::skip]
fn saved_conditions() -> Vec<SequenceCase> {
    let mut cases = Vec::new();
    for (name, kind, left, right, flags, conditions) in [
        ("stored byte ADD", 2, 0x00000080, 0x00000080,
            Flags { cf: true, pf: true, af: false, zf: true, sf: false, of: true },
            [1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        ("stored word ADD", 6, 0x00008000, 0x00008000,
            Flags { cf: true, pf: true, af: false, zf: true, sf: false, of: true },
            [1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        ("stored dword ADD", 10, 0x80000000, 0x80000000,
            Flags { cf: true, pf: true, af: false, zf: true, sf: false, of: true },
            [1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        ("stored byte SUB", 1, 0x0000007e, 0x000000fe,
            Flags { cf: true, pf: false, af: false, zf: false, sf: true, of: true },
            [1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1]),
        ("stored word SUB", 5, 0x00007ffe, 0x0000fffe,
            Flags { cf: true, pf: true, af: false, zf: false, sf: true, of: true },
            [1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        ("stored dword SUB", 9, 0x7ffffffe, 0xfffffffe,
            Flags { cf: true, pf: true, af: false, zf: false, sf: true, of: true },
            [1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        ("stored byte logic", 3, 0x00000080, 0x12345678,
            Flags { cf: false, pf: false, af: false, zf: false, sf: true, of: false },
            [0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0]),
        ("stored word logic", 7, 0x00008000, 0x12345678,
            Flags { cf: false, pf: true, af: false, zf: false, sf: true, of: false },
            [0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        ("stored dword logic", 11, 0x80000000, 0x12345678,
            Flags { cf: false, pf: true, af: false, zf: false, sf: true, of: false },
            [0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        ("stored byte logic ignores upper result bits", 3, 0x00000100, 0x12345678,
            Flags { cf: false, pf: true, af: false, zf: true, sf: false, of: false },
            [0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        ("stored word logic ignores upper result bits", 7, 0x00010000, 0x12345678,
            Flags { cf: false, pf: true, af: false, zf: true, sf: false, of: false },
            [0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
    ] {
        let record = StoredFlags {
            kind, left, right,
            status: StatusFlags { cf: 1, pf: 1, af: 1, zf: 1, sf: 1, of: 1 },
            non_status: [0, 1, 0, 0, 0, 0xa5],
            ..CpuState::filled(0xa5).flags
        };
        cases.push(SequenceCase::new(name, flags).stored_flags(record).conditions(conditions));
    }
    cases.push(SequenceCase::new("concrete equal flags ignore stale recipe operands",
        Flags { cf: false, pf: true, af: false, zf: true, sf: false, of: false })
        .stored_flags(StoredFlags {
            kind: 0,
            status: StatusFlags { cf: 0, pf: 1, af: 0, zf: 1, sf: 0, of: 0 },
            non_status: [0, 1, 0, 0, 0, 0xa5],
            ..CpuState::filled(0xa5).flags
        })
        .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]));
    cases
}

test_sequences!(incoming_recipe_conditions, saved_conditions());
