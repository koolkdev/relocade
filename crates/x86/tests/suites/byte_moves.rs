use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::support::machine;
use crate::support::step;
use machine::{both, check, Exit, Step};
use step::TestModule;

// Each byte name selects a parent register and its low or high byte.
// These expectations are independent of the decoder's register selectors.
const BYTE_REGISTERS: [(&str, Gpr32, usize, u8); 8] = [
    ("AL", Gpr32::Eax, 0, 0x11),
    ("CL", Gpr32::Ecx, 0, 0x55),
    ("DL", Gpr32::Edx, 0, 0x99),
    ("BL", Gpr32::Ebx, 0, 0xdd),
    ("AH", Gpr32::Eax, 1, 0x22),
    ("CH", Gpr32::Ecx, 1, 0x66),
    ("DH", Gpr32::Edx, 1, 0xaa),
    ("BH", Gpr32::Ebx, 1, 0xee),
];
const IMMEDIATES: [u8; 8] = [0x80, 0, 0xff, 0x66, 0x88, 0x8a, 0xb7, 0x7f];

use machine::byte_register_image as byte_image;

fn changed_byte(value: u32, byte: usize, replacement: u8) -> u32 {
    let mut bytes = value.to_le_bytes();
    bytes[byte] = replacement;
    u32::from_le_bytes(bytes)
}

#[test]
fn byte_forms_require_only_the_selected_encoding_bytes() {
    for bytes in [
        &[0xb0][..],
        &[0x88][..],
        &[0x8a, 0x04][..],
        &[0x88, 0x85, 0, 0, 0][..],
    ] {
        assert!(matches!(
            compile_block_from_bytes(0x1000, bytes, 1),
            Err(BlockError::TruncatedInstruction { address: 0x1000, available }) if available == bytes.len()
        ));
    }
    let prefix = [0xb4, 0x88, 0x8a, 0xc4];
    let expected = compile_block_from_bytes(0x1000, &prefix, 2).unwrap();
    for suffix in [&[0xb7][..], &[0x88][..], &[0x66, 0xb0, 0][..]] {
        let mut code = prefix.to_vec();
        code.extend_from_slice(suffix);
        assert_eq!(
            compile_block_from_bytes(0x1000, &code, 2).unwrap().bytes,
            expected.bytes
        );
    }
}

#[test]
fn every_byte_register_encoding_emits_valid_modules() {
    for register in 0..8 {
        Validator::new()
            .validate_all(
                &compile_block_from_bytes(
                    0x1000,
                    &[0xb0 + register, IMMEDIATES[register as usize]],
                    1,
                )
                .unwrap()
                .bytes,
            )
            .unwrap();
        for other in 0..8 {
            for code in [
                [0x88, 0xc0 | (register << 3) | other],
                [0x8a, 0xc0 | (other << 3) | register],
            ] {
                Validator::new()
                    .validate_all(&compile_block_from_bytes(0x1000, &code, 1).unwrap().bytes)
                    .unwrap();
            }
        }
    }
}

