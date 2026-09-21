//! Double-width shift inputs, count boundaries and read/modify/write effects.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

// AF is the project's zero policy; OF is zero for masked counts above one.
#[rustfmt::skip]
fn literal_results() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD EAX,EDX,1; zero result", &[0x0f, 0xa4, 0xd0, 0x01],
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x0000_0000)
            .initial_register(Edx, 0x0000_0000),
        Case::replacing_flags("SHRD EAX,EDX,1; source sets sign", &[0x0f, 0xac, 0xd0, 0x01],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0000, 0xc000_0000)
            .initial_register(Edx, 0x0000_0001),
        Case::replacing_flags("SHRD EAX,EDX,1; source clears sign", &[0x0f, 0xac, 0xd0, 0x01],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x4000_0000)
            .initial_register(Edx, 0x0000_0000),
        Case::replacing_flags("SHLD AX,DX,1; sign changes", &[0x66, 0x0f, 0xa4, 0xd0, 0x01],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_4000, 0x4433_8000)
            .initial_register(Edx, 0x0000_0000),
        Case::replacing_flags("SHLD AX,DX,16; source replaces word", &[0x66, 0x0f, 0xa4, 0xd0, 0x10],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_1234)
            .initial_register(Edx, 0x0000_1234),
        Case::replacing_flags("SHRD AX,DX,16; source replaces word", &[0x66, 0x0f, 0xac, 0xd0, 0x10],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8000, 0x4433_abcd)
            .initial_register(Edx, 0x0000_abcd),
    ]
}
test_cases!(
    source_contribution_and_count_one_overflow,
    literal_results()
);

#[rustfmt::skip]
fn alias_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD 32-bit same destination and source", &[0x0f, 0xa4, 0xc0, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xccbb_a55a, 0xa55a_ccbb)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHRD 16-bit same destination and source", &[0x66, 0x0f, 0xac, 0xc0, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xccbb_a55a, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHLD 16-bit destination contains CL", &[0x66, 0x0f, 0xa5, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x8877_001d)
            .initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHRD 32-bit destination contains CL", &[0x0f, 0xad, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x510e_f000)
            .initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHLD 32-bit source contains CL", &[0x0f, 0xa5, 0xc8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x219c_000c)
            .initial_register(Ecx, 0x8877_8003),
        Case::replacing_flags("SHRD 16-bit source contains CL", &[0x66, 0x0f, 0xad, 0xc8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_7000)
            .initial_register(Ecx, 0x8877_8003),
        Case::replacing_flags("SHLD 16-bit both operands contain CL", &[0x66, 0x0f, 0xa5, 0xc9],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x8877_001c),
        Case::replacing_flags("SHRD 32-bit both operands contain CL", &[0x0f, 0xad, 0xc9],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x710e_f000),
    ]
}
test_cases!(count_and_operand_aliases, alias_cases());

