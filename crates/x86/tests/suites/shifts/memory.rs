use wasm86_x86::Gpr32::{Ebx, Ecx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

use super::STORED_FLAGS;

// Nonzero shifts check zero AF and zero OF for masked counts above one.
// Explicit frames preserve the split-page layout and all original physical canaries.
#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("implicit byte shift needs only the last mapped byte", &[0xd0, 0x23],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81])
            .expect_memory(0x4fff, &[0x02]),
        Case::replacing_flags("CL count shares the full ECX address base", &[0x66, 0xd3, 0x29],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8000_4001)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x00, 0x40]),
        Case::replacing_flags("CL count also supplies a wrapping scaled index", &[0xd2, 0x64, 0x8b, 0xfc],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4010), (Ecx, 0x4000_0001)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x81, 0x5a])
            .expect_memory(0x4010, &[0x02]),
        Case::replacing_flags("dword arithmetic shift crosses noncontiguous pages", &[0xc1, 0x3b, 0x1f],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x00])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x00, 0x80, 0x5a])
            .expect_memory(0x4ffe, &[0xff, 0xff, 0xff, 0xff]),
        Case::replacing_flags("large arithmetic byte shift retains a positive sign", &[0xc0, 0x3b, 0xff],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4011)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8010, &[0x5a, 0x7f, 0x5a])
            .expect_memory(0x4011, &[0x00]),
        Case::preserving_flags("masked-zero byte count retains memory and the complete flags record", &[0xc0, 0x2b, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81]),
        Case::preserving_flags("zero CL count checks both word pages without changing state", &[0x66, 0xd3, 0x3b])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
    ]
}

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("zero byte count still requires a present page", &[0xc0, 0x23, 0x00])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6600)])
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 2),
        Case::preserving_flags("masked-zero dword count still requires write permission", &[0xc1, 0x2b, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("zero CL count checks a missing second word page", &[0x66, 0xd3, 0x3b])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 2),
        Case::preserving_flags("nonzero shift checks a read-only second dword page", &[0xd1, 0x23])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6601)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("word wrap reaches an absent page zero", &[0x66, 0xc1, 0x2b, 0x01])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_ffff), (Ecx, 0x8877_6601)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
        Case::preserving_flags("dword wrap reaches an absent page zero", &[0xd3, 0x3b])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_fffd), (Ecx, 0x8877_66ff)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
    ]
}

test_cases!(memory_operands, memory_cases());
test_cases!(write_faults_preserve_all_state, fault_cases());
