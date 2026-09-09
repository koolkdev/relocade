use wasm86_x86::compile_block_from_bytes;
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::support::machine;
use crate::support::step;
use machine::{both, Exit, Image, Step};
use step::TestModule;

// Word views occupy the low two bytes of each complete architectural register.
const REGISTERS: [(&str, usize, u32); 8] = [
    ("AX", 24, 0x4433_2211),
    ("CX", 28, 0x8877_6655),
    ("DX", 32, 0xccbb_aa99),
    ("BX", 36, 0x10ff_eedd),
    ("SP", 40, 0x7654_3210),
    ("BP", 44, 0xfedc_ba98),
    ("SI", 48, 0x0123_4567),
    ("DI", 52, 0x89ab_cdef),
];

fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    for (_, offset, value) in REGISTERS {
        image.register(offset, value);
    }
    image
}

#[test]
fn word_memory_moves_use_word_guest_accesses() {
    for (code, expected_loads, expected_stores) in [
        (&[0x66, 0x8b, 0x03][..], 1, 0),
        (&[0x66, 0x89, 0x03][..], 0, 1),
    ] {
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
                            Operator::I32Load16U { memarg } if Some(memarg.memory) == guest => {
                                loads += 1;
                            }
                            Operator::I32Store16 { memarg } if Some(memarg.memory) == guest => {
                                stores += 1;
                            }
                            Operator::I32Load { memarg } | Operator::I32Store { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                panic!("word MOV accessed a guest dword");
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
    for (register, immediate, expected) in [
        (0, 0x8000_u16, 0x4433_8000),
        (1, 0, 0x8877_0000),
        (2, 0xffff, 0xccbb_ffff),
        (3, 0x6688, 0x10ff_6688),
        (4, 0x1234, 0x7654_1234),
        (5, 0xc7a1, 0xfedc_c7a1),
        (6, 0x7fff, 0x0123_7fff),
        (7, 0xb88a, 0x89ab_b88a),
    ] {
        let [low, high] = immediate.to_le_bytes();
        for code in [
            vec![0x66, 0xb8 + register, low, high],
            vec![0x66, 0xc7, 0xc0 + register, low, high],
        ] {
            let (name, offset, _) = REGISTERS[register as usize];
            let next_eip = 0x1000 + code.len() as u32;
            both(
                step,
                &format!("word immediate to {name} through {:02x}", code[1]),
                &code,
                1,
                &image(&code),
                &[Step {
                    cpu: &[(offset, expected), (56, next_eip), (144, 0)],
                    ram: &[],
                    exit: Exit::Dispatch(next_eip),
                }],
            );
        }
    }
    // The permutation places every code in both operand roles, including SP–DI.
    for (source, destination, expected) in [
        (0, 4, 0x7654_2211),
        (1, 5, 0xfedc_6655),
        (2, 6, 0x0123_aa99),
        (3, 7, 0x89ab_eedd),
        (4, 0, 0x4433_3210),
        (5, 1, 0x8877_ba98),
        (6, 2, 0xccbb_4567),
        (7, 3, 0x10ff_cdef),
        (0, 0, 0x4433_2211),
        (7, 7, 0x89ab_cdef),
    ] {
        for code in [
            [0x66, 0x89, 0xc0 | (source << 3) | destination],
            [0x66, 0x8b, 0xc0 | (destination << 3) | source],
        ] {
            both(
                step,
                &format!(
                    "{} to {} through {:02x}",
                    REGISTERS[source as usize].0, REGISTERS[destination as usize].0, code[1]
                ),
                &code,
                1,
                &image(&code),
                &[Step {
                    cpu: &[
                        (REGISTERS[destination as usize].1, expected),
                        (56, 0x1003),
                        (144, 0),
                    ],
                    ram: &[],
                    exit: Exit::Dispatch(0x1003),
                }],
            );
        }
    }
}

#[test]
fn mixed_register_views() {
    let step = TestModule::interpreter();
    let code = [
        0x66, 0xb8, 0x34, 0x12, // mov ax, 1234
        0xb4, 0x56, // mov ah, 56
        0x66, 0x89, 0xc1, // mov cx, ax
        0xb0, 0x78, // mov al, 78
        0xb8, 0xaa, 0xbb, 0xcc, 0xdd, // mov eax, ddccbbaa
        0x66, 0x89, 0xca, // mov dx, cx
    ];
    both(
        step,
        "word snapshot survives byte and dword overwrites",
        &code,
        6,
        &image(&code),
        &[
            Step {
                cpu: &[(24, 0x4433_1234), (56, 0x1004), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1004),
            },
            Step {
                cpu: &[(24, 0x4433_5634), (56, 0x1006), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1006),
            },
            Step {
                cpu: &[(28, 0x8877_5634), (56, 0x1009), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1009),
            },
            Step {
                cpu: &[(24, 0x4433_5678), (56, 0x100b), (144, 3)],
                ram: &[],
                exit: Exit::Dispatch(0x100b),
            },
            Step {
                cpu: &[(24, 0xddcc_bbaa), (56, 0x1010), (144, 4)],
                ram: &[],
                exit: Exit::Dispatch(0x1010),
            },
            Step {
                cpu: &[(32, 0xccbb_5634), (56, 0x1013), (144, 5)],
                ram: &[],
                exit: Exit::Dispatch(0x1013),
            },
        ],
    );
}

#[test]
fn addressed_words() {
    let step = TestModule::interpreter();
    for (name, code, registers, expected_cpu, expected_ram) in [
        (
            "base read",
            &[0x66, 0x8b, 0x03][..],
            &[(36, 0x4020)][..],
            &[(24, 0x4433_88a1)][..],
            &[][..],
        ),
        (
            "load uses old destination as base",
            &[0x66, 0x8b, 0x00][..],
            &[(24, 0x4020)][..],
            &[(24, 0x0000_88a1)][..],
            &[][..],
        ),
        (
            "store source is also its address register",
            &[0x66, 0x89, 0x00][..],
            &[(24, 0x4020)][..],
            &[][..],
            &[(0x8020, &[0x20, 0x40][..])][..],
        ),
        (
            "negative disp8",
            &[0x66, 0x89, 0x43, 0x80][..],
            &[(36, 0x40a0)][..],
            &[][..],
            &[(0x8020, &[0x11, 0x22][..])][..],
        ),
        (
            "scaled wrapped address",
            &[0x66, 0xc7, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0xa1, 0x88][..],
            &[(36, 0xffff_fff0), (28, 4)][..],
            &[][..],
            &[(0x8020, &[0xa1, 0x88][..])][..],
        ),
        (
            "SIB without base or index",
            &[0x66, 0xc7, 0x04, 0x25, 0x20, 0x40, 0, 0, 0, 0x80][..],
            &[][..],
            &[][..],
            &[(0x8020, &[0, 0x80][..])][..],
        ),
    ] {
        let mut image = image(code);
        for &(offset, value) in registers {
            image.register(offset, value);
        }
        image.map(4, 0x8000, true);
        image.data(0x801f, &[0xa5, 0xa1, 0x88, 0x5a]);
        let next_eip = 0x1000 + code.len() as u32;
        let mut cpu = vec![(56, next_eip), (144, 0)];
        cpu.extend_from_slice(expected_cpu);
        both(
            step,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &cpu,
                ram: expected_ram,
                exit: Exit::Dispatch(next_eip),
            }],
        );
    }
    for (opcode, cpu, ram) in [
        (0xa1, &[(24, 0x4433_88a1)][..], &[][..]),
        (0xa3, &[][..], &[(0x8020, &[0x11, 0x22][..])][..]),
    ] {
        let code = [0x66, opcode, 0x20, 0x40, 0, 0x80];
        let mut image = image(&code);
        image.map(0x80004, 0x8000, true);
        image.data(0x801f, &[0xa5, 0xa1, 0x88, 0x5a]);
        let mut changes = vec![(56, 0x1006), (144, 0)];
        changes.extend_from_slice(cpu);
        both(
            step,
            "word moffs keeps all 32 address bits",
            &code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram,
                exit: Exit::Dispatch(0x1006),
            }],
        );
    }
}

