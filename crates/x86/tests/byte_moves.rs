use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, BlockError, CompiledModule};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
#[path = "support/machine.rs"]
mod machine;
use machine::{both, check, Exit, Image, Step};

// The byte offsets and initial values are architectural expectations, independent
// of the decoder's register selectors and the state owner's alias representation.
const BYTE_REGISTERS: [(&str, usize, u8); 8] = [
    ("AL", 24, 0x11),
    ("CL", 28, 0x55),
    ("DL", 32, 0x99),
    ("BL", 36, 0xdd),
    ("AH", 25, 0x22),
    ("CH", 29, 0x66),
    ("DH", 33, 0xaa),
    ("BH", 37, 0xee),
];
const IMMEDIATES: [u8; 8] = [0x80, 0, 0xff, 0x66, 0x88, 0x8a, 0xb7, 0x7f];

fn byte_image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    for (offset, value) in [
        (24, 0x4433_2211),
        (28, 0x8877_6655),
        (32, 0xccbb_aa99),
        (36, 0x10ff_eedd),
    ] {
        image.register(offset, value);
    }
    image
}

fn changed_byte(image: &Image, offset: usize, value: u8) -> (usize, u32) {
    let word_offset = offset / 4 * 4;
    let mut bytes: [u8; 4] = image.cpu[word_offset..word_offset + 4].try_into().unwrap();
    bytes[offset - word_offset] = value;
    (word_offset, u32::from_le_bytes(bytes))
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

fn check_registers(flags: &[&str], step: &ModuleFile) {
    for (register, &(name, offset, _)) in BYTE_REGISTERS.iter().enumerate() {
        let code = [0xb0 + register as u8, IMMEDIATES[register]];
        let image = byte_image(&code);
        both(
            step,
            flags,
            &format!("immediate to {name}"),
            &code,
            1,
            &image,
            &[Step {
                cpu: &[
                    changed_byte(&image, offset, IMMEDIATES[register]),
                    (56, 0x1002),
                    (144, 0),
                ],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            }],
        );
    }
    for (source, &(source_name, _, value)) in BYTE_REGISTERS.iter().enumerate() {
        for (destination, &(destination_name, offset, _)) in BYTE_REGISTERS.iter().enumerate() {
            for code in [
                [0x88, 0xc0 | ((source as u8) << 3) | destination as u8],
                [0x8a, 0xc0 | ((destination as u8) << 3) | source as u8],
            ] {
                let image = byte_image(&code);
                both(
                    step,
                    flags,
                    &format!("{source_name} to {destination_name} via {:02x}", code[0]),
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu: &[changed_byte(&image, offset, value), (56, 0x1002), (144, 0)],
                        ram: &[],
                        exit: Exit::Dispatch(0x1002),
                    }],
                );
            }
        }
    }
}