#[rustfmt::skip]
fn count_boundaries() -> Vec<Case> {
    let mut cases = vec![
        Case::replacing_flags("SHLD word one below its width", &[0x66, 0x0f, 0xa4, 0xd0, 15],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_d2ad).initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHRD word one below its width", &[0x66, 0x0f, 0xad, 0xd0], Flags::all(Clear))
            .register(Eax, 0x4433_8001, 0x4433_4ab5).initial_registers(&[(Edx, 0xccbb_a55a), (Ecx, 15)]),
        Case::replacing_flags("SHLD word count 16 takes carry from the old low bit", &[0x66, 0x0f, 0xa4, 0xd0, 16],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8000, 0x4433_a55a).initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHRD word count 16 takes carry from the old high bit", &[0x66, 0x0f, 0xad, 0xd0],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_a55a).initial_registers(&[(Edx, 0xccbb_a55a), (Ecx, 16)]),
        Case::replacing_flags("SHLD dword by 31 mixes distinct operands", &[0x0f, 0xa4, 0xd0, 31],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0001, 0xc4d5_e6f7).initial_register(Edx, 0x89ab_cdef),
        Case::replacing_flags("SHRD dword CL=255 masks to 31", &[0x0f, 0xad, 0xd0], Flags::all(Clear))
            .register(Eax, 0x8000_0001, 0x1357_9bdf).initial_registers(&[(Edx, 0x89ab_cdef), (Ecx, 0x8877_66ff)]),
        Case::replacing_flags("SHLD immediate 33 has count-one overflow", &[0x0f, 0xa4, 0xd0, 33],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0001, 2).initial_register(Edx, 0),
        Case::replacing_flags("SHRD CL=33 has count-one overflow", &[0x0f, 0xad, 0xd0],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 1, 0x8000_0000).initial_registers(&[(Edx, 1), (Ecx, 33)]),
        Case::preserving_flags("SHLD immediate zero excludes the source", &[0x66, 0x0f, 0xa4, 0xd0, 0])
            .initial_registers(&[(Eax, 0x4433_8001), (Edx, u32::MAX)]),
        Case::preserving_flags("SHRD immediate 32 preserves the record", &[0x0f, 0xac, 0xd0, 32])
            .initial_registers(&[(Eax, 0x8000_0001), (Edx, u32::MAX)]),
        Case::preserving_flags("SHLD CL=32 preserves the record", &[0x0f, 0xa5, 0xd0])
            .initial_registers(&[(Eax, 0x8000_0001), (Edx, u32::MAX), (Ecx, 32)]),
        Case::preserving_flags("SHRD CL=0 excludes the source", &[0x66, 0x0f, 0xad, 0xd0])
            .initial_registers(&[(Eax, 0x4433_8001), (Edx, u32::MAX), (Ecx, 0)]),
    ];
    // Word counts above 16 are architecturally undefined. The project chooses
    // a zero result with CF, AF and OF clear, and PF/ZF/SF derived from that zero.
    for code in [
        &[0x66, 0x0f, 0xa4, 0xd0, 17][..],
        &[0x66, 0x0f, 0xac, 0xd0, 17],
        &[0x66, 0x0f, 0xa5, 0xd0],
        &[0x66, 0x0f, 0xad, 0xd0],
    ] {
        cases.push(Case::replacing_flags(format!("word count above width: {code:02x?}"), code,
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_0000).initial_registers(&[(Edx, 0xccbb_a55a), (Ecx, 255)]));
    }
    cases
}
test_cases!(
    count_masking_width_boundaries_and_zero_policy,
    count_boundaries()
);

#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD word count 16 transfers the source to memory", &[0x66, 0x0f, 0xa4, 0x13, 16],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .initial_registers(&[(Ebx, 0x4ffe), (Edx, 0xccbb_a55a)])
            .memory(0x4ffd, &[0x5a, 1, 0x80], ReadWrite).expect_memory(0x4ffe, &[0x5a, 0xa5]),
        Case::replacing_flags("SHRD dword writes across scattered pages", &[0x0f, 0xac, 0x13, 1],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .initial_registers(&[(Ebx, 0x4ffe), (Edx, 0xccbb_a55a)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[0x5a, 1, 0, 0, 0x80, 0x5a], ReadWrite).expect_memory(0x4ffe, &[0, 0, 0, 0x40]),
        Case::replacing_flags("SHLD source register also supplies the destination address", &[0x0f, 0xa4, 0x1b, 16],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .initial_register(Ebx, 0x4010).memory(0x400f, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[0, 0, 0x78, 0x56]),
        Case::replacing_flags("SHRD samples ECX for source, count and address", &[0x0f, 0xad, 0x09],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .initial_register(Ecx, 0x8000_4001).memory(0x8000_4000, &[0x5a, 1, 0, 0, 0x80, 0x5a], ReadWrite)
            .expect_memory(0x8000_4001, &[0, 0, 0, 0xc0]),
        Case::preserving_flags("SHLD word CL=0 keeps split memory and the flag record", &[0x66, 0x0f, 0xa5, 0x13])
            .initial_registers(&[(Ebx, 0x4fff), (Edx, u32::MAX), (Ecx, 0)])
            .memory(0x4ffe, &[0x5a, 1, 0x80, 0x5a], ReadWrite),
        Case::replacing_flags("SHRD word count 17 writes the project zero result across pages", &[0x66, 0x0f, 0xac, 0x13, 17],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_registers(&[(Ebx, 0x4fff), (Edx, u32::MAX)])
            .memory(0x4ffe, &[0x5a, 1, 0x80, 0x5a], ReadWrite).expect_memory(0x4fff, &[0, 0]),
        Case::preserving_flags("SHLD zero count still requires a destination", &[0x66, 0x0f, 0xa4, 0x13, 0])
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
        Case::preserving_flags("SHRD masked-zero count still requires write access", &[0x0f, 0xac, 0x13, 32])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[1, 0, 0, 0x80], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("SHLD undefined word count still requires write access", &[0x66, 0x0f, 0xa4, 0x13, 17])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[1, 0x80], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("SHRD CL=0 still checks the second page", &[0x66, 0x0f, 0xad, 0x13])
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0)]).memory(0x4fff, &[1], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("SHLD word source transfer checks second-page write access", &[0x66, 0x0f, 0xa5, 0x13])
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 16)]).memory(0x4fff, &[1], ReadWrite)
            .memory(0x5000, &[0x80], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("SHRD second-page fault prevents flags and the whole store", &[0x0f, 0xac, 0x13, 1])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[1, 0], ReadWrite)
            .memory(0x5000, &[0, 0x80], ReadOnly).fault(0x5000, 3),
    ]
}
test_cases!(memory_effects_and_noop_write_intent, memory_cases());

