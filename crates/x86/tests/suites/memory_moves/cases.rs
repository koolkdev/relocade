use wasm86_x86::Gpr32::{Ebx, Edx};

use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};

// Intel SDM 325462-089, Volume 2, MOV instruction entry. These hand-reviewed
// literals cover ordinary accesses and fault policy. The shared runner checks
// complete registers, all flag bytes, publication, and unchanged memory.
#[rustfmt::skip]
fn cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV EDX,[EBX]: read-only page", &[0x8b, 0x13])
            .initial_register(Ebx, 0x4020)
            .register(Edx, 0xdead_beef, 0x9234_5678)
            .memory(0x4020, &[0x78, 0x56, 0x34, 0x92], ReadOnly),
        Case::preserving_flags("MOV EDX,[EBX]: absent page preserves state", &[0x8b, 0x13])
            .initial_register(Ebx, 0x4020)
            .memory(0x8ffd, &[0xa5, 0x78, 0x56], ReadWrite)
            .memory(0xa000, &[0x34, 0x92, 0x5a], ReadWrite)
            .fault(0x4020, 0),
        Case::preserving_flags("MOV [EBX],EDX: neighboring bytes preserved", &[0x89, 0x13])
            .initial_register(Ebx, 0x4020)
            .initial_register(Edx, 0x1234_5678)
            .memory(0x401f, &[0xa5, 0, 0, 0, 0, 0x5a], ReadWrite)
            .expect_memory(0x401f, &[0xa5, 0x78, 0x56, 0x34, 0x12, 0x5a]),
    ]
}

test_cases!(memory_access, cases());

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV [EBX],EDX: absent data write", &[0x89, 0x13])
            .initial_register(Ebx, 0x4020).fault(0x4020, 2),
        Case::preserving_flags("MOV [EBX],EDX: read-only data write", &[0x89, 0x13])
            .initial_register(Ebx, 0x4020).memory(0x4ffd, &[0xa5, 0x78, 0x56], ReadOnly).fault(0x4020, 3),
        Case::preserving_flags("MOV EDX,[EBX]: absent second page", &[0x8b, 0x13])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0xa5, 0x78, 0x56], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("MOV [EBX],EDX: absent second page", &[0x89, 0x13])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0xa5, 0x78, 0x56], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("MOV [EBX],EDX: second page denial preserves both halves", &[0x89, 0x13])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0xa5, 0x78, 0x56], ReadWrite).memory(0x5000, &[0x34, 0x92, 0x5a], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("MOV [EBX],EDX: first denial precedes second absence", &[0x89, 0x13])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0xa5, 0x78, 0x56], ReadOnly).fault(0x4ffe, 3),
        Case::preserving_flags("MOV EDX,[EBX]: dword range cannot wrap", &[0x8b, 0x13])
            .initial_register(Ebx, 0xffff_fffe).memory(0xffff_fffe, &[0x78, 0x56], ReadWrite).memory(0, &[0x34, 0x92], ReadWrite).fault(0xffff_fffe, 0),
        Case::preserving_flags("MOV [EBX],EDX: dword range cannot wrap", &[0x89, 0x13])
            .initial_register(Ebx, 0xffff_fffe).memory(0xffff_fffe, &[0x78, 0x56], ReadWrite).memory(0, &[0x34, 0x92], ReadWrite).fault(0xffff_fffe, 2),
        Case::preserving_flags("MOV EDX,[ESP]: complete SIB at mapped page end", &[0x8b, 0x14, 0x24])
            .initial_register(wasm86_x86::Gpr32::Esp, 0x4000).register(Edx, 0xdead_beef, 0x9234_5678)
            .memory(0x4000, &[0x78, 0x56, 0x34, 0x92], ReadOnly).at(0x1ffd),
    ]
}
test_cases!(data_fault_policy, fault_cases());

#[rustfmt::skip]
fn page_layout_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (layout, second_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        cases.push(Case::preserving_flags(format!("MOV EDX,[EBX]: {layout} dword pages"), &[0x8b, 0x13])
            .initial_register(Ebx, 0x4ffe).register(Edx, 0xdead_beef, 0x9234_5678)
            .map_page(4, 0x8000, ReadWrite).map_page(5, second_frame, ReadWrite).memory(0x4ffd, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite));
        cases.push(Case::preserving_flags(format!("MOV [EBX],EDX: {layout} dword pages"), &[0x89, 0x13])
            .initial_register(Ebx, 0x4ffe).initial_register(Edx, 0xdead_beef)
            .map_page(4, 0x8000, ReadWrite).map_page(5, second_frame, ReadWrite).memory(0x4ffd, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite)
            .expect_memory(0x4ffe, &[0xef, 0xbe, 0xad, 0xde]));
    }
    cases.push(Case::preserving_flags("MOV EDX,[EAX+disp32]: displacement crosses scattered code pages", &[0x8b, 0x90, 0, 0x40, 0, 0])
        .initial_register(wasm86_x86::Gpr32::Eax, 0).register(Edx, 0xdead_beef, 0x9234_5678).at(0x1ffc)
        .map_page(1, 0x3000, ReadOnly).map_page(2, 0x6000, ReadOnly).map_page(4, 0x8000, ReadOnly)
        .memory(0x4000, &[0x78, 0x56, 0x34, 0x92], ReadOnly));
    cases
}
test_cases!(physical_page_layouts, page_layout_cases());
