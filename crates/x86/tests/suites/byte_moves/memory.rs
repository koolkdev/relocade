use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
    RegisterExpectation::Exact,
};
use wasm86_x86::Gpr32;

#[rustfmt::skip]
fn register_memory_cases() -> Vec<Case> {
    struct Register { name: &'static str, parent: Gpr32, input: u32, loaded: u32, stored: u8 }
    let registers = [
        Register { name: "AL", parent: Gpr32::Eax, input: 0x4433_2211, loaded: 0x4433_2280, stored: 0x11 },
        Register { name: "CL", parent: Gpr32::Ecx, input: 0x8877_6655, loaded: 0x8877_6680, stored: 0x55 },
        Register { name: "DL", parent: Gpr32::Edx, input: 0xccbb_aa99, loaded: 0xccbb_aa80, stored: 0x99 },
        Register { name: "BL", parent: Gpr32::Ebx, input: 0x0000_4020, loaded: 0x0000_4080, stored: 0x20 },
        Register { name: "AH", parent: Gpr32::Eax, input: 0x4433_2211, loaded: 0x4433_8011, stored: 0x22 },
        Register { name: "CH", parent: Gpr32::Ecx, input: 0x8877_6655, loaded: 0x8877_8055, stored: 0x66 },
        Register { name: "DH", parent: Gpr32::Edx, input: 0xccbb_aa99, loaded: 0xccbb_8099, stored: 0xaa },
        Register { name: "BH", parent: Gpr32::Ebx, input: 0x0000_4020, loaded: 0x0000_8020, stored: 0x40 },
    ];
    let mut cases = Vec::new();
    for (encoding, register) in registers.iter().enumerate() {
        for opcode in [0x8a, 0x88] {
            let mut case = Case::preserving_flags(format!("MOV memory and {} via {opcode:02x}", register.name), &[opcode, ((encoding as u8) << 3) | 3])
                .initial_register(register.parent, register.input)
                .memory(0x401f, &[0xa5, 0x80, 0x5a], if opcode == 0x8a { ReadOnly } else { ReadWrite });
            if register.parent != Gpr32::Ebx { case = case.initial_register(Gpr32::Ebx, 0x4020); }
            case = if opcode == 0x8a { case.expect_register(register.parent, Exact(register.loaded)) }
                else { case.expect_memory(0x4020, &[register.stored]) };
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn address_cases() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOV AH,[EAX]: high destination overlaps base", &[0x8a, 0x20])
            .register(Gpr32::Eax, 0x4000, 0x8000).memory(0x3fff, &[0xa5, 0x80, 0x5a], ReadWrite),
        Case::preserving_flags("MOV [EAX],AH: high source overlaps base", &[0x88, 0x20])
            .initial_register(Gpr32::Eax, 0x4020).memory(0x401f, &[0xa5, 0x80, 0x5a], ReadWrite).expect_memory(0x4020, &[0x40]),
        Case::preserving_flags("MOV AH,[EAX+ECX*4+0x10]: old full destination", &[0x8a, 0x64, 0x88, 0x10])
            .register(Gpr32::Eax, 0x3ff0, 0x80f0).initial_register(Gpr32::Ecx, 4)
            .memory(0x400f, &[0xa5, 0x80, 0x5a], ReadWrite),
        Case::preserving_flags("MOV [ECX*4+0x4000],CH: no SIB base", &[0x88, 0x2c, 0x8d, 0, 0x40, 0, 0])
            .initial_register(Gpr32::Ecx, 0x104).memory(0x440f, &[0xa5, 0x80, 0x5a], ReadWrite).expect_memory(0x4410, &[1]),
        Case::preserving_flags("MOV AH,[EBX]: absent byte read", &[0x8a, 0x23])
            .initial_register(Gpr32::Ebx, 0x4020).fault(0x4020, 0),
        Case::preserving_flags("MOV [EBX],AH: absent byte write", &[0x88, 0x23])
            .initial_register(Gpr32::Ebx, 0x4020).fault(0x4020, 2),
        Case::preserving_flags("MOV [EBX],AH: read-only byte write", &[0x88, 0x23])
            .initial_register(Gpr32::Ebx, 0x4020).memory(0x401f, &[0xa5, 0x80, 0x5a], ReadOnly).fault(0x4020, 3),
    ]
}

#[rustfmt::skip]
fn final_byte_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for address in [0x4fff, 0xffff_ffff] {
        cases.push(Case::preserving_flags(format!("MOV DL,[EBX]: final mapped byte {address:08x}"), &[0x8a, 0x13])
            .initial_register(Gpr32::Ebx, address).register(Gpr32::Edx, 0xccbb_aa99, 0xccbb_aa80)
            .memory(address - 1, &[0xa5, 0x80], ReadWrite));
        cases.push(Case::preserving_flags(format!("MOV [EBX],DL: final mapped byte {address:08x}"), &[0x88, 0x13])
            .initial_register(Gpr32::Ebx, address).initial_register(Gpr32::Edx, 0xccbb_aa99)
            .memory(address - 1, &[0xa5, 0x80], ReadWrite).expect_memory(address, &[0x99]));
    }
    cases
}

test_cases!(all_byte_memory_registers, register_memory_cases());
test_cases!(effective_addresses_and_faults, address_cases());
test_cases!(single_byte_page_boundaries, final_byte_cases());