#[test]
fn page_boundaries() {
    let step = TestModule::interpreter();
    for (name, next_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        for opcode in [0xa1, 0xa3] {
            let code = [0x66, opcode, 0xff, 0x4f, 0, 0];
            let mut image = image(&code);
            image.map(4, 0x8000, true);
            image.map(5, next_frame, true);
            image.data(0x8ffe, &[0xa5, 0xa1]);
            image.data(next_frame, &[0x88, 0x5a]);
            let mut cpu = vec![(56, 0x1006), (144, 0)];
            let ram = if opcode == 0xa1 {
                cpu.push((24, 0x4433_88a1));
                vec![]
            } else {
                vec![(0x8fff, &[0x11][..]), (next_frame, &[0x22][..])]
            };
            both(
                step,
                &format!("word {opcode:02x} across {name} pages"),
                &code,
                1,
                &image,
                &[Step {
                    cpu: &cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1006),
                }],
            );
        }
    }
    for (name, opcode, second_page, fault) in [
        (
            "missing second read page",
            0xa1,
            false,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "readonly second write page",
            0xa3,
            true,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x3,
            },
        ),
    ] {
        let code = [0x66, opcode, 0xff, 0x4f, 0, 0];
        let mut image = image(&code);
        image.map(4, 0x8000, true);
        if second_page {
            image.map(5, 0xa000, false);
        }
        image.data(0x8ffe, &[0xa5, 0xa1]);
        image.data(0xa000, &[0x88, 0x5a]);
        both(
            step,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: fault,
            }],
        );
    }
    for (opcode, fault) in [
        (
            0xa1,
            Exit::PageFault {
                address: 0xffffffff,
                error: 0x0,
            },
        ),
        (
            0xa3,
            Exit::PageFault {
                address: 0xffffffff,
                error: 0x2,
            },
        ),
    ] {
        let code = [0x66, opcode, 0xff, 0xff, 0xff, 0xff];
        let mut image = image(&code);
        image.map(0xfffff, 0x8000, true);
        image.map(0, 0xa000, true);
        image.data(0x8fff, &[0xa1]);
        image.data(0xa000, &[0x88]);
        both(
            step,
            "word data range rejects address-space wrap",
            &code,
            1,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: fault,
            }],
        );
    }
    let code = [0x66, 0xa1, 0xfe, 0xff, 0xff, 0xff];
    let mut image = image(&code);
    image.map(0xfffff, 0x8000, false);
    image.data(0x8ffe, &[0xa1, 0x88]);
    both(
        step,
        "word fits at the final two linear bytes",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(24, 0x4433_88a1), (56, 0x1006), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x1006),
        }],
    );
}

