use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, BlockError, CompiledModule};
use wasmparser::Validator;

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
#[path = "support/machine.rs"]
mod machine;
use machine::{both, check, Exit, Image, Step};

fn image(code: &[u8]) -> Image {
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

#[test]
fn selected_form_lengths_include_the_address_then_the_immediate() {
    for code in [
        &[0xc6, 0xc4, 0x80][..],
        &[0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xc6, 0x44, 0x8b, 0x7f, 0xff][..],
        &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xa0, 0x20, 0x40, 0, 0x80][..],
        &[0xa1, 0x20, 0x40, 0, 0x80][..],
        &[0xa2, 0x20, 0x40, 0, 0x80][..],
        &[0xa3, 0x20, 0x40, 0, 0x80][..],
    ] {
        for available in 0..code.len() {
            assert!(matches!(
                compile_block_from_bytes(0x1000, &code[..available], 1),
                Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                    if actual == available
            ));
        }
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut with_suffix = code.to_vec();
        with_suffix.extend_from_slice(&[0xc7, 0x0c]);
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes
        );
    }
}

#[test]
fn unsupported_group_extensions_do_not_require_an_address_or_immediate() {
    // These ModRM bytes would require a SIB or disp32 if the group matched.
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        for suffix in [&[][..], &[0x24, 0, 0, 0, 0, 0x80][..]] {
            let mut code = vec![opcode, modrm];
            code.extend_from_slice(suffix);
            assert!(matches!(
                compile_block_from_bytes(0x1ffe, &code, 1),
                Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual })
                    if actual == opcode
            ));
        }
    }
}

fn check_register_immediates(flags: &[&str], step: &ModuleFile) {
    // The parent words are literal byte-alias expectations; ModRM's reg field
    // selects the /0 form, while its r/m field selects the destination.
    for (register, name, offset, immediate, result) in [
        (0, "AL", 24, 0x80, 0x4433_2280),
        (1, "CL", 28, 0, 0x8877_6600),
        (2, "DL", 32, 0xff, 0xccbb_aaff),
        (3, "BL", 36, 0x66, 0x10ff_ee66),
        (4, "AH", 24, 0xc6, 0x4433_c611),
        (5, "CH", 28, 0xa0, 0x8877_a055),
        (6, "DH", 32, 0xc7, 0xccbb_c799),
        (7, "BH", 36, 0x7f, 0x10ff_7fdd),
    ] {
        let code = [0xc6, 0xc0 + register, immediate];
        both(
            step,
            flags,
            &format!("C6 immediate to {name}"),
            &code,
            1,
            &image(&code),
            &[Step {
                cpu: &[(offset, result), (56, 0x1003), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1003),
            }],
        );
    }
    for (register, name, offset) in [
        (0, "EAX", 24),
        (1, "ECX", 28),
        (2, "EDX", 32),
        (3, "EBX", 36),
        (4, "ESP", 40),
        (5, "EBP", 44),
        (6, "ESI", 48),
        (7, "EDI", 52),
    ] {
        let code = [0xc7, 0xc0 + register, 0xa0, 0x66, 0xc7, 0x88];
        both(
            step,
            flags,
            &format!("C7 immediate to {name}"),
            &code,
            1,
            &image(&code),
            &[Step {
                cpu: &[(offset, 0x88c7_66a0), (56, 0x1006), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1006),
            }],
        );
    }
}

fn check_addressed_immediates(flags: &[&str], step: &ModuleFile) {
    for (name, code, registers, address, stored) in [
        (
            "byte through base",
            &[0xc6, 0x03, 0x80][..],
            &[(36, 0x4020)][..],
            0x8020,
            &[0x80][..],
        ),
        (
            "dword after negative disp8",
            &[0xc7, 0x43, 0x80, 0xa0, 0x66, 0xc7, 0x88][..],
            &[(36, 0x4080)][..],
            0x8000,
            &[0xa0, 0x66, 0xc7, 0x88][..],
        ),
        (
            "byte after scaled index and positive disp8",
            &[0xc6, 0x44, 0x8b, 0x7f, 0xff][..],
            &[(36, 0x3f01), (28, 32)][..],
            0x8000,
            &[0xff][..],
        ),
        (
            "dword after wrapped scaled address",
            &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88][..],
            &[(36, 0xffff_fff0), (28, 4)][..],
            0x8000,
            &[0xa0, 0x66, 0xc7, 0x88][..],
        ),
        (
            "byte with neither base nor index",
            &[0xc6, 0x04, 0x25, 0x20, 0x40, 0, 0, 0xb7][..],
            &[][..],
            0x8020,
            &[0xb7][..],
        ),
        (
            "dword with index and no base",
            &[0xc7, 0x04, 0x8d, 0xf0, 0x3f, 0, 0, 0xff, 0xff, 0xff, 0xff][..],
            &[(28, 4)][..],
            0x8000,
            &[0xff, 0xff, 0xff, 0xff][..],
        ),
    ] {
        let mut image = image(code);
        for &(offset, value) in registers {
            image.register(offset, value);
        }
        image.map(4, 0x8000, true);
        let mut before = vec![0xa5];
        before.extend(vec![0xcc; stored.len()]);
        before.push(0x5a);
        image.data(address - 1, &before);
        let next_eip = 0x1000 + code.len() as u32;
        both(
            step,
            flags,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &[(56, next_eip), (144, 0)],
                ram: &[(address, stored)],
                exit: Exit::Dispatch(next_eip),
            }],
        );
    }
}

