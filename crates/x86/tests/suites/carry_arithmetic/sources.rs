use wasm86_x86::{CpuState, Gpr32::Eax, StatusFlags, StoredFlags};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

#[rustfmt::skip]
fn stored_carry_sources() -> Vec<Case> {
    let mut cases = Vec::new();
    for (source_width, tag, maximum, high) in [
        (8, 0, 0xff, 0x7e57_ae00),
        (16, 4, 0xffff, 0x7e57_0000),
        (32, 8, 0xffff_ffff, 0),
    ] {
        for (kind, left, right, initial_flags, carry) in [
            (2, maximum, 1, Flags { cf: true, pf: true, af: true, zf: true, sf: false, of: false }, true),
            (2, 0, 1, Flags { cf: false, pf: false, af: false, zf: false, sf: false, of: false }, false),
            (1, 0, 1, Flags { cf: true, pf: true, af: true, zf: false, sf: true, of: false }, true),
            (1, maximum, 1, Flags { cf: false, pf: false, af: false, zf: false, sf: true, of: false }, false),
            (3, maximum, 0xdead_beef, Flags { cf: false, pf: true, af: false, zf: false, sf: true, of: false }, false),
        ] {
            let record = StoredFlags {
                kind: tag | kind, left: left | high, right: right | high,
                status: StatusFlags { cf: u8::from(!carry), pf: 1, af: 1, zf: 1, sf: 1, of: 1 },
                non_status: [0, 1, 0, 0, 0, 0xa5],
                ..CpuState::filled(0xa5).flags
            };
            for (name, code, input, outputs, flags) in [
                ("ADC AL,0", &[0x14, 0][..], 0x4433_2200, [0x4433_2200, 0x4433_2201],
                    [Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
                     Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }]),
                ("ADC AX,0", &[0x66, 0x15, 0, 0][..], 0x4433_0000, [0x4433_0000, 0x4433_0001],
                    [Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
                     Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }]),
                ("ADC EAX,0", &[0x15, 0, 0, 0, 0][..], 0, [0, 1],
                    [Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
                     Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }]),
                ("SBB AL,0", &[0x1c, 0][..], 0x4433_2200, [0x4433_2200, 0x4433_22ff],
                    [Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
                     Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }]),
                ("SBB AX,0", &[0x66, 0x1d, 0, 0][..], 0x4433_0000, [0x4433_0000, 0x4433_ffff],
                    [Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
                     Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }]),
                ("SBB EAX,0", &[0x1d, 0, 0, 0, 0][..], 0, [0, 0xffff_ffff],
                    [Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear },
                     Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }]),
            ] {
                cases.push(Case::new(format!("{name}; saved {source_width}-bit kind {kind}, carry {carry}"), code,
                    initial_flags, flags[usize::from(carry)])
                    .stored_flags(record).register(Eax, input, outputs[usize::from(carry)]));
            }
        }
    }
    cases
}

test_cases!(incoming_recipes, stored_carry_sources());
