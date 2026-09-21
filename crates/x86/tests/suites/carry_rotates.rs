//! Carry-ring boundaries, incoming carry and conditional flag effects.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

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
    ..CARRY_CLEAR
};

// OF is the project's zero policy for masked counts above one.
#[rustfmt::skip]
fn implicit_one_cases() -> Vec<Case> {
    vec![
        Case::new("RCL AL,1; CF=0", &[0xd0, 0xd0], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_2202),
        Case::new("RCL AX,1; CF=0", &[0x66, 0xd1, 0xd0], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_0002),
        Case::new("RCL EAX,1; CF=0", &[0xd1, 0xd0], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0x0000_0002),
        Case::new("RCL AL,1; CF=1", &[0xd0, 0xd0], CARRY_SET, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_2203),
        Case::new("RCL AX,1; CF=1", &[0x66, 0xd1, 0xd0], CARRY_SET, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_0003),
        Case::new("RCL EAX,1; CF=1", &[0xd1, 0xd0], CARRY_SET, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0x0000_0003),
        Case::new("RCR AL,1; CF=0", &[0xd0, 0xd8], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_2240),
        Case::new("RCR AX,1; CF=0", &[0x66, 0xd1, 0xd8], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_4000),
        Case::new("RCR EAX,1; CF=0", &[0xd1, 0xd8], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0x4000_0000),
        Case::new("RCR AL,1; CF=1", &[0xd0, 0xd8], CARRY_SET, Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_22c0),
        Case::new("RCR AX,1; CF=1", &[0x66, 0xd1, 0xd8], CARRY_SET, Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_c000),
        Case::new("RCR EAX,1; CF=1", &[0xd1, 0xd8], CARRY_SET, Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0xc000_0000),
        Case::new("RCL AL,1; zero input", &[0xd0, 0xd0], CARRY_SET, Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2200, 0x4433_2201),
        Case::new("RCR AL,1; zero input", &[0xd0, 0xd8], CARRY_SET, Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2200, 0x4433_2280),
        Case::new("RCL AL,1; all bits set", &[0xd0, 0xd0], CARRY_CLEAR, Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_22ff, 0x4433_22fe),
        Case::new("RCR AL,1; all bits set", &[0xd0, 0xd8], CARRY_CLEAR, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_22ff, 0x4433_227f),
    ]
}
test_cases!(implicit_one_forms, implicit_one_cases());

#[rustfmt::skip]
fn alias_cases() -> Vec<Case> {
    vec![
        Case::new("RCL CL,CL; CF=0", &[0xd2, 0xd1], CARRY_CLEAR, Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_6603, 0x8877_6618),
        Case::new("RCR CH,CL; CF=0", &[0xd2, 0xdd], CARRY_CLEAR, Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_8001, 0x8877_4001),
        Case::new("RCL CX,CL; CF=1", &[0x66, 0xd3, 0xd1], CARRY_SET, Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_8001, 0x8877_0003),
        Case::new("RCR ECX,CL; CF=1", &[0xd3, 0xd9], CARRY_SET, Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Ecx, 0x8000_0001, 0xc000_0000),
    ]
}
test_cases!(count_and_operand_aliases, alias_cases());

#[rustfmt::skip]
fn count_boundaries() -> Vec<Case> {
    vec![
        Case::new("RCL byte one below a complete carry ring", &[0xc0, 0xd0, 8], CARRY_CLEAR,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2281, 0x4433_2240),
        Case::new("RCR byte one below a complete carry ring", &[0xd2, 0xd8], CARRY_SET,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2281, 0x4433_2203).initial_register(Ecx, 8),
        Case::new("RCL word one below a complete carry ring", &[0x66, 0xc1, 0xd0, 16], CARRY_SET,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_8001, 0x4433_c000),
        Case::new("RCR word one below a complete carry ring", &[0x66, 0xd3, 0xd8], CARRY_CLEAR,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_8001, 0x4433_0002).initial_register(Ecx, 16),
        Case::new("RCL byte ring plus one uses the multiple-bit overflow policy", &[0xc0, 0xd0, 10], CARRY_CLEAR,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2281, 0x4433_2202),
        Case::new("RCR word ring plus one uses the multiple-bit overflow policy", &[0x66, 0xd3, 0xd8], CARRY_CLEAR,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_8001, 0x4433_4000).initial_register(Ecx, 18),
        Case::new("RCL byte immediate 255 masks to 31 before ring reduction", &[0xc0, 0xd0, 255], CARRY_SET,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2281, 0x4433_221c),
        Case::new("RCR byte CL=255 masks to 31 before ring reduction", &[0xd2, 0xd8], CARRY_CLEAR,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2281, 0x4433_2228).initial_register(Ecx, 0x8877_66ff),
        Case::new("RCL dword immediate 33 masks to one with defined overflow", &[0xc1, 0xd0, 33], CARRY_SET,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) }).register(Eax, 0x8000_0001, 3),
        Case::new("RCR dword CL=33 masks to one with defined overflow", &[0xd3, 0xd8], CARRY_CLEAR,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) }).register(Eax, 0x8000_0001, 0x4000_0000).initial_register(Ecx, 33),
        Case::new("RCL dword by 31 retains the incoming carry bit", &[0xc1, 0xd0, 31], CARRY_SET,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x8000_0001, 0xe000_0000),
        Case::new("RCR dword by 31 retains the incoming carry bit", &[0xd3, 0xd8], CARRY_CLEAR,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x8000_0001, 5).initial_register(Ecx, 31),
    ]
}
test_cases!(carry_ring_boundaries_and_count_masking, count_boundaries());