fn check_register_chains(flags: &[&str], step: &ModuleFile) {
    let code = [
        0xb8, 0x78, 0x56, 0x34, 0x12, 0xb4, 0xab, 0xb0, 0xcd, 0x8b, 0xc8, 0xb8, 0x98, 0xba, 0xdc,
        0xfe,
    ];
    both(
        step,
        flags,
        "interleaved full and partial definitions",
        &code,
        5,
        &byte_image(&code),
        &[
            Step {
                cpu: &[(24, 0x1234_5678), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[(24, 0x1234_ab78), (56, 0x1007), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1007),
            },
            Step {
                cpu: &[(24, 0x1234_abcd), (56, 0x1009), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1009),
            },
            Step {
                cpu: &[(28, 0x1234_abcd), (56, 0x100b), (144, 3)],
                ram: &[],
                exit: Exit::Dispatch(0x100b),
            },
            Step {
                cpu: &[(24, 0xfedc_ba98), (56, 0x1010), (144, 4)],
                ram: &[],
                exit: Exit::Dispatch(0x1010),
            },
        ],
    );
    let code = [0x8a, 0xcc, 0xb4, 0xff, 0x88, 0xc4];
    both(
        step,
        flags,
        "old high byte survives later alias writes",
        &code,
        3,
        &byte_image(&code),
        &[
            Step {
                cpu: &[(28, 0x8877_6622), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            },
            Step {
                cpu: &[(24, 0x4433_ff11), (56, 0x1004), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1004),
            },
            Step {
                cpu: &[(24, 0x4433_1111), (56, 0x1006), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1006),
            },
        ],
    );
}

fn check_memory(flags: &[&str], step: &ModuleFile) {
    for (register, &(name, offset, value)) in BYTE_REGISTERS.iter().enumerate() {
        for opcode in [0x8a, 0x88] {
            let code = [opcode, ((register as u8) << 3) | 3];
            let mut image = byte_image(&code);
            image.register(36, 0x4020);
            image.map(4, 0x8000, opcode == 0x88);
            image.data(0x801f, &[0xa5, 0x80, 0x5a]);
            let mut cpu = vec![(56, 0x1002), (144, 0)];
            let stored = match register {
                3 => 0x20,
                7 => 0x40,
                _ => value,
            };
            let bytes = [stored];
            let ram = if opcode == 0x8a {
                cpu.push(changed_byte(&image, offset, 0x80));
                vec![]
            } else {
                vec![(0x8020, &bytes[..])]
            };
            both(
                step,
                flags,
                &format!("memory and {name} via {opcode:02x}"),
                &code,
                1,
                &image,
                &[Step {
                    cpu: &cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1002),
                }],
            );
        }
    }
    for (name, code, registers, physical, cpu, stored) in [
        (
            "high destination overlaps its address base",
            &[0x8a, 0x20][..],
            &[(24, 0x4000)][..],
            0x8000,
            &[(24, 0x0000_8000)][..],
            None,
        ),
        (
            "high source overlaps its address base",
            &[0x88, 0x20][..],
            &[(24, 0x4020)][..],
            0x8020,
            &[][..],
            Some(0x40),
        ),
        (
            "scaled address uses old full destination",
            &[0x8a, 0x64, 0x88, 0x10][..],
            &[(24, 0x3ff0), (28, 4)][..],
            0x8010,
            &[(24, 0x0000_80f0)][..],
            None,
        ),
        (
            "SIB without a base stores the high index byte",
            &[0x88, 0x2c, 0x8d, 0, 0x40, 0, 0][..],
            &[(28, 0x104)][..],
            0x8410,
            &[][..],
            Some(1),
        ),
    ] {
        let mut image = byte_image(code);
        for &(offset, value) in registers {
            image.register(offset, value);
        }
        image.map(4, 0x8000, true);
        image.data(physical - 1, &[0xa5, 0x80, 0x5a]);
        let next = 0x1000 + code.len() as u32;
        let mut cpu = cpu.to_vec();
        cpu.extend([(56, next), (144, 0)]);
        let stored_byte = [stored.unwrap_or(0)];
        let ram = if stored.is_some() {
            vec![(physical, &stored_byte[..])]
        } else {
            vec![]
        };
        both(
            step,
            flags,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &cpu,
                ram: &ram,
                exit: Exit::Dispatch(next),
            }],
        );
    }
    for address in [0x4fff, 0xffff_ffff] {
        for opcode in [0x8a, 0x88] {
            let code = [opcode, 0x13];
            let mut image = byte_image(&code);
            image.register(36, address);
            image.map(address >> 12, 0x8000, true);
            image.data(0x8ffe, &[0xa5, 0x80]);
            let cpu = if opcode == 0x8a {
                vec![(32, 0xccbb_aa80), (56, 0x1002), (144, 0)]
            } else {
                vec![(56, 0x1002), (144, 0)]
            };
            let ram = if opcode == 0x88 {
                vec![(0x8fff, &[0x99][..])]
            } else {
                vec![]
            };
            both(
                step,
                flags,
                "one-byte access needs no following page",
                &code,
                1,
                &image,
                &[Step {
                    cpu: &cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1002),
                }],
            );
        }
    }
    for (name, opcode, permissions, fault) in [
        ("missing byte read", 0x8a, None, 0x0004_0000_0000_4020),
        ("missing byte write", 0x88, None, 0x0004_0002_0000_4020),
        (
            "readonly byte write",
            0x88,
            Some(false),
            0x0004_0003_0000_4020,
        ),
    ] {
        let code = [opcode, 0x23];
        let mut image = byte_image(&code);
        image.register(36, 0x4020);
        if let Some(writable) = permissions {
            image.map(4, 0x8000, writable);
        }
        image.data(0x801f, &[0xa5, 0x80, 0x5a]);
        both(
            step,
            flags,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(fault),
            }],
        );
    }
    let code = [0x8a, 0x23];
    let mut image = byte_image(&code);
    image.register(36, 0x4000);
    image.map(4, 0x10000, false);
    both(
        step,
        flags,
        "present byte frame outside RAM traps",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::Trap,
        }],
    );
}

