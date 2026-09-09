use crate::support::machine::{check, Exit, Image, Step};
use crate::support::step;
use step::TestModule;
use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step};
use wasmparser::{ExternalKind, Parser, Payload, TypeRef, ValType, Validator};

const MOVES: [(&[u8], usize, u32); 8] = [
    (&[0xb8, 0x78, 0x56, 0x34, 0x12], 24, 0x1234_5678),
    (&[0xb9, 0, 0, 0, 0x80], 28, 0x8000_0000),
    (&[0xba, 0xff, 0xff, 0xff, 0xff], 32, 0xffff_ffff),
    (&[0xbb, 0, 0, 0, 0], 36, 0),
    (&[0xbc, 0xf3, 0x0f, 0xb8, 0x66], 40, 0x66b8_0ff3),
    (&[0xbd, 0xff, 0xff, 0xff, 0x7f], 44, 0x7fff_ffff),
    (&[0xbe, 0xef, 0xbe, 0xad, 0xde], 48, 0xdead_beef),
    (&[0xbf, 0x21, 0x43, 0x65, 0x87], 52, 0x8765_4321),
];

const REGISTERS: [(usize, u32); 8] = [
    (24, 0x1111_1111),
    (28, 0x2222_2222),
    (32, 0x3333_3333),
    (36, 0x4444_4444),
    (40, 0x5555_5555),
    (44, 0x6666_6666),
    (48, 0x7777_7777),
    (52, 0x8888_8888),
];

fn state(eip: u32) -> [u8; 152] {
    let mut bytes = [0xa5; 152];
    for (offset, value) in REGISTERS {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [(56, eip), (144, 0xffff_ffff), (148, 0)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn interpreter_step_exposes_the_cpu_ram_page_map_and_dispatch_abi() {
    let module = compile_interpreter_step().unwrap();
    assert_eq!(module.entry, "step");
    Validator::new().validate_all(&module.bytes).unwrap();
    let mut types = Vec::new();
    let mut memories = Vec::new();
    let mut functions = Vec::new();
    let mut imports = Vec::new();
    let mut exported = None;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for ty in section.into_iter_err_on_gc_types() {
                    let ty = ty.unwrap();
                    types.push((ty.params().to_vec(), ty.results().to_vec()));
                }
            }
            Payload::ImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    assert_eq!(import.module, "wasm86");
                    match import.ty {
                        TypeRef::Memory(memory) => {
                            assert!(!memory.memory64 && !memory.shared);
                            memories.push((import.name.to_owned(), memory.initial));
                        }
                        TypeRef::Func(signature) => {
                            imports.push((import.name.to_owned(), signature))
                        }
                        _ => panic!("unexpected resource import"),
                    }
                }
            }
            Payload::FunctionSection(section) => {
                functions.extend(section.into_iter().map(Result::unwrap))
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    assert_eq!(export.kind, ExternalKind::Func);
                    assert_eq!(export.name, "step");
                    assert!(exported.replace(export.index).is_none());
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        memories,
        [
            ("cpuState".into(), 1),
            ("guest".into(), 1),
            ("machine".into(), 64)
        ]
    );
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].0, "dispatch");
    assert_eq!(
        types[imports[0].1 as usize],
        (vec![ValType::I32], vec![ValType::I64])
    );
    let entry = exported.unwrap() as usize - imports.len();
    assert_eq!(
        types[functions[entry] as usize],
        (vec![], vec![ValType::I64])
    );
}

