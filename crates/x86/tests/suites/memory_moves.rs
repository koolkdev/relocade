use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::Validator;

use crate::support::step;
use step::TestModule;

use crate::support::machine;
use machine::{both, check, Exit, Image, Step};

struct Address {
    name: &'static str,
    code: &'static [u8],
    registers: &'static [(usize, u32)],
    linear: u32,
    stored: u32,
}

const ADDRESSES: &[Address] = &[
    Address {
        name: "EAX base",
        code: &[0x8b, 0x10],
        registers: &[(24, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "ECX base",
        code: &[0x8b, 0x11],
        registers: &[(28, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "EDX address read before EDX replacement",
        code: &[0x8b, 0x12],
        registers: &[(32, 0x4000)],
        linear: 0x4000,
        stored: 0x4000,
    },
    Address {
        name: "EBX base",
        code: &[0x8b, 0x13],
        registers: &[(36, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "ESP through SIB with no index",
        code: &[0x8b, 0x14, 0x24],
        registers: &[(40, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "EBP with displacement zero",
        code: &[0x8b, 0x55, 0],
        registers: &[(44, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "ESI base",
        code: &[0x8b, 0x16],
        registers: &[(48, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "EDI base",
        code: &[0x8b, 0x17],
        registers: &[(52, 0x4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale one",
        code: &[0x8b, 0x54, 0x0b, 0x10],
        registers: &[(36, 0x4000), (28, 3)],
        linear: 0x4013,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale two",
        code: &[0x8b, 0x54, 0x4b, 0x10],
        registers: &[(36, 0x4000), (28, 3)],
        linear: 0x4016,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale four",
        code: &[0x8b, 0x54, 0x8b, 0x10],
        registers: &[(36, 0x4000), (28, 3)],
        linear: 0x401c,
        stored: 0xdead_beef,
    },
    Address {
        name: "scale eight",
        code: &[0x8b, 0x54, 0xcb, 0x10],
        registers: &[(36, 0x4000), (28, 3)],
        linear: 0x4028,
        stored: 0xdead_beef,
    },
    Address {
        name: "SIB absent index ignores scale",
        code: &[0x8b, 0x14, 0xe3],
        registers: &[(36, 0x4000)],
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
        registers: &[(44, 3)],
        linear: 0x400c,
        stored: 0xdead_beef,
    },
    Address {
        name: "negative disp8 boundary",
        code: &[0x8b, 0x50, 0x80],
        registers: &[(24, 0x4080)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "positive disp8 boundary",
        code: &[0x8b, 0x50, 0x7f],
        registers: &[(24, 0x3f81)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "high-bit disp32 wrapping sum",
        code: &[0x8b, 0x90, 0, 0, 0, 0x80],
        registers: &[(24, 0x8000_4000)],
        linear: 0x4000,
        stored: 0xdead_beef,
    },
    Address {
        name: "scaled effective address wraps",
        code: &[0x8b, 0x14, 0x88],
        registers: &[(24, 0xffff_fffc), (28, 2)],
        linear: 4,
        stored: 0xdead_beef,
    },
];

#[test]
fn addresses() {
    let step = TestModule::interpreter();
    for address in ADDRESSES {
        for opcode in [0x8b, 0x89] {
            let mut code = address.code.to_vec();
            code[0] = opcode;
            let mut image = Image::new(&code);
            for &(offset, value) in address.registers {
                image.register(offset, value);
            }
            image.map(address.linear >> 12, 0x8000, true);
            let physical = 0x8000 | (address.linear & 0xfff);
            image.data(physical - 1, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a]);
            let next = 0x1000 + code.len() as u32;
            let mut updates = vec![(56, next), (144, 0)];
            let value = address.stored.to_le_bytes();
            let writes = if opcode == 0x8b {
                updates.push((32, 0x9234_5678));
                vec![]
            } else {
                vec![(physical, &value[..])]
            };
            both(
                step,
                address.name,
                &code,
                1,
                &image,
                &[Step {
                    cpu: &updates,
                    ram: &writes,
                    exit: Exit::Dispatch(next),
                }],
            );
        }
    }
}

#[test]
fn selected_memory_forms_require_only_their_address_bytes() {
    for bytes in [
        &[0x8b, 0x04][..],
        &[0x8b, 0x44, 0x24],
        &[0x89, 0x05, 0x12, 0x34, 0x56],
    ] {
        assert_eq!(
            compile_block_from_bytes(0x1000, bytes, 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: 0x1000,
                available: bytes.len()
            })
        );
    }
    let code = [0x8b, 0x14, 0x25, 0xf3, 0x0f, 0xb8, 0x66, 0x90];
    let module = compile_block_from_bytes(0x1000, &code, 1).unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    assert_eq!(
        compile_block_from_bytes(0x1000, &code, 2).err(),
        Some(BlockError::UnsupportedInstruction {
            address: 0x1007,
            opcode: 0x90
        })
    );
}

#[test]
fn accesses() {
    let step = TestModule::interpreter();
    for (name, second_frame) in [("contiguous dword", 0x9000), ("scattered dword", 0xa000)] {
        for opcode in [0x8b, 0x89] {
            let code = [opcode, 0x13];
            let mut image = Image::new(&code);
            image.register(36, 0x4ffe);
            image.map(4, 0x8000, true);
            image.map(5, second_frame, true);
            image.data(0x8ffd, &[0xa5, 0x78, 0x56]);
            image.data(second_frame, &[0x34, 0x92, 0x5a]);
            let mut cpu = vec![(56, 0x1002), (144, 0)];
            let ram = if opcode == 0x8b {
                cpu.push((32, 0x9234_5678));
                vec![]
            } else {
                vec![
                    (0x8ffe, &[0xef, 0xbe][..]),
                    (second_frame, &[0xad, 0xde][..]),
                ]
            };
            both(
                step,
                name,
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
    enum Page {
        Absent,
        ReadOnly,
        Writable,
    }
    for (name, opcode, address, first, second, exit) in [
        (
            "absent data write",
            0x89,
            0x4020,
            Page::Absent,
            Page::Absent,
            Exit::PageFault {
                address: 0x00004020,
                error: 0x2,
            },
        ),
        (
            "readonly data write",
            0x89,
            0x4020,
            Page::ReadOnly,
            Page::Absent,
            Exit::PageFault {
                address: 0x00004020,
                error: 0x3,
            },
        ),
        (
            "later absent read page",
            0x8b,
            0x4ffe,
            Page::ReadOnly,
            Page::Absent,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x0,
            },
        ),
        (
            "later absent write page",
            0x89,
            0x4ffe,
            Page::Writable,
            Page::Absent,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x2,
            },
        ),
        (
            "later readonly page leaves both halves untouched",
            0x89,
            0x4ffe,
            Page::Writable,
            Page::ReadOnly,
            Exit::PageFault {
                address: 0x00005000,
                error: 0x3,
            },
        ),
        (
            "first write denial precedes later absence",
            0x89,
            0x4ffe,
            Page::ReadOnly,
            Page::Absent,
            Exit::PageFault {
                address: 0x00004ffe,
                error: 0x3,
            },
        ),
    ] {
        let code = [opcode, 0x13];
        let mut image = Image::new(&code);
        image.register(36, address);
        for (page, frame, permissions) in [(4, 0x8000, first), (5, 0xa000, second)] {
            match permissions {
                Page::Absent => {}
                Page::ReadOnly => image.map(page, frame, false),
                Page::Writable => image.map(page, frame, true),
            }
        }
        image.data(0x8ffd, &[0xa5, 0x78, 0x56]);
        image.data(0xa000, &[0x34, 0x92, 0x5a]);
        both(
            step,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit,
            }],
        );
    }
    // Address arithmetic wraps, but the selected memory policy rejects a dword range that wraps.
    for (opcode, exit) in [
        (
            0x8b,
            Exit::PageFault {
                address: 0xfffffffe,
                error: 0x0,
            },
        ),
        (
            0x89,
            Exit::PageFault {
                address: 0xfffffffe,
                error: 0x2,
            },
        ),
    ] {
        let code = [opcode, 0x13];
        let mut image = Image::new(&code);
        image.register(36, 0xffff_fffe);
        image.map(0xfffff, 0x8000, true);
        image.map(0, 0xa000, true);
        image.data(0x8ffe, &[0x78, 0x56]);
        image.data(0xa000, &[0x34, 0x92]);
        both(
            step,
            "dword range wrap denial",
            &code,
            1,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit,
            }],
        );
    }
    for opcode in [0x8b, 0x89] {
        let code = [opcode, 0x13];
        let mut image = Image::new(&code);
        image.register(36, 0x4000);
        image.map(4, 0x10000, true);
        both(
            step,
            "present frame without RAM backing traps",
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
    for (name, start, code) in [
        ("missing SIB before data access", 0x1ffe, &[0x8b, 0x04][..]),
        (
            "missing disp8 before data access",
            0x1ffe,
            &[0x8b, 0x40][..],
        ),
        (
            "missing disp32 before data access",
            0x1ffc,
            &[0x8b, 0x80, 0x12, 0x34][..],
        ),
    ] {
        let mut image = Image::new(&[]);
        image.register(56, start);
        image.register(24, 0x4000);
        image.data(0x3000 + (start & 0xfff), code);
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
    let code = [0x8b, 0x14, 0x24];
    let mut image = Image::new(&[]);
    image.register(56, 0x1ffd);
    image.register(40, 0x4000);
    image.data(0x3ffd, &code);
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0x78, 0x56, 0x34, 0x92]);
    both(
        step,
        "complete SIB at page end",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(32, 0x9234_5678), (56, 0x2000), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x2000),
        }],
    );
    let code = [0x8b, 0x90, 0, 0x40, 0, 0];
    let mut image = Image::new(&[]);
    image.register(56, 0x1ffc);
    image.register(24, 0);
    image.data(0x3ffc, &code[..4]);
    image.data(0x6000, &code[4..]);
    image.map(2, 0x6000, false);
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0x78, 0x56, 0x34, 0x92]);
    both(
        step,
        "displacement crosses scattered instruction pages",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(32, 0x9234_5678), (56, 0x2002), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x2002),
        }],
    );
}

#[test]
fn progress() {
    let step = TestModule::interpreter();
    for (opcode, modrm) in [(0x8b, 0x13), (0x89, 0x03)] {
        let code = [0xb8, 42, 0, 0, 0, opcode, modrm, 0xb9, 7, 0, 0, 0];
        let mut image = Image::new(&code);
        image.register(36, 0x4000);
        image.map(4, 0x8000, true);
        image.data(0x8000, &[0x78, 0x56, 0x34, 0x92]);
        let second_cpu = if opcode == 0x8b {
            vec![(32, 0x9234_5678), (56, 0x1007), (144, 1)]
        } else {
            vec![(56, 0x1007), (144, 1)]
        };
        let writes = if opcode == 0x89 {
            vec![(0x8000, &[42, 0, 0, 0][..])]
        } else {
            vec![]
        };
        both(
            step,
            "successful memory operation continues",
            &code,
            3,
            &image,
            &[
                Step {
                    cpu: &[(24, 42), (56, 0x1005), (144, 0)],
                    ram: &[],
                    exit: Exit::Dispatch(0x1005),
                },
                Step {
                    cpu: &second_cpu,
                    ram: &writes,
                    exit: Exit::Dispatch(0x1007),
                },
                Step {
                    cpu: &[(28, 7), (56, 0x100c), (144, 2)],
                    ram: &[],
                    exit: Exit::Dispatch(0x100c),
                },
            ],
        );
        image.machine.truncate(1);
        let fault = if opcode == 0x89 {
            image.map(4, 0x8000, false);
            Exit::PageFault {
                address: 0x00004000,
                error: 0x3,
            }
        } else {
            Exit::PageFault {
                address: 0x00004000,
                error: 0x0,
            }
        };
        both(
            step,
            "data fault publishes only completed prefix",
            &code,
            3,
            &image,
            &[
                Step {
                    cpu: &[(24, 42), (56, 0x1005), (144, 0)],
                    ram: &[],
                    exit: Exit::Dispatch(0x1005),
                },
                Step {
                    cpu: &[],
                    ram: &[],
                    exit: fault,
                },
            ],
        );
    }
    let code = [0xb8, 42, 0, 0, 0, 0x89, 0x03, 0x8b, 0x11];
    let mut image = Image::new(&code);
    image.register(36, 0x4000);
    image.register(28, 0x5000);
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0xa5; 4]);
    both(
        step,
        "completed store survives later read fault",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(24, 42), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[(56, 0x1007), (144, 1)],
                ram: &[(0x8000, &[42, 0, 0, 0])],
                exit: Exit::Dispatch(0x1007),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00005000,
                    error: 0x0,
                },
            },
        ],
    );
}

