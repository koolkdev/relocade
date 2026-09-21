//! Shared 32-bit ModRM/SIB address integration using MOV loads and stores.

use crate::support::cases::{
    test_cases, InstructionCase as Case, Permissions::ReadWrite, RegisterExpectation::Exact,
};
use wasm86_x86::Gpr32;

struct Address {
    name: &'static str,
    code: &'static [u8],
    registers: &'static [(Gpr32, u32)],
    linear: u32,
    stored: u32,
}

const ADDRESSES: &[Address] = &[
    Address {
        name: "EAX base",
        code: &[0x8b, 0x10],
        registers: &[(Gpr32::Eax, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "ECX base",
        code: &[0x8b, 0x11],
        registers: &[(Gpr32::Ecx, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "EDX address read before EDX replacement",
        code: &[0x8b, 0x12],
        registers: &[(Gpr32::Edx, 0x4000)],
        linear: 0x4000,
        stored: 0x4000,
    },
    Address {
        name: "EBX base",
        code: &[0x8b, 0x13],
        registers: &[(Gpr32::Ebx, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "ESP through SIB with no index",
        code: &[0x8b, 0x14, 0x24],
        registers: &[(Gpr32::Esp, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "EBP with displacement zero",
        code: &[0x8b, 0x55, 0],
        registers: &[(Gpr32::Ebp, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "ESI base",
        code: &[0x8b, 0x16],
        registers: &[(Gpr32::Esi, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "EDI base",
        code: &[0x8b, 0x17],
        registers: &[(Gpr32::Edi, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale one",
        code: &[0x8b, 0x54, 0x0b, 0x10],
        registers: &[(Gpr32::Ebx, 0x4000), (Gpr32::Ecx, 3)],
        linear: 0x4013,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale two",
        code: &[0x8b, 0x54, 0x4b, 0x10],
        registers: &[(Gpr32::Ebx, 0x4000), (Gpr32::Ecx, 3)],
        linear: 0x4016,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale four",
        code: &[0x8b, 0x54, 0x8b, 0x10],
        registers: &[(Gpr32::Ebx, 0x4000), (Gpr32::Ecx, 3)],
        linear: 0x401c,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale eight",
        code: &[0x8b, 0x54, 0xcb, 0x10],
        registers: &[(Gpr32::Ebx, 0x4000), (Gpr32::Ecx, 3)],
        linear: 0x4028,
        stored: 0xdead_beef,
    },
    Address {
        name: "SIB absent index ignores scale",
        code: &[0x8b, 0x14, 0xe3],
        registers: &[(Gpr32::Ebx, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "absolute displacement",
        code: &[0x8b, 0x15, 0, 0x40, 0, 0],
        registers: &[],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "SIB without base or index",
        code: &[0x8b, 0x14, 0x25, 0, 0x40, 0, 0],
        registers: &[],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "SIB EBP index without base",
        code: &[0x8b, 0x14, 0xad, 0, 0x40, 0, 0],
        registers: &[(Gpr32::Ebp, 3)],
        linear: 0x400c,
        stored: 0xdead_beef,
    },
    Address {
        name: "negative disp8 boundary",
        code: &[0x8b, 0x50, 0x80],
        registers: &[(Gpr32::Eax, 0x4080)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "positive disp8 boundary",
        code: &[0x8b, 0x50, 0x7f],
        registers: &[(Gpr32::Eax, 0x3f81)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "high-bit disp32 wrapping sum",
        code: &[0x8b, 0x90, 0, 0, 0, 0x80],
        registers: &[(Gpr32::Eax, 0x8000_4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "scaled effective address wraps",
        code: &[0x8b, 0x14, 0x88],
        registers: &[(Gpr32::Eax, 0xffff_fffc), (Gpr32::Ecx, 2)],
        linear: 4,
        stored: 0xdead_beef,
    },
];

#[rustfmt::skip]
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for address in ADDRESSES {
        for opcode in [0x8b, 0x89] {
            let mut code = address.code.to_vec();
            code[0] = opcode;
            let mut case = Case::preserving_flags(format!("{} via {opcode:02x}", address.name), &code)
                .initial_registers(address.registers)
                .memory(address.linear - 1, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a], ReadWrite);
            if !address.registers.iter().any(|&(register, _)| register == Gpr32::Edx) {
                case = case.initial_register(Gpr32::Edx, 0xdead_beef);
            }
            case = if opcode == 0x8b { case.expect_register(Gpr32::Edx, Exact(0x9234_5678)) }
                else { case.expect_memory(address.linear, &address.stored.to_le_bytes()) };
            cases.push(case);
        }
    }
    cases
}

test_cases!(all_address_forms, cases());
