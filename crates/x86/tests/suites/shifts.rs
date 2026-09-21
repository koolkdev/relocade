//! Shift counts, signed saturation and conditional flag effects.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ebx, Ecx, Edx},
};

// AF is the project's zero policy; OF is zero for masked counts above one.
#[rustfmt::skip]
fn implicit_one_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL AL,1; input 81", &[0xd0, 0xe0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_2281, 0x4433_2202),
        Case::replacing_flags("SHL AX,1; input 8001", &[0x66, 0xd1, 0xe0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_8001, 0x4433_0002),
        Case::replacing_flags("SHL EAX,1; input 80000001", &[0xd1, 0xe0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0001, 0x0000_0002),
        Case::replacing_flags("SHR AL,1; input 81", &[0xd0, 0xe8],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_2281, 0x4433_2240),
        Case::replacing_flags("SHR AX,1; input 8001", &[0x66, 0xd1, 0xe8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_8001, 0x4433_4000),
        Case::replacing_flags("SHR EAX,1; input 80000001", &[0xd1, 0xe8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0001, 0x4000_0000),
        Case::replacing_flags("SAR AL,1; input 81", &[0xd0, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2281, 0x4433_22c0),
        Case::replacing_flags("SAR AX,1; input 8001", &[0x66, 0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_c000),
        Case::replacing_flags("SAR EAX,1; input 80000001", &[0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0001, 0xc000_0000),
        Case::replacing_flags("SAR EAX,1; input 7fffffff", &[0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x7fff_ffff, 0x3fff_ffff),
    ]
}
test_cases!(implicit_one_forms, implicit_one_cases());

#[rustfmt::skip]
fn alias_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL CL,CL reads the old count", &[0xd2, 0xe1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_6603, 0x8877_6618),
        Case::replacing_flags("SHR CH,CL preserves CL", &[0xd2, 0xed],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Ecx, 0x8877_8001, 0x8877_4001),
        Case::replacing_flags("SAR CX,CL reads the old word", &[0x66, 0xd3, 0xf9],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Ecx, 0x8877_8001, 0x8877_c000),
        Case::replacing_flags("SHL ECX,CL reads both old views", &[0xd3, 0xe1],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Ecx, 0x8000_0001, 0x0000_0002),
    ]
}
test_cases!(count_and_operand_aliases, alias_cases());

#[rustfmt::skip]
fn count_boundaries() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL byte one below its width", &[0xc0, 0xe0, 7],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2281, 0x4433_2280),
        // CF is undefined at or above the width for SHL/SHR; the project chooses zero.
        Case::replacing_flags("SHL byte at its width uses the zero-carry policy", &[0xc0, 0xe0, 8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2281, 0x4433_2200),
        Case::replacing_flags("SHR byte above its width uses the zero-carry policy", &[0xd2, 0xe8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2281, 0x4433_2200).initial_register(Ecx, 9),
        Case::replacing_flags("SHR word one below its width", &[0x66, 0xc1, 0xe8, 15],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_0001),
        Case::replacing_flags("SHR word at its width uses the zero-carry policy", &[0x66, 0xd3, 0xe8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_0000).initial_register(Ecx, 16),
        Case::replacing_flags("SHL word above its width uses the zero-carry policy", &[0x66, 0xc1, 0xe0, 17],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_0000),
        Case::replacing_flags("SHL dword at the largest masked count", &[0xc1, 0xe0, 31],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0001, 0x8000_0000),
        Case::replacing_flags("SHR CL=255 masks to 31", &[0xd3, 0xe8], Flags::all(Clear))
            .register(Eax, 0x8000_0001, 1).initial_register(Ecx, 0x8877_66ff),
        Case::replacing_flags("SAR negative byte saturates at its width", &[0xc0, 0xf8, 8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2281, 0x4433_22ff),
        Case::replacing_flags("SAR negative word saturates above its width", &[0x66, 0xd3, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_ffff).initial_register(Ecx, 255),
        Case::replacing_flags("SAR dword carry comes from the last shifted bit", &[0xc1, 0xf8, 31],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0001, u32::MAX),
        Case::replacing_flags("SAR positive byte saturates to zero", &[0xc0, 0xf8, 255],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_227f, 0x4433_2200),
        Case::replacing_flags("SHL immediate 33 has count-one overflow", &[0xc1, 0xe0, 33],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0001, 2),
        Case::replacing_flags("SHR CL=33 has count-one overflow", &[0x66, 0xd3, 0xe8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_8001, 0x4433_4000).initial_register(Ecx, 33),
        Case::preserving_flags("SHL immediate zero preserves the record", &[0xc0, 0xe4, 0]),
        Case::preserving_flags("SHR immediate 32 preserves the record", &[0x66, 0xc1, 0xe8, 32]),
        Case::preserving_flags("SAR immediate 32 preserves the record", &[0xc1, 0xf8, 32]),
        Case::preserving_flags("SHL CL=32 preserves the record", &[0xd3, 0xe1]).initial_register(Ecx, 0x8877_6620),
        Case::preserving_flags("SHR CL=0 preserves the record", &[0xd2, 0xed]).initial_register(Ecx, 0x8877_6600),
        Case::preserving_flags("SAR CL=32 preserves the record", &[0x66, 0xd3, 0xf8]).initial_register(Ecx, 0x8877_6620),
    ]
}
test_cases!(masked_counts_and_width_boundaries, count_boundaries());

#[rustfmt::skip]
fn memory_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL byte at the last mapped byte", &[0xd0, 0x23],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .initial_register(Ebx, 0x4fff).memory(0x4ffe, &[0x5a, 0x81], ReadWrite).expect_memory(0x4fff, &[2]),
        Case::replacing_flags("SHR word samples ECX for both address and count", &[0x66, 0xd3, 0x29],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .initial_register(Ecx, 0x8000_4001).memory(0x8000_4000, &[0x5a, 1, 0x80, 0x5a], ReadWrite)
            .expect_memory(0x8000_4001, &[0, 0x40]),
        Case::replacing_flags("SHL uses CL with a wrapping scaled ECX address", &[0xd2, 0x64, 0x8b, 0xfc],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .initial_registers(&[(Ebx, 0x4010), (Ecx, 0x4000_0001)])
            .memory(0x400f, &[0x5a, 0x81, 0x5a], ReadWrite).expect_memory(0x4010, &[2]),
        Case::replacing_flags("SAR dword writes across scattered pages", &[0xc1, 0x3b, 31],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .initial_register(Ebx, 0x4ffe).map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .memory(0x4ffd, &[0x5a, 1, 0, 0, 0x80, 0x5a], ReadWrite).expect_memory(0x4ffe, &[0xff; 4]),
        Case::preserving_flags("SHL zero count still requires a destination", &[0xc0, 0x23, 0])
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
        Case::preserving_flags("SHR masked-zero count still requires write access", &[0xc1, 0x2b, 32])
            .initial_register(Ebx, 0x4000).memory(0x4000, &[1, 0, 0, 0x80], ReadOnly).fault(0x4000, 3),
        Case::preserving_flags("SAR CL=0 still checks the second page", &[0x66, 0xd3, 0x3b])
            .initial_registers(&[(Ebx, 0x4fff), (Ecx, 0)]).memory(0x4fff, &[0x81], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("SHL second-page write fault prevents flags and the whole store", &[0xd1, 0x23])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[1, 0], ReadWrite)
            .memory(0x5000, &[0, 0x80], ReadOnly).fault(0x5000, 3),
    ]
}
test_cases!(memory_effects_and_noop_write_intent, memory_cases());

#[rustfmt::skip]
fn dependencies() -> Vec<Sequence> {
    vec![
        Sequence::new("zero SHL keeps pending ADD overflow for SETO", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_017f), (Ecx, 32), (Edx, 1), (Ebx, 0)])
            .step(Step::new(&[0x00, 0xd0], Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x4433_0180))
            .step(Step::preserving_flags(&[0xd2, 0xe4]))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 1)),
        Sequence::new("changing CL keeps the latest nonzero shift flags", Flags::all(false))
            .initial_registers(&[(Eax, 0x4433_8111), (Ecx, 0x8877_6601), (Edx, 0xccbb_aa99)])
            .step(Step::new(&[0xd2, 0xe1], Flags::all(Clear)).register(Ecx, 0x8877_6602))
            .step(Step::new(&[0xd2, 0xe4], Flags::all(Clear)).register(Eax, 0x4433_0411))
            .step(Step::new(&[0xd2, 0xe9], Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
                .register(Ecx, 0x8877_6600))
            .step(Step::preserving_flags(&[0xd3, 0xf8]))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc2]).register(Edx, 0xccbb_aa01)),
        Sequence::new("completed SAR effects survive a later zero-count write fault", Flags::all(false))
            .initial_registers(&[(Ebx, 0x4000), (Edx, 0x5000)])
            .memory(0x4000, &[1, 0, 0, 0x80], ReadWrite).memory(0x5000, &[0x55], ReadOnly)
            .step(Step::new(&[0xd1, 0x3b], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
                .expect_memory(0x4000, &[0, 0, 0, 0xc0]))
            .step(Step::preserving_flags(&[0xc0, 0x22, 32]).fault(0x5000, 3)),
    ]
}
test_sequences!(count_dependencies_and_fault_publication, dependencies());

#[test]
fn snapshot_lengths_distinguish_implicit_cl_and_immediate_counts() {
    for code in [
        &[0xd0, 0xe4][..],
        &[0xd1, 0x2d, 0x20, 0x40, 0, 0][..],
        &[0xd2, 0xfc][..],
        &[0x66, 0xd3, 0xf9][..],
        &[0xc0, 0xed, 3][..],
        &[0x66, 0xc1, 0x64, 0x8b, 0xfc, 32][..],
    ] {
        check_length(code);
    }
}

#[test]
fn unsupported_groups_stop_before_address_or_count_bytes() {
    for opcode in [0xc0, 0xc1, 0xd0, 0xd1, 0xd2, 0xd3] {
        let code = [opcode, 0x34]; // Unsupported /6, missing SIB and possibly an immediate.
        assert!(matches!(
            compile_block_from_bytes(0x1ffe, &code, 1),
            Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual }) if actual == opcode
        ));
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffe;
        image.data(0x3ffe, &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "undocumented group six remains unsupported",
            Exit::Other(0x0008_0000_0000_1ffe | (u64::from(opcode) << 32)),
        );
    }
}
