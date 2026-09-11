use wasm86_x86::Gpr32;

use crate::support::cases::{
    test_cases, FlagExpectation,
    FlagExpectation::{Clear, Set},
    InstructionCase,
    Permissions::{ReadOnly, ReadWrite},
};

use super::{bit_flags, other_register_inputs, INITIAL_FLAGS, OPERATIONS, STORED_FLAGS};

fn signed_and_immediate_indexes() -> Vec<InstructionCase> {
    struct Operand {
        name: &'static str,
        bits: u32,
        base: u32,
        index: u32,
        immediate: bool,
        address: u32,
        input: u32,
        // BT, BTS, BTR, BTC, in that order.
        outputs: [u32; 4],
        carry: FlagExpectation,
    }
    let mut cases = Vec::new();
    for operand in [
        Operand {
            name: "word minus one",
            bits: 16,
            base: 0x4004,
            index: 0xffff,
            immediate: false,
            address: 0x4002,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "word minus sixteen",
            bits: 16,
            base: 0x4004,
            index: 0xfff0,
            immediate: false,
            address: 0x4002,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_8000, 0x0000_8000],
            carry: Set,
        },
        Operand {
            name: "word minus seventeen",
            bits: 16,
            base: 0x4004,
            index: 0xffef,
            immediate: false,
            address: 0x4000,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "dword minus one",
            bits: 32,
            base: 0x4008,
            index: u32::MAX,
            immediate: false,
            address: 0x4004,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "dword minus thirty-two",
            bits: 32,
            base: 0x4008,
            index: 0xffff_ffe0,
            immediate: false,
            address: 0x4004,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x8000_0000, 0x8000_0000],
            carry: Set,
        },
        Operand {
            name: "dword minus thirty-three",
            bits: 32,
            base: 0x4008,
            index: 0xffff_ffdf,
            immediate: false,
            address: 0x4000,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "word next operand",
            bits: 16,
            base: 0x4000,
            index: 16,
            immediate: false,
            address: 0x4002,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_8000, 0x0000_8000],
            carry: Set,
        },
        Operand {
            name: "word next operand second bit",
            bits: 16,
            base: 0x4000,
            index: 17,
            immediate: false,
            address: 0x4002,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8003, 0x0000_8001, 0x0000_8003],
            carry: Clear,
        },
        Operand {
            name: "dword next operand",
            bits: 32,
            base: 0x4000,
            index: 32,
            immediate: false,
            address: 0x4004,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x8000_0000, 0x8000_0000],
            carry: Set,
        },
        Operand {
            name: "dword next operand second bit",
            bits: 32,
            base: 0x4000,
            index: 33,
            immediate: false,
            address: 0x4004,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0003, 0x8000_0001, 0x8000_0003],
            carry: Clear,
        },
        Operand {
            name: "word sign comes from the low word",
            bits: 16,
            base: 0x5000,
            index: 0x1234_8000,
            immediate: false,
            address: 0x4000,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_8000, 0x0000_8000],
            carry: Set,
        },
        Operand {
            name: "word positive maximum ends at the page boundary",
            bits: 16,
            base: 0x4000,
            index: 0xabcd_7fff,
            immediate: false,
            address: 0x4ffe,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "dword negative minimum",
            bits: 32,
            base: 0x1000_4000,
            index: 0x8000_0000,
            immediate: false,
            address: 0x4000,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x8000_0000, 0x8000_0000],
            carry: Set,
        },
        Operand {
            name: "dword positive maximum wraps the adjusted address",
            bits: 32,
            base: 0xf000_4004,
            index: 0x7fff_ffff,
            immediate: false,
            address: 0x4000,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "word adjustment preserves an unaligned base",
            bits: 16,
            base: 0x4003,
            index: 16,
            immediate: false,
            address: 0x4005,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_8000, 0x0000_8000],
            carry: Set,
        },
        Operand {
            name: "word immediate high bits stay in the encoded operand",
            bits: 16,
            base: 0x4ffe,
            index: 255,
            immediate: true,
            address: 0x4ffe,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "dword immediate high bits stay in the encoded operand",
            bits: 32,
            base: 0x4ffc,
            index: 255,
            immediate: true,
            address: 0x4ffc,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "word immediate sixteen selects bit zero",
            bits: 16,
            base: 0x4ffe,
            index: 16,
            immediate: true,
            address: 0x4ffe,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_8000, 0x0000_8000],
            carry: Set,
        },
        Operand {
            name: "dword immediate thirty-two selects bit zero",
            bits: 32,
            base: 0x4ffc,
            index: 32,
            immediate: true,
            address: 0x4ffc,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x8000_0000, 0x8000_0000],
            carry: Set,
        },
        Operand {
            name: "word split after adjustment",
            bits: 16,
            base: 0x5001,
            index: 0xffff,
            immediate: false,
            address: 0x4fff,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "dword split after adjustment",
            bits: 32,
            base: 0x5002,
            index: u32::MAX,
            immediate: false,
            address: 0x4ffe,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
        Operand {
            name: "word adjusted past the address limit",
            bits: 16,
            base: 0xffff_fffe,
            index: 16,
            immediate: false,
            address: 0,
            input: 0x0000_8001,
            outputs: [0x0000_8001, 0x0000_8001, 0x0000_8000, 0x0000_8000],
            carry: Set,
        },
        Operand {
            name: "dword adjusted below zero",
            bits: 32,
            base: 0,
            index: u32::MAX,
            immediate: false,
            address: 0xffff_fffc,
            input: 0x8000_0001,
            outputs: [0x8000_0001, 0x8000_0001, 0x0000_0001, 0x0000_0001],
            carry: Set,
        },
    ] {
        for (operation, output) in OPERATIONS.into_iter().zip(operand.outputs) {
            let mut code = if operand.bits == 16 {
                vec![0x66]
            } else {
                vec![]
            };
            if operand.immediate {
                code.extend_from_slice(&[
                    0x0f,
                    0xba,
                    0x03 | (operation.extension() << 3),
                    operand.index as u8,
                ]);
            } else {
                code.extend_from_slice(&[0x0f, operation.register_opcode(), 0x13]);
            }
            let permissions = if operation.modifies() {
                ReadWrite
            } else {
                ReadOnly
            };
            let bytes = operand.input.to_le_bytes();
            let written = output.to_le_bytes();
            let len = (operand.bits / 8) as usize;
            let offset = operand.address & 0xfff;
            let physical = 0x8000 + offset;
            let first_len = len.min((0x1000 - offset) as usize);
            // BT uses read-only mappings; each modifier requires writable mappings.
            let mut case = InstructionCase::new(
                format!("{operation:?}: {}", operand.name),
                &code,
                INITIAL_FLAGS,
                bit_flags(operand.carry),
            )
            .initial_registers(&other_register_inputs(&[Gpr32::Ebx, Gpr32::Edx]))
            .stored_flags(STORED_FLAGS)
            .initial_register(Gpr32::Ebx, operand.base)
            .initial_register(Gpr32::Edx, operand.index)
            .map_page(operand.address >> 12, 0x8000, permissions)
            .backing(physical - 1, &[0x5a])
            .backing(physical, &bytes[..first_len]);
            if first_len < len {
                case = case
                    .map_page((operand.address >> 12) + 1, 0xa000, permissions)
                    .backing(0xa000, &bytes[first_len..len])
                    .backing(0xa000 + (len - first_len) as u32, &[0x5a]);
            } else if offset + (first_len as u32) < 0x1000 {
                case = case.backing(physical + first_len as u32, &[0x5a]);
            }
            if operation.modifies() {
                case = case.expect_memory(operand.address, &written[..len]);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    signed_indexes_and_immediate_masking,
    signed_and_immediate_indexes()
);

fn aliased_address_indexes() -> Vec<InstructionCase> {
    struct Operand {
        name: &'static str,
        bits: u32,
        address_bytes: &'static [u8],
        registers: &'static [(Gpr32, u32)],
        address: u32,
        input: u32,
        outputs: [u32; 4],
        carry: FlagExpectation,
    }
    let mut cases = Vec::new();
    for operand in [
        Operand {
            name: "EBX supplies address and dword index",
            bits: 32,
            address_bytes: &[0x1b],
            registers: &[(Gpr32::Ebx, 0x4000)],
            address: 0x4800,
            input: 0xa55a_5aa5,
            outputs: [0xa55a_5aa5, 0xa55a_5aa5, 0xa55a_5aa4, 0xa55a_5aa4],
            carry: Set,
        },
        Operand {
            name: "ECX supplies a full address and word index",
            bits: 16,
            address_bytes: &[0x09],
            registers: &[(Gpr32::Ecx, 0xffff_4001)],
            address: 0xffff_4801,
            input: 0x0000_5aa5,
            outputs: [0x0000_5aa5, 0x0000_5aa7, 0x0000_5aa5, 0x0000_5aa7],
            carry: Clear,
        },
        Operand {
            name: "EBP supplies a negative address and bit index",
            bits: 32,
            address_bytes: &[0x2c, 0x2b],
            registers: &[(Gpr32::Ebx, 0x4005), (Gpr32::Ebp, u32::MAX)],
            address: 0x4000,
            input: 0xa55a_5aa5,
            outputs: [0xa55a_5aa5, 0xa55a_5aa5, 0x255a_5aa5, 0x255a_5aa5],
            carry: Set,
        },
        Operand {
            name: "ECX supplies a wrapping scaled address and bit index",
            bits: 32,
            address_bytes: &[0x4c, 0x8b, 0xfc],
            registers: &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)],
            address: 0x0800_4010,
            input: 0xa55a_5aa5,
            outputs: [0xa55a_5aa5, 0xa55a_5aa7, 0xa55a_5aa5, 0xa55a_5aa7],
            carry: Clear,
        },
    ] {
        for (operation, output) in OPERATIONS.into_iter().zip(operand.outputs) {
            let mut code = if operand.bits == 16 {
                vec![0x66]
            } else {
                vec![]
            };
            code.extend_from_slice(&[0x0f, operation.register_opcode()]);
            code.extend_from_slice(operand.address_bytes);
            let len = (operand.bits / 8) as usize;
            let physical = 0x8000 + (operand.address & 0xfff);
            let permissions = if operation.modifies() {
                ReadWrite
            } else {
                ReadOnly
            };
            let address_registers = operand
                .registers
                .iter()
                .map(|&(register, _)| register)
                .collect::<Vec<_>>();
            let mut case = InstructionCase::new(
                format!("{operation:?}: {}", operand.name),
                &code,
                INITIAL_FLAGS,
                bit_flags(operand.carry),
            )
            .initial_registers(&other_register_inputs(&address_registers))
            .stored_flags(STORED_FLAGS)
            .initial_registers(operand.registers)
            .map_page(operand.address >> 12, 0x8000, permissions)
            .backing(physical - 1, &[0x5a])
            .backing(physical, &operand.input.to_le_bytes()[..len])
            .backing(physical + len as u32, &[0x5a]);
            if operation.modifies() {
                case = case.expect_memory(operand.address, &output.to_le_bytes()[..len]);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    indexes_capture_the_old_address_registers,
    aliased_address_indexes()
);