fn check_absolute_offsets(flags: &[&str], step: &ModuleFile) {
    for (opcode, cpu, ram) in [
        (0xa0, &[(24, 0x4433_22a0)][..], &[][..]),
        (0xa1, &[(24, 0x88c7_66a0)][..], &[][..]),
        (0xa2, &[][..], &[(0x8020, &[0x11][..])][..]),
        (
            0xa3,
            &[][..],
            &[(0x8020, &[0x11, 0x22, 0x33, 0x44][..])][..],
        ),
    ] {
        // Both byte and dword data forms have a full 32-bit absolute offset.
        let code = [opcode, 0x20, 0x40, 0, 0x80];
        let mut image = image(&code);
        image.map(0x80004, 0x8000, true);
        image.data(0x801f, &[0xa5, 0xa0, 0x66, 0xc7, 0x88, 0x5a]);
        let mut changes = vec![(56, 0x1005), (144, 0)];
        changes.extend_from_slice(cpu);
        both(
            step,
            flags,
            &format!("absolute opcode {opcode:02x} with high offset bit"),
            &code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram,
                exit: Exit::Dispatch(0x1005),
            }],
        );
    }
    for (opcode, cpu, ram) in [
        (0xa0, &[(24, 0x4433_2280)][..], &[][..]),
        (0xa2, &[][..], &[(0x8fff, &[0x11][..])][..]),
    ] {
        let code = [opcode, 0xff, 0xff, 0xff, 0xff];
        let mut image = image(&code);
        image.map(0xfffff, 0x8000, true);
        image.data(0x8ffe, &[0xa5, 0x80]);
        let mut changes = vec![(56, 0x1005), (144, 0)];
        changes.extend_from_slice(cpu);
        both(
            step,
            flags,
            "absolute byte at the last linear address",
            &code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram,
                exit: Exit::Dispatch(0x1005),
            }],
        );
    }
    for (name, next_frame) in [("contiguous", 0x9000), ("scattered", 0xa000)] {
        for opcode in [0xa1, 0xa3] {
            let code = [opcode, 0xfe, 0x4f, 0, 0];
            let mut image = image(&code);
            image.map(4, 0x8000, true);
            image.map(5, next_frame, true);
            image.data(0x8ffd, &[0xa5, 0xa0, 0x66]);
            image.data(next_frame, &[0xc7, 0x88, 0x5a]);
            let mut cpu = vec![(56, 0x1005), (144, 0)];
            let ram = if opcode == 0xa1 {
                cpu.push((24, 0x88c7_66a0));
                vec![]
            } else {
                vec![(0x8ffe, &[0x11, 0x22][..]), (next_frame, &[0x33, 0x44][..])]
            };
            both(
                step,
                flags,
                &format!("absolute dword {opcode:02x} across {name} pages"),
                &code,
                1,
                &image,
                &[Step {
                    cpu: &cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(0x1005),
                }],
            );
        }
    }
}