#[test]
fn byte_memory_moves_use_byte_guest_accesses() {
    for (code, expected_loads, expected_stores) in
        [(&[0x8a, 0x23][..], 1, 0), (&[0x88, 0x23][..], 0, 1)]
    {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut guest = None;
        let mut memory_index = 0;
        let mut loads = 0;
        let mut stores = 0;
        for payload in Parser::new(0).parse_all(&module.bytes) {
            match payload.unwrap() {
                Payload::ImportSection(section) => {
                    for import in section {
                        let import = import.unwrap();
                        if matches!(import.ty, TypeRef::Memory(_)) {
                            if import.name == "guest" {
                                guest = Some(memory_index);
                            }
                            memory_index += 1;
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    for operation in body.get_operators_reader().unwrap() {
                        match operation.unwrap() {
                            Operator::I32Load8U { memarg } if Some(memarg.memory) == guest => {
                                loads += 1
                            }
                            Operator::I32Store8 { memarg } if Some(memarg.memory) == guest => {
                                stores += 1
                            }
                            Operator::I32Load { memarg } | Operator::I32Store { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                panic!("byte MOV accessed a guest dword")
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(guest.is_some());
        assert_eq!((loads, stores), (expected_loads, expected_stores));
    }
}

#[test]
fn registers() {
    let step = TestModule::interpreter();
    for (register, &(name, parent, byte, _)) in BYTE_REGISTERS.iter().enumerate() {
        let code = [0xb0 + register as u8, IMMEDIATES[register]];
        let image = byte_image(&code);
        let mut expected_cpu = image.cpu;
        expected_cpu.registers[parent] =
            changed_byte(image.cpu.registers[parent], byte, IMMEDIATES[register]);
        expected_cpu.eip = 0x1002;
        expected_cpu.instruction_count = 0;
        both(
            step,
            &format!("immediate to {name}"),
            &code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            }],
        );
    }
    for (source, &(source_name, _, _, value)) in BYTE_REGISTERS.iter().enumerate() {
        for (destination, &(destination_name, parent, byte, _)) in BYTE_REGISTERS.iter().enumerate()
        {
            for code in [
                [0x88, 0xc0 | ((source as u8) << 3) | destination as u8],
                [0x8a, 0xc0 | ((destination as u8) << 3) | source as u8],
            ] {
                let image = byte_image(&code);
                let mut expected_cpu = image.cpu;
                expected_cpu.registers[parent] =
                    changed_byte(image.cpu.registers[parent], byte, value);
                expected_cpu.eip = 0x1002;
                expected_cpu.instruction_count = 0;
                both(
                    step,
                    &format!("{source_name} to {destination_name} via {:02x}", code[0]),
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu: expected_cpu,
                        ram: &[],
                        exit: Exit::Dispatch(0x1002),
                    }],
                );
            }
        }
    }
}

#[test]
fn register_chains() {
    let step = TestModule::interpreter();
    let code = [
        0xb8, 0x78, 0x56, 0x34, 0x12, // MOV EAX, 0x1234_5678
        0xb4, 0xab, // MOV AH, 0xab
        0xb0, 0xcd, // MOV AL, 0xcd
        0x8b, 0xc8, // MOV ECX, EAX
        0xb8, 0x98, 0xba, 0xdc, 0xfe, // MOV EAX, 0xfedc_ba98
    ];
    let image = byte_image(&code);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.eax = 0x1234_5678;
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    });

    expected_cpu.registers.eax = 0x1234_ab78;
    expected_cpu.eip = 0x1007;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1007),
    });

    expected_cpu.registers.eax = 0x1234_abcd;
    expected_cpu.eip = 0x1009;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1009),
    });

    expected_cpu.registers.ecx = 0x1234_abcd;
    expected_cpu.eip = 0x100b;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100b),
    });

    expected_cpu.registers.eax = 0xfedc_ba98;
    expected_cpu.eip = 0x1010;
    expected_cpu.instruction_count = 4;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1010),
    });

    both(
        step,
        "interleaved full and partial definitions",
        &code,
        5,
        &image,
        &steps,
    );
    let code = [
        0x8a, 0xcc, // MOV CL, AH
        0xb4, 0xff, // MOV AH, 0xff
        0x88, 0xc4, // MOV AH, AL
    ];
    let image = byte_image(&code);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.ecx = 0x8877_6622;
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1002),
    });

    expected_cpu.registers.eax = 0x4433_ff11;
    expected_cpu.eip = 0x1004;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1004),
    });

    expected_cpu.registers.eax = 0x4433_1111;
    expected_cpu.eip = 0x1006;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1006),
    });

    both(
        step,
        "old high byte survives later alias writes",
        &code,
        3,
        &image,
        &steps,
    );
}

