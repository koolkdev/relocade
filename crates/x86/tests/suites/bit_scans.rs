//! Scan indices, full-source parity, zero preservation and complete source reads.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadOnly,
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{self, Eax, Ebp, Ebx, Ecx, Edi, Edx},
};

// Preserve the defined scan flags from Intel SDM 325462-089, Volume 2.
// Scan parity covers the full logical source; zero also preserves the destination.
const ZERO: Flags<FlagExpectation> = Flags {
    cf: Clear,
    pf: Set,
    af: Clear,
    zf: Set,
    sf: Clear,
    of: Clear,
};
const EVEN: Flags<FlagExpectation> = Flags { zf: Clear, ..ZERO };
const ODD: Flags<FlagExpectation> = Flags { pf: Clear, ..EVEN };

fn bit_positions() -> Vec<Case> {
    let mut cases = Vec::new();
    for bits in [16, 32] {
        for bit in 0..bits {
            for opcode in [0xbc, 0xbd] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend([0x0f, opcode, 0xc2]);
                let source = (1_u32 << bit) | if bits == 16 { 0xffff_0000 } else { 0 };
                let result = bit | if bits == 16 { 0x4433_0000 } else { 0 };
                cases.push(
                    Case::replacing_flags(format!("{opcode:02x}, bit {bit} of {bits}"), &code, ODD)
                        .register(Eax, 0x4433_a55b, result)
                        .initial_register(Edx, source),
                );
            }
        }
    }
    cases
}

fn parity_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, source, first, last, flags) in [
        (&[0x66][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[0x66][..], 0x100, 0x4433_0008, 0x4433_0008, ODD),
        (&[][..], 0x100, 8, 8, ODD),
        (&[0x66][..], 0x101, 0x4433_0000, 0x4433_0008, EVEN),
        (&[][..], 0x101, 0, 8, EVEN),
        (&[0x66][..], 0x1_0000, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[][..], 0x1_0000, 16, 16, ODD),
        (&[0x66][..], 0x1_0001, 0x4433_0000, 0x4433_0000, ODD),
        (&[][..], 0x1_0001, 0, 16, EVEN),
        (&[0x66][..], 0x1_0100, 0x4433_0008, 0x4433_0008, ODD),
        (&[][..], 0x1_0100, 8, 16, EVEN),
    ] {
        for (opcode, result) in [(0xbc, first), (0xbd, last)] {
            cases.push(Case::replacing_flags(format!("full source parity, prefix {prefix:02x?}, opcode {opcode:x}, source {source:x}"),
                &[prefix, &[0x0f, opcode, 0xc2]].concat(), flags)
                .register(Gpr32::Eax, 0x4433_a55b, result).initial_register(Gpr32::Edx, source));
        }
    }
    cases
}
test_cases!(full_source_parity, parity_cases());

#[rustfmt::skip]
fn overlapping_registers() -> Vec<Case> {
    vec![
        Case::replacing_flags("BSF AX,AX reads before replacing its low word", &[0x66, 0x0f, 0xbc, 0xc0], ODD)
            .register(Eax, 0x8001_0080, 0x8001_0007),
        Case::replacing_flags("BSR ECX,ECX scans its old full value", &[0x0f, 0xbd, 0xc9], ODD)
            .register(Ecx, 0x8001_0080, 31),
        Case::replacing_flags("zero BSR DX,DX preserves the full parent", &[0x66, 0x0f, 0xbd, 0xd2], ZERO)
            .register(Edx, 0xffff_0000, 0xffff_0000),
    ]
}

#[rustfmt::skip]
fn memory_sources() -> Vec<Case> {
    vec![
        Case::replacing_flags("BSF word ends at the last readable byte", &[0x66, 0x0f, 0xbc, 0x03], EVEN)
            .register(Eax, 0x4433_a55b, 0x4433_0003).initial_register(Ebx, 0x4ffe)
            .memory(0x4ffe, &[8, 0x80], ReadOnly),
        Case::replacing_flags("BSR dword ends at the last readable byte", &[0x0f, 0xbd, 0x03], EVEN)
            .register(Eax, 0x4433_a55b, 31).initial_register(Ebx, 0x4ffc)
            .memory(0x4ffc, &[8, 0, 0, 0x80], ReadOnly),
        Case::replacing_flags("BSF reads the full scattered dword before scanning", &[0x0f, 0xbc, 0x03], EVEN)
            .register(Eax, 0x4433_a55b, 3).initial_register(Ebx, 0x4ffe)
            .map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly)
            .memory(0x4ffe, &[8, 0, 0, 0x80], ReadOnly),
        Case::replacing_flags("zero BSR word preserves the destination after a split read", &[0x66, 0x0f, 0xbd, 0x03], ZERO)
            .register(Eax, 0x4433_a55b, 0x4433_a55b).initial_register(Ebx, 0x4fff)
            .memory(0x4fff, &[0, 0], ReadOnly),
        Case::replacing_flags("BSF AX,[EAX] uses the old full address", &[0x66, 0x0f, 0xbc, 0x00], EVEN)
            .register(Eax, 0x8000_4020, 0x8000_0003).memory(0x8000_4020, &[8, 0x80], ReadOnly),
        Case::replacing_flags("BSR ECX,[EBX+ECX*4-4] uses the old index", &[0x0f, 0xbd, 0x4c, 0x8b, 0xfc], ODD)
            .register(Ecx, 0x4000_0001, 31).initial_register(Ebx, 0x4010)
            .memory(0x4010, &[8, 0x80, 0, 0x80], ReadOnly),
    ]
}