#[rustfmt::skip]
fn unchanged_records() -> Vec<Case> {
    // These instructions sample CF even when they preserve every flag, so use valid records.
    vec![
        Case::new("RCL one byte ring", &[0xc0, 0xd0, 9], CARRY_CLEAR, Flags::all(Preserved)),
        Case::new("RCR two byte rings", &[0xc0, 0xd8, 18], CARRY_SET, Flags::all(Preserved)),
        Case::new("RCL three byte rings", &[0xc0, 0xd0, 27], CARRY_SET, Flags::all(Preserved)),
        Case::new("RCR one word ring", &[0x66, 0xc1, 0xd8, 17], CARRY_CLEAR, Flags::all(Preserved)),
        Case::new("RCL immediate 49 masks to one word ring", &[0x66, 0xc1, 0xd0, 49], CARRY_SET, Flags::all(Preserved)),
        Case::new("RCR CL=41 masks to one byte ring", &[0xd2, 0xd8], CARRY_CLEAR, Flags::all(Preserved)).initial_register(Ecx, 41),
        Case::new("RCL CL=49 masks to one word ring", &[0x66, 0xd3, 0xd0], CARRY_SET, Flags::all(Preserved)).initial_register(Ecx, 49),
        Case::new("RCR immediate 32 masks to zero", &[0xc1, 0xd8, 32], CARRY_CLEAR, Flags::all(Preserved)),
        Case::new("RCL CL=32 masks to zero", &[0xd3, 0xd0], CARRY_SET, Flags::all(Preserved)).initial_register(Ecx, 32),
        Case::new("RCR immediate zero", &[0xc0, 0xd8, 0], CARRY_SET, Flags::all(Preserved)),
        Case::new("RCL CL=0", &[0x66, 0xd3, 0xd0], CARRY_CLEAR, Flags::all(Preserved)).initial_register(Ecx, 0),
    ]
    .into_iter()
    .map(|case| case.initial_register(Eax, 0x4433_8181).preserve_flag_record())
    .collect()
}
test_cases!(
    zero_counts_and_complete_rings_preserve_the_record,
    unchanged_records()
);

