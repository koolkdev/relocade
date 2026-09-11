use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, operand_address, prior_flags, OPERATIONS};

#[test]
fn signed_register_indexes_choose_the_operand_while_immediates_stay_within_it() {
    struct Case {
        name: &'static str,
        bits: u32,
        base: u32,
        index: u32,
        immediate: bool,
        address: u32,
    }
    for case in [
        Case {
            name: "word minus one",
            bits: 16,
            base: 0x4004,
            index: 0xffff,
            immediate: false,
            address: 0x4002,
        },
        Case {
            name: "word minus sixteen",
            bits: 16,
            base: 0x4004,
            index: 0xfff0,
            immediate: false,
            address: 0x4002,
        },
        Case {
            name: "word minus seventeen",
            bits: 16,
            base: 0x4004,
            index: 0xffef,
            immediate: false,
            address: 0x4000,
        },
        Case {
            name: "dword minus one",
            bits: 32,
            base: 0x4008,
            index: u32::MAX,
            immediate: false,
            address: 0x4004,
        },
        Case {
            name: "dword minus thirty-two",
            bits: 32,
            base: 0x4008,
            index: 0xffff_ffe0,
            immediate: false,
            address: 0x4004,
        },
        Case {
            name: "dword minus thirty-three",
            bits: 32,
            base: 0x4008,
            index: 0xffff_ffdf,
            immediate: false,
            address: 0x4000,
        },
        Case {
            name: "word next operand",
            bits: 16,
            base: 0x4000,
            index: 16,
            immediate: false,
            address: 0x4002,
        },
        Case {
            name: "word next operand second bit",
            bits: 16,
            base: 0x4000,
            index: 17,
            immediate: false,
            address: 0x4002,
        },
        Case {
            name: "dword next operand",
            bits: 32,
            base: 0x4000,
            index: 32,
            immediate: false,
            address: 0x4004,
        },
        Case {
            name: "dword next operand second bit",
            bits: 32,
            base: 0x4000,
            index: 33,
            immediate: false,
            address: 0x4004,
        },
        Case {
            name: "word sign comes from the low word",
            bits: 16,
            base: 0x5000,
            index: 0x1234_8000,
            immediate: false,
            address: 0x4000,
        },
        Case {
            name: "word positive maximum ends at the page boundary",
            bits: 16,
            base: 0x4000,
            index: 0xabcd_7fff,
            immediate: false,
            address: 0x4ffe,
        },
        Case {
            name: "dword negative minimum",
            bits: 32,
            base: 0x1000_4000,
            index: 0x8000_0000,
            immediate: false,
            address: 0x4000,
        },
        Case {
            name: "dword positive maximum wraps the adjusted address",
            bits: 32,
            base: 0xf000_4004,
            index: 0x7fff_ffff,
            immediate: false,
            address: 0x4000,
        },
        Case {
            name: "word adjustment preserves an unaligned base",
            bits: 16,
            base: 0x4003,
            index: 16,
            immediate: false,
            address: 0x4005,
        },
        Case {
            name: "word immediate high bits stay in the encoded operand",
            bits: 16,
            base: 0x4ffe,
            index: 255,
            immediate: true,
            address: 0x4ffe,
        },
        Case {
            name: "dword immediate high bits stay in the encoded operand",
            bits: 32,
            base: 0x4ffc,
            index: 255,
            immediate: true,
            address: 0x4ffc,
        },
        Case {
            name: "word immediate sixteen selects bit zero",
            bits: 16,
            base: 0x4ffe,
            index: 16,
            immediate: true,
            address: 0x4ffe,
        },
        Case {
            name: "dword immediate thirty-two selects bit zero",
            bits: 32,
            base: 0x4ffc,
            index: 32,
            immediate: true,
            address: 0x4ffc,
        },
        Case {
            name: "word split after adjustment",
            bits: 16,
            base: 0x5001,
            index: 0xffff,
            immediate: false,
            address: 0x4fff,
        },
        Case {
            name: "dword split after adjustment",
            bits: 32,
            base: 0x5002,
            index: u32::MAX,
            immediate: false,
            address: 0x4ffe,
        },
        Case {
            name: "word adjusted past the address limit",
            bits: 16,
            base: 0xffff_fffe,
            index: 16,
            immediate: false,
            address: 0,
        },
        Case {
            name: "dword adjusted below zero",
            bits: 32,
            base: 0,
            index: u32::MAX,
            immediate: false,
            address: 0xffff_fffc,
        },
    ] {
        assert_eq!(
            operand_address(case.base, case.bits, case.index, case.immediate),
            case.address,
            "{}",
            case.name
        );
        for operation in OPERATIONS {
            let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
            if case.immediate {
                code.extend_from_slice(&[
                    0x0f,
                    0xba,
                    0x03 | (operation.extension() << 3),
                    case.index as u8,
                ]);
            } else {
                code.extend_from_slice(&[0x0f, operation.register_opcode(), 0x13]);
            }
            let mut image = image(&code);
            image.cpu.registers.ebx = case.base;
            image.cpu.registers.edx = case.index;
            let input = (1 << (case.bits - 1)) | 1;
            let result = expected(operation, case.bits, input, case.index);
            let bytes = input.to_le_bytes();
            let written = result.value.to_le_bytes();
            let len = (case.bits / 8) as usize;
            let offset = case.address & 0xfff;
            let physical = 0x8000 + offset;
            let first_len = len.min((0x1000 - offset) as usize);
            // A successful BT also proves the selected pages need no write permission.
            image.map(case.address >> 12, 0x8000, operation.modifies());
            image.data(physical - 1, &[0x5a]);
            image.data(physical, &bytes[..first_len]);
            let mut writes = vec![];
            if operation.modifies() {
                writes.push((physical, &written[..first_len]));
            }
            if first_len < len {
                image.map((case.address >> 12) + 1, 0xa000, operation.modifies());
                image.data(0xa000, &bytes[first_len..len]);
                image.data(0xa000 + (len - first_len) as u32, &[0x5a]);
                if operation.modifies() {
                    writes.push((0xa000, &written[first_len..len]));
                }
            } else if offset + (first_len as u32) < 0x1000 {
                image.data(physical + first_len as u32, &[0x5a]);
            }
            let mut cpu = image.cpu;
            result.apply_flags(&mut cpu, prior_flags());
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("{operation:?}: {}", case.name),
                &code,
                1,
                &image,
                &[Step {
                    cpu,
                    ram: &writes,
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

#[test]
fn an_index_can_also_supply_the_old_base_or_scaled_address_index() {
    struct Case {
        name: &'static str,
        bits: u32,
        address_bytes: &'static [u8],
        registers: &'static [(Gpr32, u32)],
        base: u32,
        index: u32,
        address: u32,
    }
    for case in [
        Case {
            name: "EBX supplies address and dword index",
            bits: 32,
            address_bytes: &[0x1b],
            registers: &[(Gpr32::Ebx, 0x4000)],
            base: 0x4000,
            index: 0x4000,
            address: 0x4800,
        },
        Case {
            name: "ECX supplies a full address and word index",
            bits: 16,
            address_bytes: &[0x09],
            registers: &[(Gpr32::Ecx, 0xffff_4001)],
            base: 0xffff_4001,
            index: 0xffff_4001,
            address: 0xffff_4801,
        },
        Case {
            name: "EBP supplies a negative address and bit index",
            bits: 32,
            address_bytes: &[0x2c, 0x2b],
            registers: &[(Gpr32::Ebx, 0x4005), (Gpr32::Ebp, u32::MAX)],
            base: 0x4004,
            index: u32::MAX,
            address: 0x4000,
        },
        Case {
            name: "ECX supplies a wrapping scaled address and bit index",
            bits: 32,
            address_bytes: &[0x4c, 0x8b, 0xfc],
            registers: &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)],
            base: 0x4010,
            index: 0x4000_0001,
            address: 0x0800_4010,
        },
    ] {
        assert_eq!(
            operand_address(case.base, case.bits, case.index, false),
            case.address,
            "{}",
            case.name
        );
        for operation in OPERATIONS {
            let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, operation.register_opcode()]);
            code.extend_from_slice(case.address_bytes);
            let mut image = image(&code);
            for &(register, value) in case.registers {
                image.cpu.registers[register] = value;
            }
            let input = 0xa55a_5aa5;
            let result = expected(operation, case.bits, input, case.index);
            let bytes = input.to_le_bytes();
            let written = result.value.to_le_bytes();
            let len = (case.bits / 8) as usize;
            let physical = 0x8000 + (case.address & 0xfff);
            image.map(case.address >> 12, 0x8000, operation.modifies());
            image.data(physical - 1, &[0x5a]);
            image.data(physical, &bytes[..len]);
            image.data(physical + len as u32, &[0x5a]);
            let writes = [(physical, &written[..len])];
            let mut cpu = image.cpu;
            result.apply_flags(&mut cpu, prior_flags());
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("{operation:?}: {}", case.name),
                &code,
                1,
                &image,
                &[Step {
                    cpu,
                    ram: if operation.modifies() { &writes } else { &[] },
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}
