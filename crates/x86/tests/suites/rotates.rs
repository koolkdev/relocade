//! Rotate counts, full turns and partial flag effects.
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

const INITIAL: Flags<bool> = Flags {
    cf: true,
    pf: true,
    af: false,
    zf: false,
    sf: true,
    of: true,
};

// OF is the project's deterministic zero policy when the masked count exceeds one.
#[rustfmt::skip]
fn implicit_one_cases() -> Vec<Case> {
    vec![
        Case::new("ROL AL,1; input 81", &[0xd0, 0xc0], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_2203),
        Case::new("ROL AX,1; input 8001", &[0x66, 0xd1, 0xc0], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_0003),
        Case::new("ROL EAX,1; input 80000001", &[0xd1, 0xc0], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0x0000_0003),
        Case::new("ROR AL,1; input 81", &[0xd0, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_22c0),
        Case::new("ROR AX,1; input 8001", &[0x66, 0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_c000),
        Case::new("ROR EAX,1; input 80000001", &[0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0xc000_0000),
        Case::new("ROL AL,1; input 40", &[0xd0, 0xc0], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2240, 0x4433_2280),
        Case::new("ROR AX,1; input 1", &[0x66, 0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_0001, 0x4433_8000),
    ]
}
test_cases!(implicit_one_forms, implicit_one_cases());

#[rustfmt::skip]
fn alias_cases() -> Vec<Case> {
    vec![
        Case::new("ROL CL,CL reads the old count", &[0xd2, 0xc1], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_6603, 0x8877_6618),
        Case::new("ROR CH,CL preserves CL", &[0xd2, 0xcd], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_8001, 0x8877_4001),
        Case::new("ROR CX,CL reads the old word", &[0x66, 0xd3, 0xc9], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_8001, 0x8877_c000),
        Case::new("ROL ECX,CL reads both old views", &[0xd3, 0xc1], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Ecx, 0x8000_0001, 0x0000_0003),
    ]
}
test_cases!(count_and_operand_aliases, alias_cases());

#[rustfmt::skip]
fn count_boundaries() -> Vec<Case> {
    vec![
        Case::new("ROL byte full turn changes flags even when the value is unchanged", &[0xc0, 0xc0, 8], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).initial_register(Eax, 0x4433_2280),
        Case::new("ROR byte full turn takes carry from the top bit", &[0xc0, 0xc8, 8], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).initial_register(Eax, 0x4433_2201),
        Case::new("ROL word full turn takes carry from the low bit", &[0x66, 0xc1, 0xc0, 16], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).initial_register(Eax, 0x4433_8001),
        Case::new("ROR word full turn through CL still updates flags", &[0x66, 0xd3, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).initial_registers(&[(Eax, 0x4433_8001), (Ecx, 16)]),
        Case::new("ROL three byte turns still update flags", &[0xd2, 0xc0], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).initial_registers(&[(Eax, 0x4433_2281), (Ecx, 24)]),
        Case::new("ROL count 9 uses the multiple-bit overflow policy", &[0xc0, 0xc0, 9], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2240, 0x4433_2280),
        Case::new("ROR count 17 uses the multiple-bit overflow policy", &[0x66, 0xd3, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_0001, 0x4433_8000).initial_register(Ecx, 17),
        Case::new("ROL immediate 33 masks to one before choosing overflow", &[0xc1, 0xc0, 33], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) }).register(Eax, 0x8000_0001, 3),
        Case::new("ROR CL=33 masks to one before choosing overflow", &[0xd3, 0xc8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) }).register(Eax, 1, 0x8000_0000).initial_register(Ecx, 0x8877_6621),
        Case::new("ROL dword by 31 keeps all operand bits", &[0xd3, 0xc0], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x8000_0001, 0xc000_0000).initial_register(Ecx, 31),
        Case::new("ROR byte immediate 255 masks to 31", &[0xc0, 0xc8, 255], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }).register(Eax, 0x4433_2281, 0x4433_2203),
        Case::preserving_flags("ROL immediate zero preserves the record", &[0xc0, 0xc4, 0]),
        Case::preserving_flags("ROR immediate 32 preserves the record", &[0x66, 0xc1, 0xc8, 32]),
        Case::preserving_flags("ROL CL=32 preserves the record", &[0xd3, 0xc1]).initial_register(Ecx, 0x8877_6620),
        Case::preserving_flags("ROR CL=0 preserves the record", &[0xd2, 0xcd]).initial_register(Ecx, 0x8877_6600),
    ]
}
test_cases!(full_turns_and_masked_counts, count_boundaries());

#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::new("ROL byte at the last mapped byte", &[0xd0, 0x03], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .initial_register(Ebx, 0x4fff).memory(0x4ffe, &[0x5a, 0x81], ReadWrite).expect_memory(0x4fff, &[3]),
        Case::new("ROR word samples ECX for address and count", &[0x66, 0xd3, 0x09], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .initial_register(Ecx, 0x8000_4001).memory(0x8000_4000, &[0x5a, 1, 0x80, 0x5a], ReadWrite)
            .expect_memory(0x8000_4001, &[0, 0xc0]),
        Case::new("ROR dword writes across scattered pages", &[0xc1, 0x0b, 31], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .initial_register(Ebx, 0x4ffe).map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[0x5a, 1, 0, 0, 0x80, 0x5a], ReadWrite).expect_memory(0x4ffe, &[3, 0, 0, 0]),
        Case::preserving_flags("ROR word CL=0 keeps the split operand and flag record", &[0x66, 0xd3, 0x0b])
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0)]).memory(0x4ffe, &[0x5a, 1, 0x80, 0x5a], ReadWrite),
        Case::preserving_flags("ROL zero count still requires a destination", &[0xc0, 0x03, 0])
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
        Case::preserving_flags("ROR masked-zero count still requires write access", &[0xc1, 0x0b, 32])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[1, 0, 0, 0x80], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("ROL full turn still requires write access", &[0xc0, 0x03, 8])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[0x81], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("ROR CL=0 still checks the second page", &[0x66, 0xd3, 0x0b])
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0)]).memory(0x4fff, &[0x81], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("ROL second-page write fault prevents flags and the whole store", &[0xd1, 0x03])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[1, 0], ReadWrite)
            .memory(0x5000, &[0, 0x80], ReadOnly).fault(0x5000, 3),
    ]
}
test_cases!(memory_effects_and_noop_write_intent, memory_cases());