#[test]
fn register_moves_and_forwarded_values() {
    let step = TestModule::interpreter();
    for (source, &(_, value)) in REGISTERS.iter().enumerate() {
        for (destination, &(offset, _)) in REGISTERS.iter().enumerate() {
            for bytes in [
                [0x89, 0xc0 | ((source as u8) << 3) | destination as u8],
                [0x8b, 0xc0 | ((destination as u8) << 3) | source as u8],
            ] {
                let label = format!(
                    "opcode {:02x}, source {source}, destination {destination}",
                    bytes[0]
                );
                let image_name = &label;
                let image = Image {
                    cpu: state(0x1234),
                    guest: vec![(0x3234, bytes.to_vec())],
                    machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
                };
                let updates = [(offset, value), (56, 0x1236), (144, 0)];
                check(
                    step,
                    image_name,
                    &image,
                    &[Step {
                        cpu: &updates,
                        ram: &[],
                        exit: Exit::Dispatch(4662),
                    }],
                );
                let snapshot =
                    TestModule::new(&compile_block_from_bytes(0x1234, &bytes, 1).unwrap());
                check(
                    &snapshot,
                    image_name,
                    &image,
                    &[Step {
                        cpu: &updates,
                        ram: &[],
                        exit: Exit::Dispatch(4662),
                    }],
                );
            }
        }
    }

    let rotation = &[
        0x89, 0xc2, 0x8b, 0xc1, 0x89, 0xd1, 0x8b, 0xf8, 0x89, 0xce, 0x8b, 0xda,
    ];
    let image_name = "register rotation retains source values";
    let image = Image {
        cpu: state(0x1000),
        guest: vec![(0x3000, (rotation).to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
    };
    check(
        step,
        image_name,
        &image,
        &[
            Step {
                cpu: &[(32, 0x1111_1111), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(4098),
            },
            Step {
                cpu: &[(24, 0x2222_2222), (56, 0x1004), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(4100),
            },
            Step {
                cpu: &[(28, 0x1111_1111), (56, 0x1006), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(4102),
            },
            Step {
                cpu: &[(52, 0x2222_2222), (56, 0x1008), (144, 3)],
                ram: &[],
                exit: Exit::Dispatch(4104),
            },
            Step {
                cpu: &[(48, 0x1111_1111), (56, 0x100a), (144, 4)],
                ram: &[],
                exit: Exit::Dispatch(4106),
            },
            Step {
                cpu: &[(36, 0x1111_1111), (56, 0x100c), (144, 5)],
                ram: &[],
                exit: Exit::Dispatch(4108),
            },
        ],
    );
    let snapshot = TestModule::new(&compile_block_from_bytes(0x1000, rotation, 6).unwrap());
    check(
        &snapshot,
        image_name,
        &image,
        &[Step {
            cpu: &[
                (24, 0x2222_2222),
                (28, 0x1111_1111),
                (32, 0x1111_1111),
                (36, 0x1111_1111),
                (48, 0x1111_1111),
                (52, 0x2222_2222),
                (56, 0x100c),
                (144, 5),
            ],
            ram: &[],
            exit: Exit::Dispatch(4108),
        }],
    );

    let forward = &[0xb8, 42, 0, 0, 0, 0x89, 0xc1, 0x8b, 0xd1, 0x89, 0xd3];
    let image_name = "immediate definition forwards through register copies";
    let image = Image {
        cpu: state(0x1000),
        guest: vec![(0x3000, (forward).to_vec())],
        machine: image.machine.clone(),
    };
    check(
        step,
        image_name,
        &image,
        &[
            Step {
                cpu: &[(24, 42), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(4101),
            },
            Step {
                cpu: &[(28, 42), (56, 0x1007), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(4103),
            },
            Step {
                cpu: &[(32, 42), (56, 0x1009), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(4105),
            },
            Step {
                cpu: &[(36, 42), (56, 0x100b), (144, 3)],
                ram: &[],
                exit: Exit::Dispatch(4107),
            },
        ],
    );
    let snapshot = TestModule::new(&compile_block_from_bytes(0x1000, forward, 4).unwrap());
    check(
        &snapshot,
        image_name,
        &image,
        &[Step {
            cpu: &[
                (24, 42),
                (28, 42),
                (32, 42),
                (36, 42),
                (56, 0x100b),
                (144, 3),
            ],
            ram: &[],
            exit: Exit::Dispatch(4107),
        }],
    );

    let old_value = &[0x89, 0xc1, 0xb8, 9, 0, 0, 0, 0x8b, 0xd1];
    let image_name = "earlier copy survives replacing its source register";
    let image = Image {
        cpu: state(0x1000),
        guest: vec![(0x3000, (old_value).to_vec())],
        machine: image.machine.clone(),
    };
    check(
        step,
        image_name,
        &image,
        &[
            Step {
                cpu: &[(28, 0x1111_1111), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(4098),
            },
            Step {
                cpu: &[(24, 9), (56, 0x1007), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(4103),
            },
            Step {
                cpu: &[(32, 0x1111_1111), (56, 0x1009), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(4105),
            },
        ],
    );
    let snapshot = TestModule::new(&compile_block_from_bytes(0x1000, old_value, 3).unwrap());
    check(
        &snapshot,
        image_name,
        &image,
        &[Step {
            cpu: &[
                (24, 9),
                (28, 0x1111_1111),
                (32, 0x1111_1111),
                (56, 0x1009),
                (144, 2),
            ],
            ram: &[],
            exit: Exit::Dispatch(4105),
        }],
    );

    let complete_name = "complete register MOV does not fetch a following page";
    let complete = Image {
        cpu: state(0x1ffe),
        guest: vec![(0x3ffe, [0x89, 0xc1].to_vec())],
        machine: image.machine.clone(),
    };
    check(
        step,
        complete_name,
        &complete,
        &[Step {
            cpu: &[(28, 0x1111_1111), (56, 0x2000), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8192),
        }],
    );
    let scattered_name = "ModRM is in a nonadjacent physical frame";
    let scattered = Image {
        cpu: state(0x1fff),
        guest: vec![(0x3fff, [0x8b].to_vec()), (0x1000, [0xf8].to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0, 1, 0x10, 0, 0].to_vec())],
    };
    check(
        step,
        scattered_name,
        &scattered,
        &[Step {
            cpu: &[(52, 0x1111_1111), (56, 0x2001), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8193),
        }],
    );
    let wrapped_name = "register MOV crosses wrapped EIP";
    let wrapped = Image {
        cpu: state(0xffff_ffff),
        guest: vec![(0x3fff, [0x89].to_vec()), (0x1000, [0xc1].to_vec())],
        machine: vec![
            (0x003f_fffc, [1, 0x30, 0, 0].to_vec()),
            (0, [1, 0x10, 0, 0].to_vec()),
        ],
    };
    check(
        step,
        wrapped_name,
        &wrapped,
        &[Step {
            cpu: &[(28, 0x1111_1111), (56, 1), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(1),
        }],
    );
    let snapshot =
        TestModule::new(&compile_block_from_bytes(0xffff_ffff, &[0x89, 0xc1], 1).unwrap());
    check(
        &snapshot,
        wrapped_name,
        &wrapped,
        &[Step {
            cpu: &[(28, 0x1111_1111), (56, 1), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(1),
        }],
    );

    let missing_modrm_name = "missing ModRM preserves the preceding instruction's progress";
    let missing_modrm = Image {
        cpu: state(0x1ffa),
        guest: vec![(0x3ffa, [0xb8, 42, 0, 0, 0, 0x89].to_vec())],
        machine: image.machine.clone(),
    };
    check(
        step,
        missing_modrm_name,
        &missing_modrm,
        &[
            Step {
                cpu: &[(24, 42), (56, 0x1fff), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(8191),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            },
        ],
    );
    let memory_form_name = "missing displacement preserves the preceding instruction progress";
    let memory_form = Image {
        cpu: state(0x1ff9),
        guest: vec![(0x3ff9, [0xb8, 42, 0, 0, 0, 0x89, 0x05].to_vec())],
        machine: image.machine.clone(),
    };
    check(
        step,
        memory_form_name,
        &memory_form,
        &[
            Step {
                cpu: &[(24, 42), (56, 0x1ffe), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(8190),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            },
        ],
    );
}

#[test]
fn immediate_moves_and_fetch_boundaries() {
    let step = TestModule::interpreter();
    // Virtual page 1 maps to frame 3. PRESENT alone permits instruction fetch.
    for &(bytes, offset, value) in &MOVES {
        let image_name = "all register codes";
        let image = Image {
            cpu: state(0x1234),
            guest: vec![(0x3234, (bytes).to_vec())],
            machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
        };
        let updates = [(offset, value), (56, 0x1239), (144, 0)];
        check(
            step,
            image_name,
            &image,
            &[Step {
                cpu: &updates,
                ram: &[],
                exit: Exit::Dispatch(4665),
            }],
        );
        let snapshot = TestModule::new(&compile_block_from_bytes(0x1234, bytes, 1).unwrap());
        check(
            &snapshot,
            image_name,
            &image,
            &[Step {
                cpu: &updates,
                ram: &[],
                exit: Exit::Dispatch(4665),
            }],
        );
    }
    let contiguous_name = "contiguous crossing";
    let contiguous = Image {
        cpu: state(0x1ffd),
        guest: vec![(0x3ffd, (MOVES[0].0).to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0, 1, 0x40, 0, 0].to_vec())],
    };
    check(
        step,
        contiguous_name,
        &contiguous,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 0x2002), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8194),
        }],
    );
    let scattered_name = "scattered immediate";
    let scattered = Image {
        cpu: state(0x1ffd),
        guest: vec![
            (0x3ffd, [0xb8, 0x78, 0x56].to_vec()),
            (0x1000, [0x34, 0x12].to_vec()),
        ],
        machine: vec![(4, [1, 0x30, 0, 0, 1, 0x10, 0, 0].to_vec())],
    };
    check(
        step,
        scattered_name,
        &scattered,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 0x2002), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8194),
        }],
    );
    let snapshot = TestModule::new(&compile_block_from_bytes(0x1ffd, MOVES[0].0, 1).unwrap());
    check(
        &snapshot,
        scattered_name,
        &scattered,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 0x2002), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8194),
        }],
    );
    let split_opcode_name = "opcode and complete immediate in separate frames";
    let split_opcode = Image {
        cpu: state(0x1fff),
        guest: vec![
            (0x3fff, [0xb8].to_vec()),
            (0x1000, [0x78, 0x56, 0x34, 0x12].to_vec()),
        ],
        machine: vec![(4, [1, 0x30, 0, 0, 1, 0x10, 0, 0].to_vec())],
    };
    check(
        step,
        split_opcode_name,
        &split_opcode,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 0x2004), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8196),
        }],
    );
    // The next page is absent; a complete instruction does not fetch beyond its five bytes.
    let exact_end_name = "complete at page end";
    let exact_end = Image {
        cpu: state(0x1ffb),
        guest: vec![(0x3ffb, (MOVES[0].0).to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
    };
    check(
        step,
        exact_end_name,
        &exact_end,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 0x2000), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(8192),
        }],
    );
    let absent_opcode_name = "absent opcode";
    let absent_opcode = Image {
        cpu: state(0x1234),
        guest: vec![],
        machine: vec![(4, [0, 0xf0, 0xff, 0xff].to_vec())],
    };
    check(
        step,
        absent_opcode_name,
        &absent_opcode,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00001234,
                error: 0x10,
            },
        }],
    );
    let missing_immediate_name = "missing first immediate byte";
    let missing_immediate = Image {
        cpu: state(0x1fff),
        guest: vec![(0x3fff, [0xb8].to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
    };
    check(
        step,
        missing_immediate_name,
        &missing_immediate,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        }],
    );
    let partial_immediate_name = "partial immediate";
    let partial_immediate = Image {
        cpu: state(0x1ffd),
        guest: vec![(0x3ffd, [0xb8, 0x78, 0x56].to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
    };
    check(
        step,
        partial_immediate_name,
        &partial_immediate,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        }],
    );
    for (opcode, exit) in [
        (0x90, Exit::Other(0x0008_0090_0000_1fff)),
        (0x67, Exit::Other(0x0008_0067_0000_1fff)),
    ] {
        let unsupported_name = "unsupported opcode at page end";
        let unsupported = Image {
            cpu: state(0x1fff),
            guest: vec![(0x3fff, [opcode].to_vec())],
            machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
        };
        check(
            step,
            unsupported_name,
            &unsupported,
            &[Step {
                cpu: &[],
                ram: &[],
                exit,
            }],
        );
    }
    let wrapped_name = "wrapped instruction";
    let wrapped = Image {
        cpu: state(0xffff_fffd),
        guest: vec![
            (0x3ffd, [0xb8, 0x78, 0x56].to_vec()),
            (0x1000, [0x34, 0x12].to_vec()),
        ],
        machine: vec![
            (0x003f_fffc, [1, 0x30, 0, 0].to_vec()),
            (0, [1, 0x10, 0, 0].to_vec()),
        ],
    };
    check(
        step,
        wrapped_name,
        &wrapped,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 2), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(2),
        }],
    );
    let snapshot = TestModule::new(&compile_block_from_bytes(0xffff_fffd, MOVES[0].0, 1).unwrap());
    check(
        &snapshot,
        wrapped_name,
        &wrapped,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 2), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(2),
        }],
    );
    let wrapped_fault_name = "missing page after EIP wrap";
    let wrapped_fault = Image {
        cpu: wrapped.cpu,
        guest: wrapped.guest.clone(),
        machine: vec![(0x003f_fffc, [1, 0x30, 0, 0].to_vec())],
    };
    check(
        step,
        wrapped_fault_name,
        &wrapped_fault,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::PageFault {
                address: 0x00000000,
                error: 0x10,
            },
        }],
    );
    let high_eip_name = "signed dispatch EIP";
    let high_eip = Image {
        cpu: state(0x7fff_fffd),
        guest: vec![(0x3ffd, (MOVES[0].0).to_vec())],
        machine: vec![(0x001f_fffc, [1, 0x30, 0, 0, 1, 0x40, 0, 0].to_vec())],
    };
    check(
        step,
        high_eip_name,
        &high_eip,
        &[Step {
            cpu: &[(24, 0x1234_5678), (56, 0x8000_0002), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch((-2147483646_i32) as u32),
        }],
    );
    let invalid_frame_name = "present frame outside RAM";
    let invalid_frame = Image {
        cpu: state(0x1000),
        guest: vec![],
        machine: vec![(4, [1, 0, 1, 0].to_vec())],
    };
    check(
        step,
        invalid_frame_name,
        &invalid_frame,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::Trap,
        }],
    );
    let two_name = "one instruction only";
    let two = Image {
        cpu: state(0x1000),
        guest: vec![(0x3000, [0xb8, 42, 0, 0, 0, 0xbf, 7, 0, 0, 0].to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
    };
    check(
        step,
        two_name,
        &two,
        &[Step {
            cpu: &[(24, 42), (56, 0x1005), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(4101),
        }],
    );
    let changed_name = "live bytes differ from snapshot";
    let changed = Image {
        cpu: state(0x1000),
        guest: vec![(0x3000, [0xbf, 7, 0, 0, 0].to_vec())],
        machine: vec![(4, [1, 0x30, 0, 0].to_vec())],
    };
    let snapshot =
        TestModule::new(&compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1).unwrap());
    check(
        step,
        changed_name,
        &changed,
        &[Step {
            cpu: &[(52, 7), (56, 0x1005), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(4101),
        }],
    );
    check(
        &snapshot,
        changed_name,
        &changed,
        &[Step {
            cpu: &[(24, 42), (56, 0x1005), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(4101),
        }],
    );
}