fn check_data_faults(flags: &[&str], step: &ModuleFile) {
    let code = [0xa0, 0, 0x40, 0, 0];
    let mut invalid_backing = image(&code);
    invalid_backing.map(4, 0x10000, false);
    both(
        step,
        flags,
        "present frame outside RAM traps before updating AL",
        &code,
        1,
        &invalid_backing,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::Trap,
        }],
    );
    let code = [0xa1, 0xfe, 0x4f, 0, 0];
    let mut missing = image(&code);
    missing.map(4, 0x8000, false);
    missing.data(0x8ffe, &[0xa0, 0x66]);
    both(
        step,
        flags,
        "absolute read reports the missing second page",
        &code,
        1,
        &missing,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::Fault(0x0004_0000_0000_5000),
        }],
    );
    for code in [
        &[0xa3, 0xfe, 0x4f, 0, 0][..],
        &[0xc7, 0x03, 0xa0, 0x66, 0xc7, 0x88][..],
    ] {
        let mut denied = image(code);
        denied.register(36, 0x4ffe);
        denied.map(4, 0x8000, true);
        denied.map(5, 0xa000, false);
        denied.data(0x8ffd, &[0xa5, 1, 2]);
        denied.data(0xa000, &[3, 4, 0x5a]);
        both(
            step,
            flags,
            "denied second page leaves the entire dword unchanged",
            code,
            1,
            &denied,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(0x0004_0003_0000_5000),
            }],
        );
    }
    for (opcode, fault) in [(0xa1, 0x0004_0000_ffff_fffe), (0xa3, 0x0004_0002_ffff_fffe)] {
        let code = [opcode, 0xfe, 0xff, 0xff, 0xff];
        let mut wrapped = image(&code);
        wrapped.map(0xfffff, 0x8000, true);
        wrapped.map(0, 0xa000, true);
        wrapped.data(0x8ffe, &[1, 2]);
        wrapped.data(0xa000, &[3, 4]);
        both(
            step,
            flags,
            "absolute dword span rejects linear wrap",
            &code,
            1,
            &wrapped,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(fault),
            }],
        );
    }
}