#[test]
fn forwarded_addresses() {
    let step = TestModule::interpreter();
    let code = [0xbb, 0, 0x40, 0, 0, 0x8b, 0x03, 0xbe, 7, 0, 0, 0];
    let mut image = Image::new(&code);
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0x78, 0x56, 0x34, 0x92]);
    both(
        step,
        "memory address uses the preceding register definition",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(36, 0x4000), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[(24, 0x9234_5678), (56, 0x1007), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1007),
            },
            Step {
                cpu: &[(48, 7), (56, 0x100c), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x100c),
            },
        ],
    );
    image.machine.truncate(1);
    both(
        step,
        "data fault publishes the preceding address definition",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(36, 0x4000), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00004000,
                    error: 0x0,
                },
            },
        ],
    );
}

#[test]
fn aliased_guest_snapshot() {
    let step = TestModule::interpreter();
    let code = [0x8b, 0x03, 0x89, 0x11, 0x8b, 0xf0];
    let mut image = Image::new(&code);
    image.register(36, 0x4000);
    image.register(28, 0x6000);
    image.map(4, 0x8000, true);
    image.map(6, 0x8000, true);
    image.data(0x7fff, &[0xa5, 0x78, 0x56, 0x34, 0x92, 0x5a]);
    both(
        step,
        "distinct guest pages alias the loaded snapshot",
        &code,
        3,
        &image,
        &[
            Step {
                cpu: &[(24, 0x9234_5678), (56, 0x1002), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            },
            Step {
                cpu: &[(56, 0x1004), (144, 1)],
                ram: &[(0x8000, &[0xef, 0xbe, 0xad, 0xde])],
                exit: Exit::Dispatch(0x1004),
            },
            Step {
                cpu: &[(48, 0x9234_5678), (56, 0x1006), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1006),
            },
        ],
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn memory_accesses_and_faults_execute_in_optimizing_v8() {
    // A scattered store completes before a later read faults or traps.
    let code = [0xb8, 42, 0, 0, 0, 0x89, 0x03, 0x8b, 0x11];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
    for bad_frame in [false, true] {
        let mut image = Image::new(&code);
        image.register(36, 0x4ffe);
        image.register(28, 0x6000);
        image.map(4, 0x8000, true);
        image.map(5, 0xa000, true);
        image.data(0x8ffd, &[0xa5, 0x78, 0x56]);
        image.data(0xa000, &[0x34, 0x92, 0x5a]);
        if bad_frame {
            image.map(6, 0x10000, false);
        }
        let exit = if bad_frame {
            Exit::Trap
        } else {
            Exit::PageFault {
                address: 0x00006000,
                error: 0x0,
            }
        };
        let writes = [(0x8ffe, &[42, 0][..]), (0xa000, &[0, 0][..])];
        let steps = [
            Step {
                cpu: &[(24, 42), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[(56, 0x1007), (144, 1)],
                ram: &writes,
                exit: Exit::Dispatch(0x1007),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit,
            },
        ];
        assert_eq!(
            TestModule::interpreter().observe_v8(&image.input(), 3),
            machine::expected(&image, &steps),
        );
        // A Wasm trap interrupts snapshot publication; successful stores remain visible.
        let cpu = if bad_frame {
            &[][..]
        } else {
            &[(24, 42), (56, 0x1007), (144, 1)][..]
        };
        assert_eq!(
            block.observe_v8(&image.input(), 1),
            machine::expected(
                &image,
                &[Step {
                    cpu,
                    ram: &writes,
                    exit
                }]
            ),
        );
    }
}

#[test]
fn read_does_not_require_writable_memory() {
    use crate::support::guest::{Exit, Machine, Permissions::*, Register::*};

    let mut machine = Machine::new(&[0x8b, 0x13]); // MOV EDX, [EBX].
    machine.cpu.set_register(Ebx, 0x4020);
    machine.memory(0x4020, &0x9234_5678_u32.to_le_bytes(), ReadOnly);
    let mut expected = machine.state();
    expected.cpu.set_register(Edx, 0x9234_5678);
    expected.cpu.set_eip(0x1002);
    expected.cpu.set_instruction_count(0);

    for result in [machine.run_step(), machine.run_block(1)] {
        assert_eq!(result.exit, Exit::Dispatch(0x1002));
        assert_eq!(result.dispatches, [(0x1002, expected.clone())]);
        assert_eq!(result.state, expected);
        assert!(result.machine_unchanged);
    }
}

#[test]
fn an_unmapped_read_preserves_cpu_and_memory() {
    use crate::support::guest::{Exit, Machine, Permissions::*, Register::*};

    let mut machine = Machine::new(&[0x8b, 0x13]); // MOV EDX, [EBX].
    machine.cpu.set_register(Ebx, 0x4020);
    // Retain data canaries even though this read has no mapping.
    machine.memory(0x8ffd, &[0xa5, 0x78, 0x56], ReadWrite);
    machine.memory(0xa000, &[0x34, 0x92, 0x5a], ReadWrite);

    for result in [machine.run_step(), machine.run_block(1)] {
        assert_eq!(
            result.exit,
            Exit::PageFault {
                address: 0x4020,
                error: 0
            }
        );
        assert!(result.dispatches.is_empty());
        assert_eq!(result.state, machine.state());
        assert!(result.machine_unchanged);
    }
}

#[test]
fn a_store_changes_only_the_addressed_bytes() {
    use crate::support::guest::{Exit, Machine, Permissions::ReadWrite, Register::*};

    let mut machine = Machine::new(&[0x89, 0x13]); // MOV [EBX], EDX.
    machine.cpu.set_register(Ebx, 0x4020);
    machine.cpu.set_register(Edx, 0x1234_5678);
    machine.memory(0x401f, &[0xa5, 0, 0, 0, 0, 0x5a], ReadWrite);
    let mut expected = machine.state();
    expected.cpu.set_eip(0x1002);
    expected.cpu.set_instruction_count(0);
    expected
        .memory
        .write(0x4020, &0x1234_5678_u32.to_le_bytes());

    for result in [machine.run_step(), machine.run_block(1)] {
        assert_eq!(result.exit, Exit::Dispatch(0x1002));
        assert_eq!(
            result.state.memory.read(0x401f, 6),
            [0xa5, 0x78, 0x56, 0x34, 0x12, 0x5a]
        );
        assert_eq!(result.dispatches, [(0x1002, expected.clone())]);
        assert_eq!(result.state, expected);
        assert!(result.machine_unchanged);
    }
}
