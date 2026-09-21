//! MOV forms, selected operand widths and fault publication.

use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32::*};

#[path = "data_moves/sequences.rs"]
mod sequences;

#[rustfmt::skip]
fn register_forms() -> Vec<Case> {
    vec![
        Case::preserving_flags("88 copies a high byte into another low byte", &[0x88, 0xe1])
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_6655, 0x8877_6622),
        Case::preserving_flags("8A reads AL before replacing AH in the same parent", &[0x8a, 0xe0])
            .register(Eax, 0x4433_2211, 0x4433_1111),
        Case::preserving_flags("89 copies a word and preserves the destination upper half", &[0x66, 0x89, 0xc4])
            .initial_register(Eax, 0x4433_2211).register(Esp, 0x7654_3210, 0x7654_2211),
        Case::preserving_flags("8B selects the word destination from reg", &[0x66, 0x8b, 0xef])
            .initial_register(Edi, 0x89ab_cdef).register(Ebp, 0xfedc_ba98, 0xfedc_cdef),
        Case::preserving_flags("89 selects the dword destination from r/m", &[0x89, 0xd9])
            .initial_register(Ebx, 0x9234_5678).register(Ecx, 0x8877_6655, 0x9234_5678),
        Case::preserving_flags("8B selects the dword destination from reg", &[0x8b, 0xd9])
            .initial_register(Ecx, 0x8877_6655).register(Ebx, 0x9234_5678, 0x8877_6655),
        Case::preserving_flags("B0 immediate replaces only AL", &[0xb0, 0x80])
            .register(Eax, 0x4433_2211, 0x4433_2280),
        Case::preserving_flags("B4 opcode-looking immediate replaces only AH", &[0xb4, 0x88])
            .register(Eax, 0x4433_2211, 0x4433_8811),
        Case::preserving_flags("B8 word form consumes two immediate bytes", &[0x66, 0xbd, 0xa1, 0xc7])
            .register(Ebp, 0xfedc_ba98, 0xfedc_c7a1),
        Case::preserving_flags("B8 dword form consumes opcode-looking immediate bytes", &[0xbc, 0xf3, 0x0f, 0xb8, 0x66])
            .register(Esp, 0x7654_3210, 0x66b8_0ff3),
        Case::preserving_flags("C6 group immediate addresses AH", &[0xc6, 0xc4, 0xc6])
            .register(Eax, 0x4433_2211, 0x4433_c611),
        Case::preserving_flags("C7 word group immediate preserves upper bits", &[0x66, 0xc7, 0xc7, 0x8a, 0xb8])
            .register(Edi, 0x89ab_cdef, 0x89ab_b88a),
        Case::preserving_flags("C7 dword group immediate replaces all bits", &[0xc7, 0xc6, 0xa0, 0x66, 0xc7, 0x88])
            .register(Esi, 0x0123_4567, 0x88c7_66a0),
    ]
}

#[rustfmt::skip]
fn memory_forms() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AH,[EAX]: high destination overlaps base", &[0x8a, 0x20])
            .register(Eax, 0x4000, 0x8000).memory(0x3fff, &[0xa5, 0x80, 0x5a], ReadWrite),
        Case::preserving_flags("MOV [EAX],AH: high source overlaps base", &[0x88, 0x20])
            .initial_register(Eax, 0x4020).memory(0x401f, &[0xa5, 0x80, 0x5a], ReadWrite)
            .expect_memory(0x4020, &[0x40]),
        Case::preserving_flags("MOV DL reads only the final byte of a mapped page", &[0x8a, 0x13])
            .initial_register(Ebx, 0x4fff).register(Edx, 0xccbb_aa99, 0xccbb_aa80)
            .memory(0x4ffe, &[0xa5, 0x80], ReadOnly),
        Case::preserving_flags("MOV DL store touches only the final mapped byte", &[0x88, 0x13])
            .initial_registers(&[(Ebx, 0x4fff), (Edx, 0xccbb_aa99)])
            .memory(0x4ffe, &[0xa5, 0x80], ReadWrite).expect_memory(0x4fff, &[0x99]),
        Case::preserving_flags("MOV AX reads the old full address before replacing its low half", &[0x66, 0x8b, 0x00])
            .register(Eax, 0x8000_4020, 0x8000_88a1).memory(0x8000_4020, &[0xa1, 0x88], ReadOnly),
        Case::preserving_flags("MOV AX store uses the full address and only its low word as data", &[0x66, 0x89, 0x00])
            .initial_register(Eax, 0x8000_4ffe).memory(0x8000_4ffd, &[0xa5, 0, 0], ReadWrite)
            .expect_memory(0x8000_4ffe, &[0xfe, 0x4f]),
        Case::preserving_flags("MOV byte [EBX],imm8: base", &[0xc6, 0x03, 0x80])
            .initial_register(Ebx, 0x4020).memory(0x401f, &[0xa5, 0xcc, 0x5a], ReadWrite).expect_memory(0x4020, &[0x80]),
        Case::preserving_flags("MOV dword [EBX+ECX*4+0x4000],imm32: address wraps", &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88])
            .initial_register(Ebx, 0xffff_fff0).initial_register(Ecx, 4)
            .memory(0x3fff, &[0xa5, 0xcc, 0xcc, 0xcc, 0xcc, 0x5a], ReadWrite)
            .expect_memory(0x4000, &[0xa0, 0x66, 0xc7, 0x88]),
        Case::preserving_flags("MOV word [0x4020],imm16: absent SIB base and index", &[0x66, 0xc7, 0x04, 0x25, 0x20, 0x40, 0, 0, 0, 0x80])
            .memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x4020, &[0, 0x80]),
    ]
}

