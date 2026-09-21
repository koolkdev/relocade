//! MOVSX/MOVZX source selection, extension boundaries and exact memory widths.
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::{self, Eax, Ebx, Ecx, Edx, Esi};

#[rustfmt::skip]
fn byte_source_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVZX AX,AH reads the old high byte", &[0x66, 0x0f, 0xb6, 0xc4])
            .register(Eax, 0x4433_8011, 0x4433_0080),
        Case::preserving_flags("MOVSX EAX,AH reads the old high byte", &[0x0f, 0xbe, 0xc4])
            .register(Eax, 0x4433_8011, 0xffff_ff80),
        Case::preserving_flags("MOVZX EBP,CH separates source and destination register codes", &[0x0f, 0xb6, 0xed])
            .initial_register(Ecx, 0x8877_6655).register(Gpr32::Ebp, 0x6666_6666, 0x66),
        Case::preserving_flags("MOVSX DI,BH preserves the destination upper half", &[0x66, 0x0f, 0xbe, 0xff])
            .initial_register(Ebx, 0x10ff_eedd).register(Gpr32::Edi, 0x8888_8888, 0x8888_ffee),
    ]
}
test_cases!(byte_source_selection_and_overlap, byte_source_cases());

#[rustfmt::skip]
fn sign_boundary_cases() -> Vec<Case> {
    [
        (&[0x0f, 0xbe, 0xc3][..], 0xa5a5_0000, 0x0000_0000),
        (&[0x66, 0x0f, 0xbe, 0xc3][..], 0xa5a5_007f, 0x4433_007f),
        (&[0x0f, 0xbe, 0xc3][..], 0xa5a5_0080, 0xffff_ff80),
        (&[0x66, 0x0f, 0xbe, 0xc3][..], 0xa5a5_00ff, 0x4433_ffff),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_0000, 0x0000_0000),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_7fff, 0x0000_7fff),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_8000, 0xffff_8000),
        (&[0x0f, 0xbf, 0xc3][..], 0xa5a5_ffff, 0xffff_ffff),
    ].into_iter().map(|(code, ebx, eax)| {
        Case::preserving_flags(format!("MOVSX sign boundary {ebx:08x} via {code:02x?}"), code)
            .initial_register(Gpr32::Ebx, ebx).register(Gpr32::Eax, 0x4433_2211, eax)
    }).collect()
}
test_cases!(sign_boundaries_preserve_flags, sign_boundary_cases());

#[rustfmt::skip]
fn word_source_cases() -> Vec<Case> {
    [
        (&[0x0f, 0xb7, 0xc2][..], 0x0000_8000),
        (&[0x66, 0x0f, 0xb7, 0xc2][..], 0x4433_8000),
        (&[0x66, 0x0f, 0xbf, 0xc2][..], 0x4433_8000),
    ].into_iter().map(|(code, eax)| {
        Case::preserving_flags(format!("word source uses its low half: {code:02x?}"), code)
            .initial_register(Gpr32::Edx, 0x7fff_8000).register(Gpr32::Eax, 0x4433_2211, eax)
    }).collect()
}
test_cases!(
    word_sources_zero_extend_or_keep_the_destination_upper_half,
    word_source_cases()
);

#[rustfmt::skip]
fn readonly_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (opcode, word_destination, address, eax) in [
        (0xb6, false, 0x4fff, 0x0000_0080),
        (0xbe, false, 0x4fff, 0xffff_ff80),
        (0xb6, true, 0x4fff, 0x4433_0080),
        (0xbe, true, 0x4fff, 0x4433_ff80),
        (0xb7, false, 0x4ffe, 0x0000_80a1),
        (0xbf, false, 0x4ffe, 0xffff_80a1),
        (0xb7, true, 0x4ffe, 0x4433_80a1),
        (0xbf, true, 0x4ffe, 0x4433_80a1),
    ] {
        let mut code = if word_destination { vec![0x66] } else { vec![] };
        code.extend_from_slice(&[0x0f, opcode, 0x03]);
        cases.push(Case::preserving_flags(format!("read-only source at {address:04x} via {code:02x?}"), &code)
            .initial_register(Gpr32::Ebx, address).register(Gpr32::Eax, 0x4433_2211, eax)
            .memory(0x4ffd, &[0x5a, 0xa1, 0x80], ReadOnly));
    }
    cases
}
test_cases!(
    readonly_sources_need_only_their_encoded_width,
    readonly_cases()
);