#[test]
fn memory() {
    let step = TestModule::interpreter();
    for (register, &(name, parent, byte, value)) in BYTE_REGISTERS.iter().enumerate() {
        for opcode in [0x8a, 0x88] {
            let code = [opcode, ((register as u8) << 3) | 3];
            let mut image = byte_image(&code);
            image.cpu.registers.ebx = 0x4020;
            image.map(4, 0x8000, opcode == 0x88);
            image.data(0x801f, &[0xa5, 0x80, 0x5a]);
            let mut expected_cpu = image.cpu;
            expected_cpu.eip = 0x1002;
            expected_cpu.instruction_count = 0;
            let stored = match register {
                3 => 0x20,
                7 => 0x40,
                _ => value,
            };
            let bytes = [stored];
            let ram = if opcode == 0x8a {
                expected_cpu.registers[parent] =
                    changed_byte(image.cpu.registers[parent], byte, 0x80);
                vec![]
            } else {
                vec![(0x8020, &bytes[..])]
            };
            both(
                step,
                &format!("memory and {name} via {opcode:02x}"),
                &code,
                1,
                &image,
                &[Step {
                    cpu: expected_cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1002),
                }],
            );
        }
    }
    for (name, code, registers, physical, register_changes, stored) in [
        (
            "high destination overlaps its address base",
            &[0x8a, 0x20][..],
            &[(Gpr32::Eax, 0x4000)][..],
            0x8000,
            &[(Gpr32::Eax, 0x0000_8000)][..],
            None,
        ),
        (
            "high source overlaps its address base",
            &[0x88, 0x20][..],
            &[(Gpr32::Eax, 0x4020)][..],
            0x8020,
            &[][..],
            Some(0x40),
        ),
        (
            "scaled address uses old full destination",
            &[0x8a, 0x64, 0x88, 0x10][..],
            &[(Gpr32::Eax, 0x3ff0), (Gpr32::Ecx, 4)][..],
            0x8010,
            &[(Gpr32::Eax, 0x0000_80f0)][..],
            None,
        ),
        (
            "SIB without a base stores the high index byte",
            &[0x88, 0x2c, 0x8d, 0, 0x40, 0, 0][..],
            &[(Gpr32::Ecx, 0x104)][..],
            0x8410,
            &[][..],
            Some(1),
        ),
    ] {
        let mut image = byte_image(code);
        for &(register, value) in registers {
            image.cpu.registers[register] = value;
        }
        image.map(4, 0x8000, true);
        image.data(physical - 1, &[0xa5, 0x80, 0x5a]);
        let next = 0x1000 + code.len() as u32;
        let mut expected_cpu = image.cpu;
        for &(register, value) in register_changes {
            expected_cpu.registers[register] = value;
        }
        expected_cpu.eip = next;
        expected_cpu.instruction_count = 0;
        let stored_byte = [stored.unwrap_or(0)];
        let ram = if stored.is_some() {
            vec![(physical, &stored_byte[..])]
        } else {
            vec![]
        };
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &ram,
                exit: Exit::Dispatch(next),
            }],
        );
    }
    for address in [0x4fff, 0xffff_ffff] {
        for opcode in [0x8a, 0x88] {
            let code = [opcode, 0x13];
            let mut image = byte_image(&code);
            image.cpu.registers.ebx = address;
            image.map(address >> 12, 0x8000, true);
            image.data(0x8ffe, &[0xa5, 0x80]);
            let mut expected_cpu = image.cpu;
            if opcode == 0x8a {
                expected_cpu.registers.edx = 0xccbb_aa80;
            }
            expected_cpu.eip = 0x1002;
            expected_cpu.instruction_count = 0;
            let ram = if opcode == 0x88 {
                vec![(0x8fff, &[0x99][..])]
            } else {
                vec![]
            };
            both(
                step,
                "one-byte access needs no following page",
                &code,
                1,
                &image,
                &[Step {
                    cpu: expected_cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1002),
                }],
            );
        }
    }
    for (name, opcode, permissions, fault) in [
        (
            "missing byte read",
            0x8a,
            None,
            Exit::PageFault {
                address: 0x00004020,
                error: 0x0,
            },
        ),
        (
            "missing byte write",
            0x88,
            None,
            Exit::PageFault {
                address: 0x00004020,
                error: 0x2,
            },
        ),
        (
            "readonly byte write",
            0x88,
            Some(false),
            Exit::PageFault {
                address: 0x00004020,
                error: 0x3,
            },
        ),
    ] {
        let code = [opcode, 0x23];
        let mut image = byte_image(&code);
        image.cpu.registers.ebx = 0x4020;
        if let Some(writable) = permissions {
            image.map(4, 0x8000, writable);
        }
        image.data(0x801f, &[0xa5, 0x80, 0x5a]);
        let expected_cpu = image.cpu;
        both(
            step,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: fault,
            }],
        );
    }
    let code = [0x8a, 0x23];
    let mut image = byte_image(&code);
    image.cpu.registers.ebx = 0x4000;
    image.map(4, 0x10000, false);
    let expected_cpu = image.cpu;
    both(
        step,
        "present byte frame outside RAM traps",
        &code,
        1,
        &image,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Trap,
        }],
    );
}