#[rustfmt::skip]
fn absolute_forms() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AL,moffs32: high address bit", &[0xa0, 0x20, 0x40, 0, 0x80])
            .register(Eax, 0x4433_2211, 0x4433_22a0)
            .memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV EAX,moffs32: high address bit", &[0xa1, 0x20, 0x40, 0, 0x80])
            .register(Eax, 0x4433_2211, 0x88c7_66a0)
            .memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV moffs32,AL: high address bit", &[0xa2, 0x20, 0x40, 0, 0x80])
            .initial_register(Eax, 0x4433_2211).memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite)
            .expect_memory(0x8000_4020, &[0x11]),
        Case::preserving_flags("MOV moffs32,EAX: high address bit", &[0xa3, 0x20, 0x40, 0, 0x80])
            .initial_register(Eax, 0x4433_2211).memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite)
            .expect_memory(0x8000_4020, &[0x11, 0x22, 0x33, 0x44]),
        Case::preserving_flags("MOV AX,moffs32: full 32-bit absolute offset", &[0x66, 0xa1, 0x20, 0x40, 0, 0x80])
            .register(Eax, 0x4433_2211, 0x4433_88a1).memory(0x8000_401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV moffs32,AX: full 32-bit absolute offset", &[0x66, 0xa3, 0x20, 0x40, 0, 0x80])
            .initial_register(Eax, 0x4433_2211).memory(0x8000_401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite)
            .expect_memory(0x8000_4020, &[0x11, 0x22]),
        Case::preserving_flags("MOV AX,moffs: final two linear bytes", &[0x66, 0xa1, 0xfe, 0xff, 0xff, 0xff])
            .register(Eax, 0x4433_2211, 0x4433_88a1).memory(0xffff_fffe, &[0xa1, 0x88], ReadOnly),
    ]
}

#[rustfmt::skip]
fn faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AH,[EBX]: absent byte read", &[0x8a, 0x23])
            .initial_register(Ebx, 0x4020).fault(0x4020, 0),
        Case::preserving_flags("MOV [EBX],AH: read-only byte write", &[0x88, 0x23])
            .initial_register(Ebx, 0x4020).memory(0x401f, &[0xa5, 0x80, 0x5a], ReadOnly).fault(0x4020, 3),
        Case::preserving_flags("MOV AX,moffs: missing second read page", &[0x66, 0xa1, 0xff, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x4ffe, &[0xa5, 0xa1], ReadWrite).fault(0x5000, 0),
        Case::preserving_flags("MOV moffs32,EAX: second page denial leaves dword unchanged", &[0xa3, 0xfe, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x4ffd, &[0xa5, 1, 2], ReadWrite)
            .memory(0x5000, &[3, 4, 0x5a], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("MOV dword [EBX],imm32: second page denial leaves dword unchanged", &[0xc7, 0x03, 0xa0, 0x66, 0xc7, 0x88])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0xa5, 1, 2], ReadWrite)
            .memory(0x5000, &[3, 4, 0x5a], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("MOV [EBX],EDX: absent data write", &[0x89, 0x13])
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
    ]
}

test_cases!(register_widths_and_encodings, register_forms());
test_cases!(memory_operands_and_widths, memory_forms());
test_cases!(absolute_accumulator_forms, absolute_forms());
test_cases!(faults_preserve_destinations, faults());

#[test]
fn grouped_and_absolute_form_lengths() {
    for code in [
        &[0xc6, 0xc4, 0x80][..],
        &[0x66, 0xc7, 0xc0, 0xa0, 0x66][..],
        &[0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xc6, 0x44, 0x8b, 0x7f, 0xff][..],
        &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xa0, 0x20, 0x40, 0, 0x80][..],
        &[0xa1, 0x20, 0x40, 0, 0x80][..],
        &[0xa2, 0x20, 0x40, 0, 0x80][..],
        &[0xa3, 0x20, 0x40, 0, 0x80][..],
    ] {
        check_length(code);
    }
}

#[test]
fn invalid_groups_reject_before_fetching_an_address_or_immediate() {
    let module = TestModule::interpreter();
    // These ModRM bytes require inaccessible SIB or displacement bytes if accepted.
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        assert!(matches!(
            compile_block_from_bytes(0x1ffe, &[opcode, modrm], 1),
            Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual })
                if actual == opcode
        ));
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffe;
        image.data(0x3ffe, &[opcode, modrm]);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            module,
            "invalid MOV group",
            Exit::Other((8 << 48) | ((opcode as u64) << 32) | 0x1ffe),
        );
    }
}
