use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{self, *};

#[rustfmt::skip]
fn register_cases() -> Vec<Case> {
    struct Byte { encoding: u8, name: &'static str, parent: Gpr32, input: u32, immediate: u8, output: u32 }
    let mut cases = Vec::new();
    for byte in [
        Byte { encoding: 0, name: "AL", parent: Eax, input: 0x4433_2211, immediate: 0x80, output: 0x4433_2280 },
        Byte { encoding: 1, name: "CL", parent: Ecx, input: 0x8877_6655, immediate: 0, output: 0x8877_6600 },
        Byte { encoding: 2, name: "DL", parent: Edx, input: 0xccbb_aa99, immediate: 0xff, output: 0xccbb_aaff },
        Byte { encoding: 3, name: "BL", parent: Ebx, input: 0x10ff_eedd, immediate: 0x66, output: 0x10ff_ee66 },
        Byte { encoding: 4, name: "AH", parent: Eax, input: 0x4433_2211, immediate: 0xc6, output: 0x4433_c611 },
        Byte { encoding: 5, name: "CH", parent: Ecx, input: 0x8877_6655, immediate: 0xa0, output: 0x8877_a055 },
        Byte { encoding: 6, name: "DH", parent: Edx, input: 0xccbb_aa99, immediate: 0xc7, output: 0xccbb_c799 },
        Byte { encoding: 7, name: "BH", parent: Ebx, input: 0x10ff_eedd, immediate: 0x7f, output: 0x10ff_7fdd },
    ] {
        cases.push(Case::preserving_flags(format!("C6 immediate to {}", byte.name), &[0xc6, 0xc0 + byte.encoding, byte.immediate])
            .register(byte.parent, byte.input, byte.output));
    }
    for (encoding, register, input) in [
        (0, Eax, 0x4433_2211), (1, Ecx, 0x8877_6655), (2, Edx, 0xccbb_aa99), (3, Ebx, 0x10ff_eedd),
        (4, Esp, 0x5555_5555), (5, Ebp, 0x6666_6666), (6, Esi, 0x7777_7777), (7, Edi, 0x8888_8888),
    ] {
        cases.push(Case::preserving_flags(format!("C7 immediate to {register:?}"), &[0xc7, 0xc0 + encoding, 0xa0, 0x66, 0xc7, 0x88])
            .register(register, input, 0x88c7_66a0));
    }
    cases
}

#[rustfmt::skip]
fn addressed_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV byte [EBX],imm8: base", &[0xc6, 0x03, 0x80])
            .initial_register(Ebx, 0x4020).memory(0x401f, &[0xa5, 0xcc, 0x5a], ReadWrite).expect_memory(0x4020, &[0x80]),
        Case::preserving_flags("MOV dword [EBX-128],imm32: negative disp8", &[0xc7, 0x43, 0x80, 0xa0, 0x66, 0xc7, 0x88])
            .initial_register(Ebx, 0x4080).memory(0x3fff, &[0xa5, 0xcc, 0xcc, 0xcc, 0xcc, 0x5a], ReadWrite).expect_memory(0x4000, &[0xa0, 0x66, 0xc7, 0x88]),
        Case::preserving_flags("MOV byte [EBX+ECX*4+127],imm8: scaled index", &[0xc6, 0x44, 0x8b, 0x7f, 0xff])
            .initial_register(Ebx, 0x3f01).initial_register(Ecx, 32)
            .memory(0x3fff, &[0xa5, 0xcc, 0x5a], ReadWrite).expect_memory(0x4000, &[0xff]),
        Case::preserving_flags("MOV dword [EBX+ECX*4+0x4000],imm32: address wraps", &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88])
            .initial_register(Ebx, 0xffff_fff0).initial_register(Ecx, 4)
            .memory(0x3fff, &[0xa5, 0xcc, 0xcc, 0xcc, 0xcc, 0x5a], ReadWrite).expect_memory(0x4000, &[0xa0, 0x66, 0xc7, 0x88]),
        Case::preserving_flags("MOV byte [0x4020],imm8: no SIB base or index", &[0xc6, 0x04, 0x25, 0x20, 0x40, 0, 0, 0xb7])
            .memory(0x401f, &[0xa5, 0xcc, 0x5a], ReadWrite).expect_memory(0x4020, &[0xb7]),
        Case::preserving_flags("MOV dword [ECX*4+0x3ff0],imm32: no SIB base", &[0xc7, 0x04, 0x8d, 0xf0, 0x3f, 0, 0, 0xff, 0xff, 0xff, 0xff])
            .initial_register(Ecx, 4).memory(0x3fff, &[0xa5, 0xcc, 0xcc, 0xcc, 0xcc, 0x5a], ReadWrite).expect_memory(0x4000, &[0xff, 0xff, 0xff, 0xff]),
    ]
}