#[test]
fn memory_chains() {
    let step = TestModule::interpreter();
    let code = [0xb3, 0x20, 0x8a, 0x23, 0x89, 0x01];
    let mut image = byte_image(&code);
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ecx = 0x5000;
    image.map(4, 0x8000, true);
    image.map(5, 0x9000, true);
    image.data(0x801f, &[0xa5, 0x80, 0x5a]);
    image.data(0x8fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.ebx = 0x4020;
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1002),
    });

    expected_cpu.registers.eax = 0x4433_8011;
    expected_cpu.eip = 0x1004;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1004),
    });

    expected_cpu.eip = 0x1006;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x9000, &[0x11, 0x80, 0x33, 0x44])],
        exit: Exit::Dispatch(0x1006),
    });

    both(
        step,
        "partial definition feeds address and full-register source",
        &code,
        3,
        &image,
        &steps,
    );
    image.machine.truncate(1);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.ebx = 0x4020;
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1002),
    });

    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x00004020,
            error: 0x0,
        },
    });

    both(
        step,
        "data fault publishes only the completed byte definition",
        &code,
        3,
        &image,
        &steps,
    );
    let code = [0x8a, 0x23, 0x88, 0x11, 0x8a, 0xf4];
    let mut image = byte_image(&code);
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ecx = 0x6000;
    image.map(4, 0x8000, true);
    image.map(6, 0x8000, true);
    image.data(0x7fff, &[0xa5, 0x80, 0x5a]);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.eax = 0x4433_8011;
    expected_cpu.eip = 0x1002;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1002),
    });

    expected_cpu.eip = 0x1004;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[(0x8000, &[0x99])],
        exit: Exit::Dispatch(0x1004),
    });

    expected_cpu.registers.edx = 0xccbb_8099;
    expected_cpu.eip = 0x1006;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1006),
    });

    both(
        step,
        "byte snapshot survives an aliased guest store",
        &code,
        3,
        &image,
        &steps,
    );
}

#[test]
fn instruction_boundaries() {
    let step = TestModule::interpreter();
    for (name, start, code) in [
        ("missing immediate", 0x1fff, &[0xb7][..]),
        ("missing ModRM", 0x1fff, &[0x8a][..]),
        (
            "missing SIB before byte data access",
            0x1ffe,
            &[0x88, 0x04][..],
        ),
    ] {
        let mut image = byte_image(&[]);
        image.cpu.eip = start;
        image.cpu.registers.eax = 0x4000;
        image.data(0x3000 + (start & 0xfff), code);
        let expected_cpu = image.cpu;
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
    let code = [0xb4, 0x88];
    let mut image = byte_image(&[]);
    image.cpu.eip = 0x1ffe;
    image.data(0x3ffe, &code);
    let mut expected_cpu = image.cpu;
    expected_cpu.registers.eax = 0x4433_8811;
    expected_cpu.eip = 0x2000;
    expected_cpu.instruction_count = 0;
    both(
        step,
        "complete immediate at mapped page end",
        &code,
        1,
        &image,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(0x2000),
        }],
    );
    let code = [0xb7, 0x8a];
    let mut image = byte_image(&[]);
    image.cpu.eip = 0xffff_ffff;
    image.map(0xfffff, 0x8000, false);
    image.map(0, 0xa000, false);
    image.data(0x8fff, &code[..1]);
    image.data(0xa000, &code[1..]);
    let mut expected_cpu = image.cpu;
    expected_cpu.registers.ebx = 0x10ff_8add;
    expected_cpu.eip = 1;
    expected_cpu.instruction_count = 0;
    both(
        step,
        "byte immediate fetch wraps EIP",
        &code,
        1,
        &image,
        &[Step {
            cpu: expected_cpu,
            ram: &[],
            exit: Exit::Dispatch(1),
        }],
    );
}
