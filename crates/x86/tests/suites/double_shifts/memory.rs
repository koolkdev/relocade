use wasm86_x86::Gpr32::{Ebx, Ecx, Edx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

use super::STORED_FLAGS;

// Defined-result shifts check zero AF and zero OF for masked counts above one.
// Explicit frames preserve the split-page layout and all original physical canaries.
#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD word ends at the last mapped byte", &[0x66, 0x0f, 0xa4, 0x13, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6610), (Edx, 0xccbb_a55a), (Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x80])
            .expect_memory(0x4ffe, &[0x5a, 0xa5]),
        Case::replacing_flags("SHRD word ends at the last mapped byte", &[0x66, 0x0f, 0xac, 0x13, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6610), (Edx, 0xccbb_a55a), (Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x80])
            .expect_memory(0x4ffe, &[0x5a, 0xa5]),
        Case::replacing_flags("SHLD dword ends at the last mapped byte", &[0x0f, 0xa5, 0x13],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_661f), (Edx, 0xccbb_a55a), (Ebx, 0x4ffc)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffb, &[0x5a, 0x01, 0x00, 0x00, 0x80])
            .expect_memory(0x4ffc, &[0xad, 0xd2, 0x5d, 0xe6]),
        Case::replacing_flags("SHRD dword ends at the last mapped byte", &[0x0f, 0xad, 0x13],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_661f), (Edx, 0xccbb_a55a), (Ebx, 0x4ffc)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffb, &[0x5a, 0x01, 0x00, 0x00, 0x80])
            .expect_memory(0x4ffc, &[0xb5, 0x4a, 0x77, 0x99]),
        Case::replacing_flags("SHLD word spans noncontiguous pages", &[0x66, 0x0f, 0xa5, 0x13],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6610), (Edx, 0xccbb_a55a), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a])
            .expect_memory(0x4fff, &[0x5a, 0xa5]),
        Case::replacing_flags("SHRD word spans noncontiguous pages", &[0x66, 0x0f, 0xad, 0x13],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6610), (Edx, 0xccbb_a55a), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a])
            .expect_memory(0x4fff, &[0x5a, 0xa5]),
        Case::replacing_flags("SHLD dword spans noncontiguous pages", &[0x0f, 0xa4, 0x13, 0x01],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6601), (Edx, 0xccbb_a55a), (Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x00])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x00, 0x80, 0x5a])
            .expect_memory(0x4ffe, &[0x03, 0x00, 0x00, 0x00]),
        Case::replacing_flags("SHRD dword spans noncontiguous pages", &[0x0f, 0xac, 0x13, 0x01],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6601), (Edx, 0xccbb_a55a), (Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x00])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x00, 0x80, 0x5a])
            .expect_memory(0x4ffe, &[0x00, 0x00, 0x00, 0x40]),
        Case::replacing_flags("SHLD CL count shares the full ECX address base", &[0x66, 0x0f, 0xa5, 0x11],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8000_4001), (Edx, 0xccbb_a55a)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x03, 0x00]),
        Case::replacing_flags("SHRD CL count shares the full ECX address base", &[0x66, 0x0f, 0xad, 0x11],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8000_4001), (Edx, 0xccbb_a55a)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x00, 0x40]),
        Case::replacing_flags("SHLD source supplies the address base", &[0x0f, 0xa4, 0x1b, 0x10],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6610), (Edx, 0xccbb_a55a), (Ebx, 0x4010)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a])
            .expect_memory(0x4010, &[0x00, 0x00, 0x78, 0x56]),
        Case::replacing_flags("SHRD source supplies the address base", &[0x0f, 0xac, 0x1b, 0x10],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6610), (Edx, 0xccbb_a55a), (Ebx, 0x4010)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a])
            .expect_memory(0x4010, &[0x34, 0x12, 0x10, 0x40]),
        Case::replacing_flags("SHLD source count and address all share ECX", &[0x0f, 0xa5, 0x09],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8000_4001), (Edx, 0xccbb_a55a)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x00, 0x00, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x03, 0x00, 0x00, 0x00]),
        Case::replacing_flags("SHRD source count and address all share ECX", &[0x0f, 0xad, 0x09],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8000_4001), (Edx, 0xccbb_a55a)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x00, 0x00, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x00, 0x00, 0x00, 0xc0]),
        Case::replacing_flags("SHLD CL count supplies a wrapping scaled index", &[0x0f, 0xa5, 0x74, 0x8b, 0xfc],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x4000_0001), (Edx, 0xccbb_a55a), (Ebx, 0x4010), (wasm86_x86::Gpr32::Esi, 0x7777_7777)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x01, 0x00, 0x00, 0x80, 0x5a])
            .expect_memory(0x4010, &[0x02, 0x00, 0x00, 0x00]),
        Case::replacing_flags("SHRD CL count supplies a wrapping scaled index", &[0x0f, 0xad, 0x74, 0x8b, 0xfc],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x4000_0001), (Edx, 0xccbb_a55a), (Ebx, 0x4010), (wasm86_x86::Gpr32::Esi, 0x7777_7777)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x01, 0x00, 0x00, 0x80, 0x5a])
            .expect_memory(0x4010, &[0x00, 0x00, 0x00, 0xc0]),
        Case::preserving_flags("SHLD masked-zero dword preserves raw flags", &[0x0f, 0xa4, 0x13, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6620), (Edx, 0xccbb_a55a), (Ebx, 0x4ffc)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffb, &[0x5a, 0x01, 0x00, 0x00, 0x80]),
        Case::preserving_flags("SHRD masked-zero dword preserves raw flags", &[0x0f, 0xac, 0x13, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6620), (Edx, 0xccbb_a55a), (Ebx, 0x4ffc)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffb, &[0x5a, 0x01, 0x00, 0x00, 0x80]),
        Case::preserving_flags("SHLD zero CL count checks both word pages", &[0x66, 0x0f, 0xa5, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6600), (Edx, 0xccbb_a55a), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
        Case::preserving_flags("SHRD zero CL count checks both word pages", &[0x66, 0x0f, 0xad, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6600), (Edx, 0xccbb_a55a), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
    ]
}

