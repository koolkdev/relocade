use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, BlockError, CompiledModule};
use wasmparser::Validator;

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
#[allow(dead_code)]
#[path = "support/machine.rs"]
mod machine;
use machine::{both, check, Exit, Image, Step};

fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.register(24, 0x4433_2211);
    image.register(36, 0x10ff_eedd);
    image
}

#[test]
fn operand_size_changes_values_but_keeps_address_fields_at_four_bytes() {
    for code in [
        &[0x66, 0xb8, 0x34, 0x12][..],
        &[0x66, 0x89, 0xc8][..],
        &[0x66, 0x8b, 0x85, 0x20, 0x40, 0, 0][..],
        &[0x66, 0xc7, 0xc0, 0xa1, 0x88][..],
        &[0x66, 0xc7, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0xa1, 0x88][..],
        &[0x66, 0xa1, 0x20, 0x40, 0, 0x80][..],
        &[0x66, 0xa3, 0x20, 0x40, 0, 0x80][..],
        &[0x66, 0xb4, 0x80][..],
        &[0x66, 0x88, 0xd8][..],
        &[0x66, 0x8a, 0xe3][..],
        &[0x66, 0xc6, 0xc4, 0x80][..],
        &[0x66, 0xa0, 0x20, 0x40, 0, 0][..],
        &[0x66, 0xa2, 0x20, 0x40, 0, 0][..],
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
        with_suffix.extend_from_slice(&[0x66; 15]);
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes
        );
    }
}

#[test]
fn instruction_length_counts_prefixes_and_each_required_field_byte() {
    let mut maximum = vec![0x66; 12];
    maximum.extend_from_slice(&[0xb8, 0x34, 0x12]);
    Validator::new()
        .validate_all(&compile_block_from_bytes(0x1000, &maximum, 1).unwrap().bytes)
        .unwrap();
    for (prefixes, suffix) in [
        (15, &[][..]),
        (14, &[0x8b][..]),
        (13, &[0x8b, 0x04][..]),
        (13, &[0xb8, 0x34, 0x12][..]),
        (11, &[0xa1, 0, 0x40, 0, 0][..]),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        assert!(matches!(
            compile_block_from_bytes(0x1000, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1000 })
        ));
    }
    for prefixes in [1, 14] {
        assert!(
            matches!(compile_block_from_bytes(0x1000, &vec![0x66; prefixes], 1), Err(BlockError::TruncatedInstruction { available, .. }) if available == prefixes)
        );
    }
    for (prefixes, suffix, opcode) in [
        (14, &[0x62][..], 0x62),
        (13, &[0xc7, 0x0d][..], 0xc7),
        (1, &[0x67, 0x8b, 0][..], 0x67),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        assert!(
            matches!(compile_block_from_bytes(0x1000, &code, 1), Err(BlockError::UnsupportedInstruction { address: 0x1000, opcode: actual }) if actual == opcode)
        );
    }
}

fn check_operand_size(flags: &[&str], step: &ModuleFile) {
    let code = [0x66, 0x66, 0xb8, 0x34, 0x12, 0xb9, 0x55, 0x66, 0x77, 0x88];
    both(
        step,
        flags,
        "repeated override is idempotent and ends with its instruction",
        &code,
        2,
        &image(&code),
        &[
            Step {
                cpu: &[(24, 0x4433_1234), (56, 0x1005), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu: &[(28, 0x8877_6655), (56, 0x100a), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x100a),
            },
        ],
    );
    for (name, code, expected, stored) in [
        (
            "byte opcode-register immediate",
            &[0x66, 0xb4, 0x80][..],
            0x4433_8011,
            &[][..],
        ),
        (
            "byte register to r/m",
            &[0x66, 0x88, 0xd8][..],
            0x4433_22dd,
            &[][..],
        ),
        (
            "byte r/m to register",
            &[0x66, 0x8a, 0xe3][..],
            0x4433_dd11,
            &[][..],
        ),
        (
            "byte r/m immediate",
            &[0x66, 0xc6, 0xc4, 0x80][..],
            0x4433_8011,
            &[][..],
        ),
        (
            "byte absolute load",
            &[0x66, 0xa0, 0x20, 0x40, 0, 0][..],
            0x4433_2280,
            &[][..],
        ),
        (
            "byte absolute store",
            &[0x66, 0xa2, 0x20, 0x40, 0, 0][..],
            0x4433_2211,
            &[(0x8020, &[0x11][..])][..],
        ),
    ] {
        let mut image = image(code);
        image.map(4, 0x8000, true);
        image.data(0x801f, &[0xa5, 0x80, 0x5a]);
        let next_eip = 0x1000 + code.len() as u32;
        both(
            step,
            flags,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &[(24, expected), (56, next_eip), (144, 0)],
                ram: stored,
                exit: Exit::Dispatch(next_eip),
            }],
        );
    }
}

