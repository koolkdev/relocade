use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::Gpr32;

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
