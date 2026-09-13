use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx};

#[rustfmt::skip]
fn address_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AX,[EBX]: base read", &[0x66, 0x8b, 0x03])
            .initial_register(Ebx, 0x4020).register(Eax, 0x4433_2211, 0x4433_88a1)
            .memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV AX,[EAX]: old destination base", &[0x66, 0x8b, 0x00])
            .register(Eax, 0x4020, 0x88a1).memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV [EAX],AX: source aliases address", &[0x66, 0x89, 0x00])
            .initial_register(Eax, 0x4020).memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x4020, &[0x20, 0x40]),
        Case::preserving_flags("MOV [EBX-128],AX: negative disp8", &[0x66, 0x89, 0x43, 0x80])
            .initial_register(Ebx, 0x40a0).initial_register(Eax, 0x4433_2211)
            .memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x4020, &[0x11, 0x22]),
        Case::preserving_flags("MOV word [EBX+ECX*4+0x4020],imm16: wrapped address", &[0x66, 0xc7, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0xa1, 0x88])
            .initial_register(Ebx, 0xffff_fff0).initial_register(Ecx, 4)
            .memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x4020, &[0xa1, 0x88]),
        Case::preserving_flags("MOV word [0x4020],imm16: absent SIB base and index", &[0x66, 0xc7, 0x04, 0x25, 0x20, 0x40, 0, 0, 0, 0x80])
            .memory(0x401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x4020, &[0, 0x80]),
        Case::preserving_flags("MOV AX,moffs32: full 32-bit absolute offset", &[0x66, 0xa1, 0x20, 0x40, 0, 0x80])
            .register(Eax, 0x4433_2211, 0x4433_88a1).memory(0x8000_401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV moffs32,AX: full 32-bit absolute offset", &[0x66, 0xa3, 0x20, 0x40, 0, 0x80])
            .initial_register(Eax, 0x4433_2211).memory(0x8000_401f, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x8000_4020, &[0x11, 0x22]),
    ]
}

#[rustfmt::skip]
fn boundary_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AX,moffs: missing second read page", &[0x66, 0xa1, 0xff, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x4ffe, &[0xa5, 0xa1], ReadWrite).fault(0x5000, 0),
        Case::preserving_flags("MOV moffs,AX: read-only second write page", &[0x66, 0xa3, 0xff, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x4ffe, &[0xa5, 0xa1], ReadWrite).memory(0x5000, &[0x88, 0x5a], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("MOV AX,moffs: word wrap reaches an absent page zero", &[0x66, 0xa1, 0xff, 0xff, 0xff, 0xff])
            .initial_register(Eax, 0x4433_2211).memory(0xffff_ffff, &[0xa1], ReadWrite).fault(0, 0),
        Case::preserving_flags("MOV moffs,AX: word wrap reaches an absent page zero", &[0x66, 0xa3, 0xff, 0xff, 0xff, 0xff])
            .initial_register(Eax, 0x4433_2211).memory(0xffff_ffff, &[0xa1], ReadWrite).fault(0, 2),
        Case::preserving_flags("MOV AX,moffs: final two linear bytes", &[0x66, 0xa1, 0xfe, 0xff, 0xff, 0xff])
            .register(Eax, 0x4433_2211, 0x4433_88a1).memory(0xffff_fffe, &[0xa1, 0x88], ReadOnly),
    ]
}

test_cases!(word_effective_addresses, address_cases());
test_cases!(word_faults_and_boundaries, boundary_cases());

#[rustfmt::skip]
fn page_layout_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (layout, second_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        cases.push(Case::preserving_flags(format!("MOV AX,moffs: {layout} word pages"), &[0x66, 0xa1, 0xff, 0x4f, 0, 0])
            .register(Eax, 0x4433_2211, 0x4433_88a1).map_page(4, 0x8000, ReadWrite).map_page(5, second_frame, ReadWrite)
            .memory(0x4ffe, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite));
        cases.push(Case::preserving_flags(format!("MOV moffs,AX: {layout} word pages"), &[0x66, 0xa3, 0xff, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).map_page(4, 0x8000, ReadWrite).map_page(5, second_frame, ReadWrite)
            .memory(0x4ffe, &[0xa5, 0xa1, 0x88, 0x5a], ReadWrite).expect_memory(0x4fff, &[0x11, 0x22]));
    }
    cases
}
test_cases!(contiguous_and_scattered_word_pages, page_layout_cases());