fn check_fetch_boundaries(flags: &[&str], step: &ModuleFile) {
    let code = [0x66, 0xb8, 0x34, 0x12];
    for (name, start, first_frame, first_page, next_eip) in [
        ("prefix at page end", 0x1fff, 0x3000, 1, 0x2003),
        (
            "instruction fetch wraps EIP",
            0xffff_fffd,
            0x8000,
            0xfffff,
            1,
        ),
    ] {
        let mut image = image(&code);
        image.register(56, start);
        image.guest.clear();
        image.map(first_page, first_frame, false);
        image.map(if first_page == 1 { 2 } else { 0 }, 0xa000, false);
        let available = 0x1000 - (start & 0xfff);
        image.data(first_frame + (start & 0xfff), &code[..available as usize]);
        image.data(0xa000, &code[available as usize..]);
        both(
            step,
            flags,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: &[(24, 0x4433_1234), (56, next_eip), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(next_eip),
            }],
        );
    }
    let mut at_end = image(&code);
    at_end.register(56, 0x1ffc);
    at_end.guest.clear();
    at_end.data(0x3ffc, &code);
    both(
        step,
        flags,
        "complete word instruction needs no following page",
        &code,
        1,
        &at_end,
        &[Step {
            cpu: &[(24, 0x4433_1234), (56, 0x2000), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x2000),
        }],
    );

    for (name, start, available_bytes) in [
        ("missing opcode after prefix", 0x1fff, &[0x66][..]),
        (
            "missing ModRM after prefix and opcode",
            0x1ffe,
            &[0x66, 0x8b][..],
        ),
        ("missing required SIB", 0x1ffd, &[0x66, 0x8b, 0x04][..]),
        (
            "missing final displacement byte",
            0x1ffa,
            &[0x66, 0x8b, 0x05, 0, 0x40, 0][..],
        ),
        (
            "missing word immediate before any data access",
            0x1ffc,
            &[0x66, 0xc7, 0x03, 0xa1][..],
        ),
    ] {
        let mut image = image(available_bytes);
        image.register(56, start);
        image.register(36, 0x4000);
        image.guest.clear();
        image.data(0x3000 + (start & 0xfff), available_bytes);
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
    let code = [
        0x66, 0x66, 0x66, 0xc7, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0xa1, 0x88,
    ];
    let mut image = image(&code);
    image.register(56, 0x1ff8);
    image.register(36, 0xffff_fff0);
    image.register(28, 4);
    image.guest.clear();
    image.data(0x3ff8, &code[..8]);
    image.map(2, 0xa000, false);
    image.data(0xa000, &code[8..]);
    image.map(4, 0x8000, true);
    image.data(0x801f, &[0xa5, 0, 0, 0x5a]);
    both(
        step,
        flags,
        "prefixed SIB displacement crosses scattered code pages",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[(56, 0x2004), (144, 0)],
            ram: &[(0x8020, &[0xa1, 0x88])],
            exit: Exit::Dispatch(0x2004),
        }],
    );
}