// Counts beyond the word width use the project's zero-result/zero-CF-AF-OF policy.
#[rustfmt::skip]
fn undefined_count_policy_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD undefined word count follows the zero-result policy", &[0x66, 0x0f, 0xa4, 0x13, 0x11],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6611), (Edx, 0xccbb_a55a), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a])
            .expect_memory(0x4fff, &[0x00, 0x00]),
        Case::replacing_flags("SHRD undefined word count follows the zero-result policy", &[0x66, 0x0f, 0xac, 0x13, 0x11],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ecx, 0x8877_6611), (Edx, 0xccbb_a55a), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a])
            .expect_memory(0x4fff, &[0x00, 0x00]),
    ]
}

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("SHLD zero word count needs a present page", &[0x66, 0x0f, 0xa4, 0x13, 0x00])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6600)])
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 2),
        Case::preserving_flags("SHRD zero word count needs a present page", &[0x66, 0x0f, 0xac, 0x13, 0x00])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6600)])
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 2),
        Case::preserving_flags("SHLD masked-zero dword count needs write permission", &[0x0f, 0xa4, 0x13, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("SHRD masked-zero dword count needs write permission", &[0x0f, 0xac, 0x13, 0x20])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("SHLD undefined word count still needs write permission", &[0x66, 0x0f, 0xa4, 0x13, 0x11])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("SHRD undefined word count still needs write permission", &[0x66, 0x0f, 0xac, 0x13, 0x11])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("SHLD zero CL count checks the missing second word page", &[0x66, 0x0f, 0xa5, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 2),
        Case::preserving_flags("SHRD zero CL count checks the missing second word page", &[0x66, 0x0f, 0xad, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 2),
        Case::preserving_flags("SHLD masked-zero CL count checks a read-only second page", &[0x0f, 0xa5, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("SHRD masked-zero CL count checks a read-only second page", &[0x0f, 0xad, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("SHLD count at word width checks a read-only second page", &[0x66, 0x0f, 0xa5, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6610)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("SHRD count at word width checks a read-only second page", &[0x66, 0x0f, 0xad, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6610)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("SHLD nonzero dword count checks a read-only second page", &[0x0f, 0xa4, 0x13, 0x01])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6601)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("SHRD nonzero dword count checks a read-only second page", &[0x0f, 0xac, 0x13, 0x01])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6601)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("SHLD the first word page fails before the second page", &[0x66, 0x0f, 0xa4, 0x13, 0x11])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4fff, 3),
        Case::preserving_flags("SHRD the first word page fails before the second page", &[0x66, 0x0f, 0xac, 0x13, 0x11])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4fff, 3),
        Case::preserving_flags("SHLD word operand range cannot wrap", &[0x66, 0x0f, 0xa4, 0x13, 0x11])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_ffff), (Ecx, 0x8877_6611)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0xffff_ffff, 2),
        Case::preserving_flags("SHRD word operand range cannot wrap", &[0x66, 0x0f, 0xac, 0x13, 0x11])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_ffff), (Ecx, 0x8877_6611)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0xffff_ffff, 2),
        Case::preserving_flags("SHLD dword operand range cannot wrap", &[0x0f, 0xa5, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_fffd), (Ecx, 0x8877_66ff)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0xffff_fffd, 2),
        Case::preserving_flags("SHRD dword operand range cannot wrap", &[0x0f, 0xad, 0x13])
            .stored_flags(STORED_FLAGS)
            .initial_registers(&[(Ebx, 0xffff_fffd), (Ecx, 0x8877_66ff)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0xffff_fffd, 2),
    ]
}

test_cases!(memory_operands, memory_cases());
test_cases!(write_faults_preserve_all_state, fault_cases());
test_cases!(undefined_word_count_policy, undefined_count_policy_cases());