#[rustfmt::skip]
fn source_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("BSF dword missing source", &[0x0f, 0xbc, 0x03])
            .initial_register(Ebx, 0x4020).fault(0x4020, 0),
        Case::preserving_flags("BSR word missing source", &[0x66, 0x0f, 0xbd, 0x03])
            .initial_register(Ebx, 0x4020).fault(0x4020, 0),
        Case::preserving_flags("BSF word must read past an early set bit", &[0x66, 0x0f, 0xbc, 0x03])
            .initial_register(Ebx, 0x4fff).memory(0x4fff, &[1], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("BSR dword must read the absent high half", &[0x0f, 0xbd, 0x03])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffe, &[1, 0], ReadOnly).fault(0x5000, 0),
    ]
}

#[test]
fn scans_consume_a_register_or_memory_source_without_an_immediate() {
    for opcode in [0xbc, 0xbd] {
        for code in [
            vec![0x0f, opcode, 0xc2],
            vec![0x66, 0x0f, opcode, 0xed],
            vec![0x0f, opcode, 0x03],
            vec![0x66, 0x0f, opcode, 0x05, 0x20, 0x40, 0, 0],
            vec![0x0f, opcode, 0x44, 0x8b, 0xfc],
        ] {
            check_length(&code);
        }
    }
}

#[test]
fn f3_prefixed_count_instructions_are_not_accepted_as_bit_scans() {
    for opcode in [0xbc, 0xbd] {
        let code = [0xf3, 0x0f, opcode, 0x03];
        assert!(matches!(
            compile_block_from_bytes(0x1000, &code, 1),
            Err(BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode: 0xf3
            })
        ));
        Image::new(&code).check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "F3 count encoding is rejected before its unmapped source",
            Exit::Other(0x0008_00f3_0000_1000),
        );
    }
}

#[rustfmt::skip]
fn dependent_scans() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("word and dword scans replace pending flags and publish before a later fault")
            .initial_registers(&[(Eax, 0x4433_a55b), (Ebx, 0xccbb_00ff), (Edx, 0x8000_8001), (Ebp, 0x5001)])
            .step(Step::new(&[0x80, 0xc3, 1],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Ebx, 0xccbb_0000))
            .step(Step::new(&[0x66, 0x0f, 0xbc, 0xc2], EVEN).register(Eax, 0x4433_0000))
            .step(Step::preserving_flags(&[0x0f, 0x9a, 0xc4]).register(Eax, 0x4433_0100))
            .step(Step::new(&[0x0f, 0xbd, 0xfa], ODD).register(Edi, 31))
            .step(Step::preserving_flags(&[0x0f, 0x9b, 0xc3]).register(Ebx, 0xccbb_0001))
            .step(Step::preserving_flags(&[0x0f, 0xbd, 0x6d, 0]).fault(0x5001, 0)),
        Sequence::from_opaque_flags("a zero scan preserves the latest AX write for a later self scan")
            .initial_registers(&[(Eax, 0x4433_a55b), (Edx, 0)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0, 1]).register(Eax, 0x4433_0100))
            .step(Step::new(&[0x66, 0x0f, 0xbc, 0xc2], ZERO))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc2]).register(Edx, 1))
            .step(Step::new(&[0x66, 0x0f, 0xbd, 0xc0], ODD).register(Eax, 0x4433_0008)),
    ]
}

test_cases!(every_scan_bit_position, bit_positions());
test_cases!(overlapping_source_and_destination, overlapping_registers());
test_cases!(memory_widths_and_address_dependencies, memory_sources());
test_cases!(
    source_faults_preserve_destinations_and_flags,
    source_faults()
);
test_sequences!(pending_results_aliases_and_faults, dependent_scans());