fn check_length_precedence(flags: &[&str], step: &ModuleFile) {
    let mut maximum = vec![0x66; 12];
    maximum.extend_from_slice(&[0xb8, 0x34, 0x12]);
    let mut maximum_image = image(&maximum);
    maximum_image.register(56, 0x1ff1);
    maximum_image.guest.clear();
    maximum_image.data(0x3ff1, &maximum);
    both(
        step,
        flags,
        "fifteen-byte word MOV retires without fetching a sixteenth byte",
        &maximum,
        1,
        &maximum_image,
        &[Step {
            cpu: &[(24, 0x4433_1234), (56, 0x2000), (144, 0)],
            ram: &[],
            exit: Exit::Dispatch(0x2000),
        }],
    );

    // These fixtures leave the next page absent. A required byte below offset 15
    // can page-fault; a request at offset 15 reports GP before consulting memory.
    for (name, prefixes, suffix, start, expected) in [
        (
            "required ModRM is beyond the instruction limit",
            14,
            &[0x8b][..],
            0x1ff1,
            0x0002_0000_0000_0000,
        ),
        (
            "required SIB is beyond the instruction limit",
            13,
            &[0x8b, 0x04][..],
            0x1ff1,
            0x0002_0000_0000_0000,
        ),
        (
            "fifteen prefixes stop before byte sixteen",
            15,
            &[][..],
            0x1ff1,
            0x0002_0000_0000_0000,
        ),
        (
            "word immediate reaches length limit before missing page",
            13,
            &[0xb8, 0x34][..],
            0x1ff1,
            0x0002_0000_0000_0000,
        ),
        (
            "missing immediate byte below length limit wins",
            13,
            &[0xb8][..],
            0x1ff2,
            0x0004_0010_0000_2000,
        ),
        (
            "dword address reaches length limit before missing page",
            11,
            &[0xa1, 0, 0x40, 0][..],
            0x1ff1,
            0x0002_0000_0000_0000,
        ),
        (
            "missing address byte below length limit wins",
            11,
            &[0xa1, 0, 0x40][..],
            0x1ff2,
            0x0004_0010_0000_2000,
        ),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        let mut image = image(&code);
        image.register(56, start);
        image.guest.clear();
        image.data(0x3000 + (start & 0xfff), &code);
        check(
            step,
            flags,
            name,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(expected),
            }],
        );
    }
    for (name, prefixes, suffix, start, expected) in [
        (
            "unsupported opcode at last admitted byte",
            14,
            &[0x62][..],
            0x1ff1,
            0x0008_0062_0000_1ff1,
        ),
        (
            "unsupported group at last admitted ModRM",
            13,
            &[0xc7, 0x0d][..],
            0x1ff1,
            0x0008_00c7_0000_1ff1,
        ),
        (
            "unhandled prefix ends scanning",
            1,
            &[0x67][..],
            0x1ffe,
            0x0008_0067_0000_1ffe,
        ),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        let mut image = image(&code);
        image.register(56, start);
        image.guest.clear();
        image.data(0x3000 + (start & 0xfff), &code);
        check(
            step,
            flags,
            name,
            &image,
            &[Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(expected),
            }],
        );
    }
}

fn check_completed_progress(flags: &[&str], step: &ModuleFile) {
    let mut code = vec![0x66, 0xb8, 0x34, 0x12];
    code.extend_from_slice(&[0x66; 15]);
    check(
        step,
        flags,
        "overlong next instruction preserves completed word progress",
        &image(&code),
        &[
            Step {
                cpu: &[(24, 0x4433_1234), (56, 0x1004), (144, 0)],
                ram: &[],
                exit: Exit::Dispatch(0x1004),
            },
            Step {
                cpu: &[],
                ram: &[],
                exit: Exit::Fault(0x0002_0000_0000_0000),
            },
        ],
    );
}

fn check_prefixes(flags: &[&str]) {
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let step = ModuleFile::new(&module);
    check_operand_size(flags, &step);
    check_fetch_boundaries(flags, &step);
    check_length_precedence(flags, &step);
    check_completed_progress(flags, &step);
}

#[test]
#[ignore = "requires Node.js with WebAssembly tail calls"]
fn instruction_prefixes_execute_in_v8() {
    check_prefixes(&[]);
}

#[test]
#[ignore = "requires Node.js with optimizing WebAssembly compilation"]
fn instruction_prefixes_execute_in_optimizing_v8() {
    check_prefixes(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