#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::new("RCL byte at the last mapped byte", &[0xd0, 0x13], CARRY_SET,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .initial_register(Ebx, 0x4fff).memory(0x4ffe, &[0x5a, 0x81], ReadWrite).expect_memory(0x4fff, &[3]),
        Case::new("RCR word samples ECX for address and count", &[0x66, 0xd3, 0x19], CARRY_CLEAR,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .initial_register(Ecx, 0x8000_4001).memory(0x8000_4000, &[0x5a, 1, 0x80, 0x5a], ReadWrite)
            .expect_memory(0x8000_4001, &[0, 0x40]),
        Case::new("RCR dword writes across scattered pages", &[0xc1, 0x1b, 31], CARRY_SET,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .initial_register(Ebx, 0x4ffe).map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[0x5a, 1, 0, 0, 0x80, 0x5a], ReadWrite).expect_memory(0x4ffe, &[7, 0, 0, 0]),
        Case::new("RCR complete word ring preserves split memory and the record", &[0x66, 0xd3, 0x1b], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record().initial_registers(&[(Ebx, 0x4fff), (Ecx, 17)])
            .memory(0x4ffe, &[0x5a, 1, 0x80, 0x5a], ReadWrite),
        Case::new("RCL zero count still requires a destination", &[0xc0, 0x13, 0], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record()
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
        Case::new("RCR masked-zero count still requires write access", &[0xc1, 0x1b, 32], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record()
            .initial_register(Ebx, 0x4000).memory(0x4000, &[1, 0, 0, 0x80], ReadOnly).fault(0x4000, 3),
        Case::new("RCL complete byte ring still requires write access", &[0xc0, 0x13, 9], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record()
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0x81], ReadOnly).fault(0x4000, 3),
        Case::new("RCR CL=0 still checks the second page", &[0x66, 0xd3, 0x1b], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record()
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0)]).memory(0x4fff, &[0x81], ReadWrite).fault(0x5000, 2),
        Case::new("RCL complete word ring checks second-page write access", &[0x66, 0xd3, 0x13], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record()
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 17)]).memory(0x4fff, &[1], ReadWrite)
            .memory(0x5000, &[0x80], ReadOnly).fault(0x5000, 3),
        Case::new("RCR second-page fault prevents flags and the whole store", &[0xd1, 0x1b], CARRY_SET, Flags::all(Preserved))
            .preserve_flag_record()
            .initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[1, 0], ReadWrite)
            .memory(0x5000, &[0, 0x80], ReadOnly).fault(0x5000, 3),
    ]
}
test_cases!(memory_effects_and_noop_write_intent, memory_cases());

#[rustfmt::skip]
fn dependencies() -> Vec<Sequence> {
    vec![
        Sequence::new("full RCL ring preserves pending ADD carry and overflow", CARRY_CLEAR)
            .initial_registers(&[(Eax, 0x4433_8180), (Edx, 0x80), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
                .register(Eax, 0x4433_8100))
            .step(Step::preserving_flags(&[0xc0, 0xd4, 9]))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 1))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc7]).register(Ebx, 0x101)),
        Sequence::new("full RCR ring preserves flags from ADD followed by INC", CARRY_CLEAR)
            .initial_registers(&[(Eax, 0x4433_817f), (Edx, 1), (Ecx, 17), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x4433_8180))
            .step(Step::new(&[0xfe, 0xc0], Flags { cf: Preserved, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
                .register(Eax, 0x4433_8181))
            .step(Step::preserving_flags(&[0x66, 0xd3, 0xd8]))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc3]))
            .step(Step::new(&[0x83, 0xd2, 0], Flags::all(Clear))),
        Sequence::new("RCR consumes pending carry and clears the count for the next RCL", CARRY_CLEAR)
            .initial_registers(&[(Eax, 0x4433_81ff), (Edx, 1), (Ecx, 0x8877_8001), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0x4433_8100))
            .step(Step::new(&[0x66, 0xd3, 0xd9], Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
                .register(Ecx, 0x8877_c000))
            .step(Step::preserving_flags(&[0xd2, 0xd4]))
            .step(Step::new(&[0x83, 0xd3, 0], Flags::all(Clear)).register(Ebx, 1)),
        Sequence::new("completed RCL effects survive a later complete-ring write fault", CARRY_SET)
            .initial_registers(&[(Ebx, 0x4000), (Edx, 0x5000)])
            .memory(0x4000, &[0, 0, 0, 0x80], ReadWrite).memory(0x5000, &[0x55], ReadOnly)
            .step(Step::new(&[0xd1, 0x13], Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
                .expect_memory(0x4000, &[1, 0, 0, 0]))
            .step(Step::preserving_flags(&[0xc0, 0x1a, 9]).fault(0x5000, 3)),
    ]
}
test_sequences!(
    pending_carry_full_rings_and_fault_publication,
    dependencies()
);

#[test]
fn carry_rotate_forms_decode_their_complete_address_and_count() {
    for group in [0x10, 0x18] {
        for code in [
            vec![0xd0, 0xc4 | group],
            vec![0xd1, 0x05 | group, 0x20, 0x40, 0, 0],
            vec![0xd2, 0xc4 | group],
            vec![0x66, 0xd3, 0xc1 | group],
            vec![0xc0, 0xc5 | group, 3],
            vec![0x66, 0xc1, 0x44 | group, 0x8b, 0xfc, 32],
        ] {
            check_length(&code);
        }
    }
}