fn check_memory_chains(flags: &[&str], step: &ModuleFile) {
    let code = [0xb3, 0x20, 0x8a, 0x23, 0x89, 0x01];
    let mut image = byte_image(&code);
    image.register(36, 0x4000);
    image.register(28, 0x5000);
    image.map(4, 0x8000, true);
    image.map(5, 0x9000, true);
    image.data(0x801f, &[0xa5, 0x80, 0x5a]);
    image.data(0x8fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
    both(
        step,
        flags,
        "partial definition feeds address and full-register source",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(36, 0x4020), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            },
            Step {
                cpu: &[(24, 0x4433_8011), (56, 0x1004), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1004),
            },
            Step {
                cpu: &[(56, 0x1006), (144, 2)],
                ram: &[(0x9000, &[0x11, 0x80, 0x33, 0x44])],
                exit: Exit::Dispatch(0x1006),
            },
        ],
    );
    image.machine.truncate(1);
    both(
        step,
        flags,
        "data fault publishes only the completed byte definition",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(36, 0x4020), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(0x0004_0000_0000_4020),
            },
        ],
    );
    let code = [0x8a, 0x23, 0x88, 0x11, 0x8a, 0xf4];
    let mut image = byte_image(&code);
    image.register(36, 0x4000);
    image.register(28, 0x6000);
    image.map(4, 0x8000, true);
    image.map(6, 0x8000, true);
    image.data(0x7fff, &[0xa5, 0x80, 0x5a]);
    both(
        step,
        flags,
        "byte snapshot survives an aliased guest store",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(24, 0x4433_8011), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            },
            Step {
                cpu: &[(56, 0x1004), (144, 1)],
                ram: &[(0x8000, &[0x99])],
                exit: Exit::Dispatch(0x1004),
            },
            Step {
                cpu: &[(32, 0xccbb_8099), (56, 0x1006), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1006),
            },
        ],
    );
}

fn check_instruction_boundaries(flags: &[&str], step: &ModuleFile) {
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
        image.register(56, start);
        image.register(24, 0x4000);
        image.data(0x3000 + (start & 0xfff), code);
        check(
            step,
            flags,
            name,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(0x0004_0010_0000_2000),
            }],
        );
    }
    let code = [0xb4, 0x88];
    let mut image = byte_image(&[]);
    image.register(56, 0x1ffe);
    image.data(0x3ffe, &code);
    both(
        step,
        flags,
        "complete immediate at mapped page end",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(24, 0x4433_8811), (56, 0x2000), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x2000),
        }],
    );
    let code = [0xb7, 0x8a];
    let mut image = byte_image(&[]);
    image.register(56, 0xffff_ffff);
    image.map(0xfffff, 0x8000, false);
    image.map(0, 0xa000, false);
    image.data(0x8fff, &code[..1]);
    image.data(0xa000, &code[1..]);
    both(
        step,
        flags,
        "byte immediate fetch wraps EIP",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(36, 0x10ff_8add), (56, 1), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(1),
        }],
    );
}

fn execute_byte_moves(flags: &[&str]) {
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let step = ModuleFile::new(&module);
    check_registers(flags, &step);
    check_register_chains(flags, &step);
    check_memory(flags, &step);
    check_memory_chains(flags, &step);
    check_instruction_boundaries(flags, &step);
}

#[test]
#[ignore = "requires Node.js 24 with WebAssembly tail calls"]
fn byte_moves_execute_in_v8() {
    execute_byte_moves(&[]);
}

#[test]
#[ignore = "requires Node.js 24 with WebAssembly tail calls"]
fn byte_moves_execute_in_optimizing_v8() {
    execute_byte_moves(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
