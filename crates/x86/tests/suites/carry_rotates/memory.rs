use wasm86_x86::Gpr32::{Ebx, Ecx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Preserved, Set},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

use super::stored_flags;

const CARRY_CLEAR: Flags<bool> = Flags {
    cf: false,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};
const CARRY_SET: Flags<bool> = Flags {
    cf: true,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};

// Nonzero effective counts check zero OF when the masked count exceeds one.
// Explicit frames preserve the split-page layout and all original physical canaries.
#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::new("implicit byte carry rotate uses the last mapped byte; CF=0", &[0xd0, 0x13], CARRY_CLEAR,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_6601), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81])
            .expect_memory(0x4fff, &[0x02]),
        Case::new("implicit byte carry rotate uses the last mapped byte; CF=1", &[0xd0, 0x13], CARRY_SET,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_6601), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81])
            .expect_memory(0x4fff, &[0x03]),
        Case::new("CL count shares the old full ECX address base; CF=0", &[0x66, 0xd3, 0x19], CARRY_CLEAR,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8000_4001)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x00, 0x40]),
        Case::new("CL count shares the old full ECX address base; CF=1", &[0x66, 0xd3, 0x19], CARRY_SET,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8000_4001)])
            .map_page(0x0008_0004, 0x8000, ReadWrite)
            .backing(0x8000, &[0x5a, 0x01, 0x80, 0x5a])
            .expect_memory(0x8000_4001, &[0x00, 0xc0]),
        Case::new("CL count supplies an old wrapping scaled index; CF=0", &[0xd2, 0x54, 0x8b, 0xfc], CARRY_CLEAR,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x4000_0001), (Ebx, 0x4010)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x81, 0x5a])
            .expect_memory(0x4010, &[0x02]),
        Case::new("CL count supplies an old wrapping scaled index; CF=1", &[0xd2, 0x54, 0x8b, 0xfc], CARRY_SET,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x4000_0001), (Ebx, 0x4010)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x800f, &[0x5a, 0x81, 0x5a])
            .expect_memory(0x4010, &[0x03]),
        Case::new("dword carry rotate spans noncontiguous pages; CF=0", &[0xc1, 0x1b, 0x1f], CARRY_CLEAR,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_661f), (Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x00])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x00, 0x80, 0x5a])
            .expect_memory(0x4ffe, &[0x05, 0x00, 0x00, 0x00]),
        Case::new("dword carry rotate spans noncontiguous pages; CF=1", &[0xc1, 0x1b, 0x1f], CARRY_SET,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_661f), (Ebx, 0x4ffe)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffd, &[0x5a, 0x01, 0x00])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x00, 0x80, 0x5a])
            .expect_memory(0x4ffe, &[0x07, 0x00, 0x00, 0x00]),
        Case::new("large byte carry rotate retains its nine-bit ring; CF=0", &[0xc0, 0x1b, 0xff], CARRY_CLEAR,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_66ff), (Ebx, 0x4011)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8010, &[0x5a, 0x7f, 0x5a])
            .expect_memory(0x4011, &[0xe7]),
        Case::new("large byte carry rotate retains its nine-bit ring; CF=1", &[0xc0, 0x1b, 0xff], CARRY_SET,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_66ff), (Ebx, 0x4011)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8010, &[0x5a, 0x7f, 0x5a])
            .expect_memory(0x4011, &[0xf7]),
        Case::preserving_flags("masked-zero memory count preserves raw flags; CF=0", &[0xc0, 0x1b, 0x20])
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_6620), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81]),
        Case::preserving_flags("masked-zero memory count preserves raw flags; CF=1", &[0xc0, 0x1b, 0x20])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_6620), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81]),
        Case::preserving_flags("zero CL count checks both word pages; CF=0", &[0x66, 0xd3, 0x13])
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_6600), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
        Case::preserving_flags("zero CL count checks both word pages; CF=1", &[0x66, 0xd3, 0x13])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_6600), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
        Case::preserving_flags("complete byte carry ring preserves the operand and raw flags; CF=0", &[0xc0, 0x13, 0x09])
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_6609), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81]),
        Case::preserving_flags("complete byte carry ring preserves the operand and raw flags; CF=1", &[0xc0, 0x13, 0x09])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_6609), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x81]),
        Case::preserving_flags("complete word carry ring checks both mapped pages; CF=0", &[0x66, 0xd3, 0x1b])
            .stored_flags(stored_flags(0))
            .initial_registers(&[(Ecx, 0x8877_6611), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
        Case::preserving_flags("complete word carry ring checks both mapped pages; CF=1", &[0x66, 0xd3, 0x1b])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ecx, 0x8877_6611), (Ebx, 0x4fff)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8ffe, &[0x5a, 0x01])
            .map_page(0x0005, 0xa000, ReadWrite)
            .backing(0xa000, &[0x80, 0x5a]),
    ]
}

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("RCL zero byte count needs a present page", &[0xc0, 0x13, 0x00])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6600)])
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 2),
        Case::preserving_flags("RCR zero byte count needs a present page", &[0xc0, 0x1b, 0x00])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6600)])
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 2),
        Case::preserving_flags("RCL masked-zero dword count needs write permission", &[0xc1, 0x13, 0x20])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("RCR masked-zero dword count needs write permission", &[0xc1, 0x1b, 0x20])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6620)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("RCL complete byte carry ring needs write permission", &[0xc0, 0x13, 0x09])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6609)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("RCR complete byte carry ring needs write permission", &[0xc0, 0x1b, 0x09])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 0x8877_6609)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4020, 3),
        Case::preserving_flags("RCL zero CL count checks the missing second word page", &[0x66, 0xd3, 0x13])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 2),
        Case::preserving_flags("RCR zero CL count checks the missing second word page", &[0x66, 0xd3, 0x1b])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6600)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 2),
        Case::preserving_flags("RCL complete word carry ring checks a read-only second page", &[0x66, 0xd3, 0x13])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("RCR complete word carry ring checks a read-only second page", &[0x66, 0xd3, 0x1b])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("RCL nonzero dword count checks a read-only second page", &[0xc1, 0x13, 0x01])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6601)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("RCR nonzero dword count checks a read-only second page", &[0xc1, 0x1b, 0x01])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0x8877_6601)])
            .map_page(0x0004, 0x8000, ReadWrite)
            .map_page(0x0005, 0xa000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x5000, 3),
        Case::preserving_flags("RCL the first word page fails before the second page", &[0x66, 0xc1, 0x13, 0x11])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4fff, 3),
        Case::preserving_flags("RCR the first word page fails before the second page", &[0x66, 0xc1, 0x1b, 0x11])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0x8877_6611)])
            .map_page(0x0004, 0x8000, ReadOnly)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0x4fff, 3),
        Case::preserving_flags("RCL word operand wrap reaches an absent page zero", &[0x66, 0xc1, 0x13, 0x11])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0xffff_ffff), (Ecx, 0x8877_6611)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
        Case::preserving_flags("RCR word operand wrap reaches an absent page zero", &[0x66, 0xc1, 0x1b, 0x11])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0xffff_ffff), (Ecx, 0x8877_6611)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
        Case::preserving_flags("RCL dword operand wrap reaches an absent page zero", &[0xd3, 0x13])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0xffff_fffd), (Ecx, 0x8877_66ff)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
        Case::preserving_flags("RCR dword operand wrap reaches an absent page zero", &[0xd3, 0x1b])
            .stored_flags(stored_flags(1))
            .initial_registers(&[(Ebx, 0xffff_fffd), (Ecx, 0x8877_66ff)])
            .map_page(0x000f_ffff, 0x8000, ReadWrite)
            .backing(0x8020, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]).backing(0xa000, &[0x12, 0x5a])
            .fault(0, 2),
    ]
}

test_cases!(memory_operands, memory_cases());
test_cases!(write_faults_preserve_all_state, fault_cases());