#[rustfmt::skip]
fn dependencies() -> Vec<Sequence> {
    vec![
        Sequence::new("full ROL turn replaces carry while keeping pending ADD zero", INITIAL)
            .initial_registers(&[(Eax, 0x4433_80ff), (Ecx, 8), (Edx, 1), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0x4433_8000))
            .step(Step::new(&[0xd2, 0xc4], Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) }))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc3]).register(Ebx, 1))
            .step(Step::new(&[0x83, 0xda, 0], Flags::all(Clear))), // SBB EDX,0 leaves one.
        Sequence::new("ROR changes CL to zero before a rotate preserving pending flags", INITIAL)
            .initial_registers(&[(Eax, 0x4433_017f), (Edx, 1), (Ecx, 0x8877_8001), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x4433_0180))
            .step(Step::new(&[0x66, 0xd3, 0xc9], Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
                .register(Ecx, 0x8877_c000))
            .step(Step::preserving_flags(&[0xd2, 0xc4]))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 1)),
        Sequence::new("completed ROR effects survive a later masked-zero write fault", INITIAL)
            .initial_registers(&[(Ebx, 0x4000), (Edx, 0x5000)])
            .memory(0x4000, &[1, 0, 0, 0x80], ReadWrite).memory(0x5000, &[0x55], ReadOnly)
            .step(Step::new(&[0xd1, 0x0b], Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
                .expect_memory(0x4000, &[0, 0, 0, 0xc0]))
            .step(Step::preserving_flags(&[0xc0, 0x02, 32]).fault(0x5000, 3)),
    ]
}
test_sequences!(
    pending_flags_count_aliases_and_fault_publication,
    dependencies()
);

#[test]
fn snapshot_lengths_distinguish_implicit_cl_and_immediate_counts() {
    for code in [
        &[0xd0, 0xc4][..],
        &[0xd1, 0x0d, 0x20, 0x40, 0, 0][..],
        &[0xd2, 0xcc][..],
        &[0x66, 0xd3, 0xc9][..],
        &[0xc0, 0xcd, 3][..],
        &[0x66, 0xc1, 0x44, 0x8b, 0xfc, 32][..],
    ] {
        check_length(code);
    }
}