#[test]
fn progress_and_aliases() {
    let step = TestModule::interpreter();
    let code = [0x66, 0xbb, 0, 0x40, 0x66, 0x8b, 0x03];
    for mapped in [false, true] {
        let mut image = image(&code);
        image.register(36, 0x8000_1234);
        image.data(0x8000, &[0xa1, 0x88]);
        if mapped {
            image.map(0x80004, 0x8000, false);
        }
        let last = if mapped {
            Step {
                cpu: &[(24, 0x4433_88a1), (56, 0x1007), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1007),
            }
        } else {
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x80004000,
                    error: 0x0,
                },
            }
        };
        both(
            step,
            "word address definition forwards the preserved upper half",
            &code,
            2,
            &image,
            &[
                Step {
                    cpu: &[(36, 0x8000_4000), (56, 0x1004), (144, 0)],
                    ram: &[],
                    exit: Exit::Dispatch(0x1004),
                },
                last,
            ],
        );
    }
    let code = [0x66, 0x8b, 0x03, 0x66, 0x89, 0x11, 0x66, 0x89, 0xc6];
    let mut image = image(&code);
    image.register(36, 0x4000);
    image.register(28, 0x6000);
    image.map(4, 0x8000, true);
    image.map(6, 0x8000, true);
    image.data(0x7fff, &[0xa5, 0xa1, 0x88, 0x5a]);
    both(
        step,
        "word read survives an aliased physical store",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(24, 0x4433_88a1), (56, 0x1003), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1003),
            },
            Step {
                cpu: &[(56, 0x1006), (144, 1)],
                ram: &[(0x8000, &[0x99, 0xaa])],
                exit: Exit::Dispatch(0x1006),
            },
            Step {
                cpu: &[(48, 0x0123_88a1), (56, 0x1009), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1009),
            },
        ],
    );
}