#[rustfmt::skip]
fn absolute_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AL,moffs32: high address bit", &[0xa0, 0x20, 0x40, 0, 0x80])
            .register(Eax, 0x4433_2211, 0x4433_22a0).memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV EAX,moffs32: high address bit", &[0xa1, 0x20, 0x40, 0, 0x80])
            .register(Eax, 0x4433_2211, 0x88c7_66a0).memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite),
        Case::preserving_flags("MOV moffs32,AL: high address bit", &[0xa2, 0x20, 0x40, 0, 0x80])
            .initial_register(Eax, 0x4433_2211).memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite).expect_memory(0x8000_4020, &[0x11]),
        Case::preserving_flags("MOV moffs32,EAX: high address bit", &[0xa3, 0x20, 0x40, 0, 0x80])
            .initial_register(Eax, 0x4433_2211).memory(0x8000_401f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite).expect_memory(0x8000_4020, &[0x11, 0x22, 0x33, 0x44]),
        Case::preserving_flags("MOV AL,moffs32: final linear byte", &[0xa0, 0xff, 0xff, 0xff, 0xff])
            .register(Eax, 0x4433_2211, 0x4433_2280).memory(0xffff_fffe, &[0xa5, 0x80], ReadWrite),
        Case::preserving_flags("MOV moffs32,AL: final linear byte", &[0xa2, 0xff, 0xff, 0xff, 0xff])
            .initial_register(Eax, 0x4433_2211).memory(0xffff_fffe, &[0xa5, 0x80], ReadWrite).expect_memory(0xffff_ffff, &[0x11]),
    ]
}

#[rustfmt::skip]
fn fault_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV EAX,moffs32: missing second page", &[0xa1, 0xfe, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x4ffe, &[0xa0, 0x66], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("MOV moffs32,EAX: second page denial leaves dword unchanged", &[0xa3, 0xfe, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x4ffd, &[0xa5, 1, 2], ReadWrite).memory(0x5000, &[3, 4, 0x5a], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("MOV dword [EBX],imm32: second page denial leaves dword unchanged", &[0xc7, 0x03, 0xa0, 0x66, 0xc7, 0x88])
            .initial_register(Ebx, 0x4ffe).memory(0x4ffd, &[0xa5, 1, 2], ReadWrite).memory(0x5000, &[3, 4, 0x5a], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("MOV EAX,moffs32: dword range cannot wrap", &[0xa1, 0xfe, 0xff, 0xff, 0xff])
            .initial_register(Eax, 0x4433_2211).memory(0xffff_fffe, &[1, 2], ReadWrite).memory(0, &[3, 4], ReadWrite).fault(0xffff_fffe, 0),
        Case::preserving_flags("MOV moffs32,EAX: dword range cannot wrap", &[0xa3, 0xfe, 0xff, 0xff, 0xff])
            .initial_register(Eax, 0x4433_2211).memory(0xffff_fffe, &[1, 2], ReadWrite).memory(0, &[3, 4], ReadWrite).fault(0xffff_fffe, 2),
    ]
}

#[rustfmt::skip]
fn instruction_boundary_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("C6 ends at the mapped page boundary", &[0xc6, 0x03, 0x80])
            .initial_register(Ebx, 0x4000).memory(0x3fff, &[0xa5, 0, 0, 0, 0, 0x5a], ReadWrite).expect_memory(0x4000, &[0x80]).at(0x1ffd),
        Case::preserving_flags("A3 ends at the mapped page boundary", &[0xa3, 0, 0x40, 0, 0])
            .initial_register(Eax, 0x4433_2211).memory(0x3fff, &[0xa5, 0, 0, 0, 0, 0x5a], ReadWrite).expect_memory(0x4000, &[0x11, 0x22, 0x33, 0x44]).at(0x1ffb),
        Case::preserving_flags("C7 immediate fetch wraps EIP", &[0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88])
            .register(Eax, 0x4433_2211, 0x88c7_66a0).at(0xffff_fffc),
    ]
}

test_cases!(grouped_register_immediates, register_cases());
test_cases!(addressed_immediates, addressed_cases());
test_cases!(absolute_offsets, absolute_cases());
test_cases!(data_faults, fault_cases());
test_cases!(instruction_boundaries, instruction_boundary_cases());

#[rustfmt::skip]
fn page_layout_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (layout, second_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        cases.push(Case::preserving_flags(format!("MOV EAX,moffs: {layout} dword pages"), &[0xa1, 0xfe, 0x4f, 0, 0])
            .register(Eax, 0x4433_2211, 0x88c7_66a0).map_page(4, 0x8000, ReadWrite).map_page(5, second_frame, ReadWrite)
            .memory(0x4ffd, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite));
        cases.push(Case::preserving_flags(format!("MOV moffs,EAX: {layout} dword pages"), &[0xa3, 0xfe, 0x4f, 0, 0])
            .initial_register(Eax, 0x4433_2211).map_page(4, 0x8000, ReadWrite).map_page(5, second_frame, ReadWrite)
            .memory(0x4ffd, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a], ReadWrite).expect_memory(0x4ffe, &[0x11, 0x22, 0x33, 0x44]));
    }
    cases.push(Case::preserving_flags("eleven-byte MOV immediate fetch crosses scattered frames", &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88])
        .initial_register(Ebx, 0xffff_fff0).initial_register(Ecx, 4).at(0x1ff9)
        .map_page(1, 0x3000, ReadOnly).map_page(2, 0xa000, ReadOnly).map_page(4, 0x8000, ReadWrite)
        .backing(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]).expect_memory(0x4000, &[0xa0, 0x66, 0xc7, 0x88]));
    cases
}
test_cases!(physical_page_layouts, page_layout_cases());