#[rustfmt::skip]
fn dependencies() -> Vec<Sequence> {
    vec![
        Sequence::new("zero SHLD preserves pending ADD overflow for SETO", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_817f), (Edx, 1), (Ecx, 32), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x4433_8180))
            .step(Step::preserving_flags(&[0x0f, 0xa5, 0xd0]))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 1)),
        Sequence::new("SHLD uses a locally produced source and supplies carry to ADC", Flags::all(false))
            .initial_registers(&[(Eax, 0x8000_0001), (Edx, 0x7fff_ffff), (Ebx, 0)])
            .step(Step::new(&[0x83, 0xc2, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Edx, 0x8000_0000))
            .step(Step::new(&[0x0f, 0xa4, 0xd0, 1], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
                .register(Eax, 3))
            .step(Step::new(&[0x83, 0xd3, 0], Flags::all(Clear)).register(Ebx, 1)),
        Sequence::new("SHRD source transfer changes CL before the next SHLD", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_8001), (Ecx, 0x8877_8010), (Edx, 0xccbb_8000), (Ebx, 0)])
            .step(Step::new(&[0x66, 0x0f, 0xad, 0xd1], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
                .register(Ecx, 0x8877_8000))
            .step(Step::preserving_flags(&[0x0f, 0xa5, 0xd0]))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc3]).register(Ebx, 1)),
        Sequence::new("completed SHLD effects survive a later masked-zero write fault", Flags::all(false))
            .initial_registers(&[(Ebx, 0x4000), (Ecx, 0x5000), (Edx, 0x8000_0000)])
            .memory(0x4000, &[1, 0, 0, 0x80], ReadWrite).memory(0x5000, &[0x55; 4], ReadOnly)
            .step(Step::new(&[0x0f, 0xa4, 0x13, 1], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
                .expect_memory(0x4000, &[3, 0, 0, 0]))
            .step(Step::preserving_flags(&[0x0f, 0xac, 0x11, 32]).fault(0x5000, 3)),
    ]
}
test_sequences!(
    source_and_count_dependencies_and_fault_publication,
    dependencies()
);

#[test]
fn all_double_shift_forms_decode_their_source_address_and_count() {
    for opcode in [0xa4, 0xac] {
        for code in [
            vec![0x0f, opcode, 0xd0, 1],
            vec![0x0f, opcode + 1, 0xd0],
            vec![0x66, 0x0f, opcode, 0xfc, 16],
            vec![0x66, 0x0f, opcode + 1, 0xd1],
            vec![0x0f, opcode, 0x15, 0x20, 0x40, 0, 0, 32],
            vec![0x66, 0x0f, opcode + 1, 0x54, 0x8b, 0xfc],
        ] {
            check_length(&code);
        }
    }
}