#[rustfmt::skip]
fn page_layout_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVZX EAX,word [EBX]: contiguous pages", &[0x0f, 0xb7, 0x03])
            .initial_register(Gpr32::Ebx, 0x4fff).register(Gpr32::Eax, 0x4433_2211, 0x0000_80a1)
            .map_page(4, 0x8000, ReadOnly).map_page(5, 0x9000, ReadOnly).memory(0x4ffe, &[0x5a, 0xa1, 0x80, 0x5a], ReadOnly),
        Case::preserving_flags("MOVSX EAX,word [EBX]: scattered pages", &[0x0f, 0xbf, 0x03])
            .initial_register(Gpr32::Ebx, 0x4fff).register(Gpr32::Eax, 0x4433_2211, 0xffff_80a1)
            .map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly).memory(0x4ffe, &[0x5a, 0xa1, 0x80, 0x5a], ReadOnly),
    ]
}
test_cases!(
    word_sources_cross_contiguous_and_scattered_pages,
    page_layout_cases()
);

#[rustfmt::skip]
fn address_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVSX AX,[EAX]: preserves high address bits", &[0x66, 0x0f, 0xbe, 0x00])
            .register(Gpr32::Eax, 0x8000_4010, 0x8000_ffa1).initial_register(Gpr32::Ecx, 4)
            .memory(0x8000_400f, &[0x5a, 0xa1, 0x80, 0x5a], ReadOnly),
        Case::preserving_flags("MOVSX EAX,[EAX+ECX*4+0x10]: original base", &[0x0f, 0xbf, 0x44, 0x88, 0x10])
            .register(Gpr32::Eax, 0x8000_3ff0, 0xffff_80a1).initial_register(Gpr32::Ecx, 4)
            .memory(0x8000_400f, &[0x5a, 0xa1, 0x80, 0x5a], ReadOnly),
    ]
}
test_cases!(
    effective_addresses_use_the_old_full_destination,
    address_cases()
);

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVSX EAX,byte [EBX]: missing source", &[0x0f, 0xbe, 0x03])
            .initial_register(Gpr32::Ebx, 0x4020).initial_register(Gpr32::Eax, 0x4433_2211).fault(0x4020, 0),
        Case::preserving_flags("MOVSX EAX,word [EBX]: missing second page", &[0x0f, 0xbf, 0x03])
            .initial_register(Gpr32::Ebx, 0x4fff).initial_register(Gpr32::Eax, 0x4433_2211)
            .memory(0x4fff, &[0x80], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("MOVZX EAX,word [EBX]: wrapped read reaches an absent page zero", &[0x0f, 0xb7, 0x03])
            .initial_register(Gpr32::Ebx, 0xffff_ffff).initial_register(Gpr32::Eax, 0x4433_2211)
            .memory(0xffff_ffff, &[0x80], ReadOnly).fault(0, 0),
    ]
}
test_cases!(
    failed_reads_leave_the_destination_flags_and_count_unchanged,
    fault_cases()
);

#[test]
fn forms_consume_their_source_address_without_an_immediate() {
    for code in [
        &[0x0f, 0xb6, 0xc4][..],
        &[0x66, 0x0f, 0xbe, 0xc4][..],
        &[0x0f, 0xb7, 0x05, 0x20, 0x40, 0, 0][..],
        &[0x66, 0x0f, 0xbf, 0x44, 0x8b, 0x80][..],
    ] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn dependent_extensions() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("a high-byte write feeds an overlapping extension and a full-width consumer")
            .initial_registers(&[(Eax, 0x4433_2211), (Edx, 0xccbb_aa99)])
            .step(Step::preserving_flags(&[0xb4, 0x80]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0x66, 0x0f, 0xbe, 0xc4]).register(Eax, 0x4433_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xb7, 0xd0]).register(Edx, 0x0000_ff80)),
        Sequence::preserving_flags("an extended load survives an aliased store and a later fault")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0), (Ebx, 0x4000)])
            .map_page(4, 0x8000, ReadOnly).map_page(6, 0x8000, ReadWrite).backing(0x7fff, &[0x5a, 0x80, 0xff, 0x5a])
            .step(Step::preserving_flags(&[0x0f, 0xb7, 0x03]).register(Eax, 0x0000_ff80))
            .step(Step::preserving_flags(&[0x66, 0x89, 0x11]).expect_memory(0x6000, &[0, 0]))
            .step(Step::preserving_flags(&[0x0f, 0xbf, 0xf0]).register(Esi, 0xffff_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xbf, 0x3e]).fault(0xffff_ff80, 0)),
    ]
}
test_sequences!(dependent_extensions_and_faults, dependent_extensions());
