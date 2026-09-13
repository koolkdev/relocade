use wasm86_x86::Gpr32::{Ebx, Ecx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Preserved, Set},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

use super::STORED_FLAGS;

const INITIAL: Flags<bool> = Flags {
    cf: true,
    pf: true,
    af: false,
    zf: false,
    sf: true,
    of: true,
};

// Nonzero counts above one check the project's zero-OF policy.
// Explicit frames preserve the split-page layout and all original physical canaries.
#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::new("implicit byte rotate needs only the last mapped byte", &[0xd0, 0x03], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81])
            .expect_memory(0x4fff, &[0x03]),
        Case::new("CL count shares the full ECX address base", &[0x66, 0xd3, 0x09], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8000_4001)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x00, 0xc0]),
        Case::new("CL count also supplies a wrapping scaled index", &[0xd2, 0x44, 0x8b, 0xfc], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4010), (Ecx, 0x4000_0001)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x81, 0x5a])
            .expect_memory(0x4010, &[0x03]),
        Case::new("dword rotate crosses noncontiguous pages", &[0xc1, 0x0b, 0x1f], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x00])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x00, 0x80, 0x5a])
            .expect_memory(0x4ffe, &[0x03, 0x00, 0x00, 0x00]),
        Case::new("large byte rotate preserves the width of the ring", &[0xc0, 0x0b, 0xff], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4011)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8010, &[0x5a, 0x7f, 0x5a])
            .expect_memory(0x4011, &[0xfe]),
        Case::preserving_flags("masked-zero byte count retains memory and the complete flags record", &[0xc0, 0x0b, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81]),
        Case::preserving_flags("zero CL count checks both word pages without changing state", &[0x66, 0xd3, 0x0b])
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
        Case::preserving_flags("zero byte count still requires a present page", &[0xc0, 0x03, 0x00])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6600)])
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 2),
        Case::preserving_flags("masked-zero dword count still requires write permission", &[0xc1, 0x0b, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("a full byte turn still requires write permission", &[0xc0, 0x03, 0x08])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6608)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("zero CL count checks a missing second word page", &[0x66, 0xd3, 0x0b])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 2),
        Case::preserving_flags("nonzero rotate checks a read-only second dword page", &[0xd1, 0x03])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6601)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("word wrap reaches an absent page zero", &[0x66, 0xc1, 0x0b, 0x01])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_ffff), (Ecx, 0x8877_6601)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
        Case::preserving_flags("dword wrap reaches an absent page zero", &[0xd3, 0x0b])
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