fn check_progress_and_aliases(flags: &[&str], step: &ModuleFile) {
    let code = [
        0xc7, 0xc3, 0, 0x40, 0, 0, 0xc6, 0x03, 0x80, 0xa1, 0, 0x50, 0, 0,
    ];
    for (name, final_step) in [
        (
            "prior register and byte store survive a later fault",
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(0x0004_0000_0000_5000),
            },
        ),
        (
            "forwarded address continues through an absolute load",
            Step {
                cpu: &[(24, 0x88c7_66a0), (56, 0x100e), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x100e),
            },
        ),
    ] {
        let mut image = image(&code);
        image.map(4, 0x8000, true);
        image.data(0x7fff, &[0xa5, 0xcc, 0x5a]);
        image.data(0x9000, &[0xa0, 0x66, 0xc7, 0x88]);
        if matches!(final_step.exit, Exit::Dispatch(_)) {
            image.map(5, 0x9000, false);
        }
        both(
            step,
            flags,
            name,
            &code,
            3,
            &image,
            &[
                Step {
                    cpu: &[(36, 0x4000), (56, 0x1006), (144, 0)],
                    ram: &[],
                    exit: Exit::Dispatch(0x1006),
                },
                Step {
                    cpu: &[(56, 0x1009), (144, 1)],
                    ram: &[(0x8000, &[0x80])],
                    exit: Exit::Dispatch(0x1009),
                },
                final_step,
            ],
        );
    }
    let code = [
        0xc6, 0xc4, 0x80, 0xa3, 0, 0x40, 0, 0, 0xc7, 0xc0, 0xef, 0xbe, 0xad, 0xde, 0xa0, 1, 0x40,
        0, 0,
    ];
    let mut aliases = image(&code);
    aliases.map(4, 0x8000, true);
    aliases.data(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
    both(
        step,
        flags,
        "accumulator byte and dword views share their stored value",
        &code,
        4,
        &aliases,
        &[
            Step {
                cpu: &[(24, 0x4433_8011), (56, 0x1003), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1003),
            },
            Step {
                cpu: &[(56, 0x1008), (144, 1)],
                ram: &[(0x8000, &[0x11, 0x80, 0x33, 0x44])],
                exit: Exit::Dispatch(0x1008),
            },
            Step {
                cpu: &[(24, 0xdead_beef), (56, 0x100e), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x100e),
            },
            Step {
                cpu: &[(24, 0xdead_be80), (56, 0x1013), (144, 3)],
                ram: &[],
                exit: Exit::Dispatch(0x1013),
            },
        ],
    );
    let code = [
        0xa1, 0, 0x40, 0, 0, 0xc7, 0x05, 0, 0x60, 0, 0, 0x99, 0x77, 0x66, 0x55, 0x89, 0xc6,
    ];
    let mut aliases = image(&code);
    aliases.map(4, 0x8000, true);
    aliases.map(6, 0x8000, true);
    aliases.data(0x7fff, &[0xa5, 0x11, 0x22, 0x33, 0x44, 0x5a]);
    both(
        step,
        flags,
        "held absolute load survives a grouped store through a physical alias",
        &code,
        3,
        &aliases,
        &[
            Step {
                cpu: &[(24, 0x4433_2211), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[(56, 0x100f), (144, 1)],
                ram: &[(0x8000, &[0x99, 0x77, 0x66, 0x55])],
                exit: Exit::Dispatch(0x100f),
            },
            Step {
                cpu: &[(48, 0x4433_2211), (56, 0x1011), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1011),
            },
        ],
    );
}

fn check_encoding_fetches(flags: &[&str], step: &ModuleFile) {
    for (name, start, available) in [
        ("missing grouped ModRM", 0x1fff, &[0xc6][..]),
        ("missing grouped displacement", 0x1ffe, &[0xc7, 0x05][..]),
        (
            "missing immediate wins over a missing data page",
            0x1ff9,
            &[0xc7, 0x05, 0, 0x40, 0, 0, 0xa0][..],
        ),
        (
            "missing absolute offset wins over a missing data page",
            0x1ffc,
            &[0xa0, 0, 0x40, 0][..],
        ),
    ] {
        let mut missing = image(&[]);
        missing.register(56, start);
        missing.data(0x3000 + (start & 0xfff), available);
        check(
            step,
            flags,
            name,
            &missing,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(0x0004_0010_0000_2000),
            }],
        );
    }
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        let mut unsupported = image(&[]);
        unsupported.register(56, 0x1ffe);
        unsupported.data(0x3ffe, &[opcode, modrm]);
        check(
            step,
            flags,
            "unsupported group does not fetch the inaccessible address tail",
            &unsupported,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault((8 << 48) | ((opcode as u64) << 32) | 0x1ffe),
            }],
        );
    }
    for (name, start, code, stored) in [
        (
            "group ends at the mapped page boundary",
            0x1ffd,
            &[0xc6, 0x03, 0x80][..],
            &[0x80][..],
        ),
        (
            "absolute offset ends at the mapped page boundary",
            0x1ffb,
            &[0xa3, 0, 0x40, 0, 0][..],
            &[0x11, 0x22, 0x33, 0x44][..],
        ),
    ] {
        let mut complete = image(&[]);
        complete.register(36, 0x4000);
        complete.register(56, start);
        complete.map(4, 0x8000, true);
        complete.data(0x3000 + (start & 0xfff), code);
        complete.data(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
        both(
            step,
            flags,
            name,
            code,
            1,
            &complete,
            &[Step {
                cpu: &[(56, 0x2000), (144, 0)],
                ram: &[(0x8000, stored)],
                exit: Exit::Dispatch(0x2000),
            }],
        );
    }
    let code = [0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88];
    let mut scattered = image(&[]);
    scattered.register(36, 0xffff_fff0);
    scattered.register(28, 4);
    scattered.register(56, 0x1ff9);
    scattered.map(2, 0xa000, false);
    scattered.map(4, 0x8000, true);
    scattered.data(0x3ff9, &code[..7]);
    scattered.data(0xa000, &code[7..]);
    scattered.data(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a]);
    both(
        step,
        flags,
        "eleven-byte instruction fetch crosses scattered frames",
        &code,
        1,
        &scattered,
        &[Step {
            cpu: &[(56, 0x2004), (144, 0)],
            ram: &[(0x8000, &[0xa0, 0x66, 0xc7, 0x88])],
            exit: Exit::Dispatch(0x2004),
        }],
    );
    let code = [0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88];
    let mut wrapped = image(&[]);
    wrapped.register(56, 0xffff_fffc);
    wrapped.map(0xfffff, 0x8000, false);
    wrapped.map(0, 0xa000, false);
    wrapped.data(0x8ffc, &code[..4]);
    wrapped.data(0xa000, &code[4..]);
    both(
        step,
        flags,
        "group immediate fetch wraps EIP",
        &code,
        1,
        &wrapped,
        &[Step {
            cpu: &[(24, 0x88c7_66a0), (56, 2), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(2),
        }],
    );
}

fn execute_forms(flags: &[&str]) {
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let step = ModuleFile::new(&module);
    check_register_immediates(flags, &step);
    check_addressed_immediates(flags, &step);
    check_absolute_offsets(flags, &step);
    check_data_faults(flags, &step);
    check_progress_and_aliases(flags, &step);
    check_encoding_fetches(flags, &step);
}

#[test]
#[ignore = "requires Node.js with WebAssembly tail-call and multiple-memory support"]
fn immediate_and_absolute_moves_execute_in_v8() {
    execute_forms(&[]);
}

#[test]
#[ignore = "requires Node.js with WebAssembly tail-call and multiple-memory support"]
fn immediate_and_absolute_moves_execute_in_optimizing_v8() {
    execute_forms(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
