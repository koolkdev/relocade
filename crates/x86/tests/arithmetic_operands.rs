use wasm86_x86::{compile_block_from_bytes, compile_interpreter_step, CompiledModule};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

#[path = "support/step.rs"]
mod step;
use step::ModuleFile;
#[allow(dead_code)]
#[path = "support/machine.rs"]
mod machine;
use machine::{both, Exit, Step};
#[path = "support/arithmetic.rs"]
mod arithmetic;
use arithmetic::{image, recipe};

#[test]
fn compare_only_reads_guest_memory_and_add_also_stores() {
    for (code, writes) in [
        (&[0x38, 0x03][..], false),
        (&[0x66, 0x39, 0x03][..], false),
        (&[0x39, 0x03][..], false),
        (&[0x00, 0x03][..], true),
        (&[0x66, 0x01, 0x03][..], true),
        (&[0x01, 0x03][..], true),
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
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::I32Load { memarg }
                            | Operator::I32Load8U { memarg }
                            | Operator::I32Load16U { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                loads += 1
                            }
                            Operator::I32Store { memarg }
                            | Operator::I32Store8 { memarg }
                            | Operator::I32Store16 { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                stores += 1
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(guest.is_some() && loads > 0, "{code:02x?}");
        assert_eq!(stores > 0, writes, "{code:02x?}");
    }
}

fn check_register_aliases(flags: &[&str], step: &ModuleFile) {
    for (name, code, eax, result, left, right, kind) in [
        (
            "AL reads old AH",
            &[0x00, 0xe0][..],
            0x4433_7f81,
            0x4433_7f00,
            0x81,
            0x7f,
            2,
        ),
        (
            "AH reads old AL",
            &[0x00, 0xc4][..],
            0x4433_8080,
            0x4433_0080,
            0x80,
            0x80,
            2,
        ),
        (
            "dword self addition uses the old value",
            &[0x01, 0xc0][..],
            0x8000_0000,
            0,
            0x8000_0000,
            0x8000_0000,
            10,
        ),
        (
            "AH compare leaves both aliases intact",
            &[0x38, 0xc4][..],
            0x4433_7efe,
            0x4433_7efe,
            0x7e,
            0xfe,
            1,
        ),
        (
            "accumulator byte ADD ignores operand prefix",
            &[0x66, 0x04, 1][..],
            0x4433_22ff,
            0x4433_2200,
            0xff,
            1,
            2,
        ),
        (
            "accumulator word CMP reads two immediate bytes",
            &[0x66, 0x3d, 0x11, 0x22][..],
            0x4433_2211,
            0x4433_2211,
            0x2211,
            0x2211,
            5,
        ),
    ] {
        let mut image = image(code);
        image.register(24, eax);
        let next = 0x1000 + code.len() as u32;
        let mut changes = recipe(kind, left, right).to_vec();
        changes.extend_from_slice(&[(24, result), (56, next), (144, 0)]);
        both(
            step,
            flags,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram: &[],
                exit: Exit::Dispatch(next),
            }],
        );
    }
    let code = [
        0x04, 1, 0x66, 0x0f, 0x94, 0xfc, 0xb0, 0x7f, 0x0f, 0x92, 0xc0,
    ];
    let mut image = image(&code);
    image.register(24, 0x4433_22ff);
    both(
        step,
        flags,
        "SETcc and MOV preserve prior ADD flags while changing aliases",
        &code,
        4,
        &image,
        &[
            Step {
                cpu: &[
                    (0, 0xa5a5_a502),
                    (4, 0xff),
                    (8, 1),
                    (24, 0x4433_2200),
                    (56, 0x1002),
                    (144, 0),
                ],
                ram: &[],
                exit: Exit::Dispatch(0x1002),
            },
            Step {
                cpu: &[(24, 0x4433_0100), (56, 0x1006), (144, 1)],
                ram: &[],
                exit: Exit::Dispatch(0x1006),
            },
            Step {
                cpu: &[(24, 0x4433_017f), (56, 0x1008), (144, 2)],
                ram: &[],
                exit: Exit::Dispatch(0x1008),
            },
            Step {
                cpu: &[(24, 0x4433_0101), (56, 0x100b), (144, 3)],
                ram: &[],
                exit: Exit::Dispatch(0x100b),
            },
        ],
    );
}

fn check_memory(flags: &[&str], step: &ModuleFile) {
    for (name, code, eax, memory, result, kind, left, right) in [
        (
            "ADD register reads a readonly source",
            &[0x03, 0x03][..],
            0x10,
            &[0xf0, 0xff, 0xff, 0xff][..],
            0,
            10,
            0x10,
            0xffff_fff0,
        ),
        (
            "CMP memory needs no write permission",
            &[0x39, 0x03][..],
            1,
            &[0, 0, 0, 0x80][..],
            1,
            9,
            0x8000_0000,
            1,
        ),
        (
            "CMP register reads memory in operand order",
            &[0x3b, 0x03][..],
            1,
            &[0, 0, 0, 0x80][..],
            1,
            9,
            1,
            0x8000_0000,
        ),
        (
            "ADD reads old address register",
            &[0x03, 0x00][..],
            0x4000,
            &[5, 0, 0, 0][..],
            0x4005,
            10,
            0x4000,
            5,
        ),
    ] {
        let mut image = image(code);
        image.register(24, eax);
        image.register(36, 0x4000);
        image.map(4, 0x8000, false);
        image.data(0x8000, memory);
        let next = 0x1000 + code.len() as u32;
        let mut changes = recipe(kind, left, right).to_vec();
        changes.extend_from_slice(&[(24, result), (56, next), (144, 0)]);
        both(
            step,
            flags,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram: &[],
                exit: Exit::Dispatch(next),
            }],
        );
    }
    for (name, next_frame) in [("contiguous RMW", 0x9000), ("scattered RMW", 0xa000)] {
        let code = [0x01, 0x03];
        let mut image = image(&code);
        image.register(24, 1);
        image.register(36, 0x4ffe);
        image.map(4, 0x8000, true);
        image.map(5, next_frame, true);
        image.data(0x8ffd, &[0xa5, 0xff, 0xff]);
        image.data(next_frame, &[0xff, 0x7f, 0x5a]);
        both(
            step,
            flags,
            name,
            &code,
            1,
            &image,
            &[Step {
                cpu: &[
                    (0, 0xa5a5_a50a),
                    (4, 0x7fff_ffff),
                    (8, 1),
                    (56, 0x1002),
                    (144, 0),
                ],
                ram: &[(0x8ffe, &[0, 0]), (next_frame, &[0, 0x80])],
                exit: Exit::Dispatch(0x1002),
            }],
        );
    }
    for (name, code, eax, before, after, kind, left, right) in [
        (
            "byte RMW uses original address AL",
            &[0x00, 0x00][..],
            0x4020,
            &[0xe0][..],
            &[0][..],
            2,
            0xe0,
            0x20,
        ),
        (
            "word RMW wraps only the selected width",
            &[0x66, 0x01, 0x43, 0x80][..],
            1,
            &[0xff, 0xff][..],
            &[0, 0][..],
            6,
            0xffff,
            1,
        ),
        (
            "group byte immediate RMW",
            &[0x80, 0x03, 0x80][..],
            1,
            &[0x80][..],
            &[0][..],
            2,
            0x80,
            0x80,
        ),
    ] {
        let mut image = image(code);
        image.register(24, eax);
        image.register(36, if code[0] == 0x66 { 0x40a0 } else { 0x4020 });
        image.map(4, 0x8000, true);
        image.data(0x801f, &[0xa5, 0xcc, 0xcc, 0x5a]);
        image.data(0x8020, before);
        let next = 0x1000 + code.len() as u32;
        let mut changes = recipe(kind, left, right).to_vec();
        changes.extend_from_slice(&[(56, next), (144, 0)]);
        both(
            step,
            flags,
            name,
            code,
            1,
            &image,
            &[Step {
                cpu: &changes,
                ram: &[(0x8020, after)],
                exit: Exit::Dispatch(next),
            }],
        );
    }
}

fn check_faults(flags: &[&str], step: &ModuleFile) {
    for (name, faulting, first_writable, second_page, fault) in [
        (
            "readonly RMW preserves earlier flags",
            &[0x01, 0x03][..],
            false,
            None,
            0x0004_0003_0000_4ffe,
        ),
        (
            "missing RMW tail keeps every byte",
            &[0x01, 0x03][..],
            true,
            None,
            0x0004_0002_0000_5000,
        ),
        (
            "readonly RMW tail keeps every byte",
            &[0x01, 0x03][..],
            true,
            Some(false),
            0x0004_0003_0000_5000,
        ),
        (
            "ADD source fault preserves earlier flags and destination",
            &[0x03, 0x03][..],
            false,
            None,
            0x0004_0000_0000_5000,
        ),
        (
            "CMP missing tail is a read fault",
            &[0x39, 0x03][..],
            true,
            None,
            0x0004_0000_0000_5000,
        ),
    ] {
        let code = [&[0x01, 0xd1][..], faulting].concat();
        let mut image = image(&code);
        image.register(24, 1);
        image.register(28, 0x7fff_fffe);
        image.register(32, 2);
        image.register(36, 0x4ffe);
        image.map(4, 0x8000, first_writable);
        if let Some(writable) = second_page {
            image.map(5, 0xa000, writable);
        }
        image.data(0x8ffd, &[0xa5, 0xff, 0xff]);
        image.data(0xa000, &[0xff, 0xff, 0x5a]);
        both(
            step,
            flags,
            name,
            &code,
            2,
            &image,
            &[
                Step {
                    cpu: &[
                        (0, 0xa5a5_a50a),
                        (4, 0x7fff_fffe),
                        (8, 2),
                        (28, 0x8000_0000),
                        (56, 0x1002),
                        (144, 0),
                    ],
                    ram: &[],
                    exit: Exit::Dispatch(0x1002),
                },
                Step {
                    cpu: &[],
                    ram: &[],
                    exit: Exit::Fault(fault),
                },
            ],
        );
    }
    let code = [0x0f, 0x94, 0x03];
    let mut image = image(&code);
    image.register(36, 0x4000);
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0xa5]);
    both(
        step,
        flags,
        "SETcc denied destination preserves incoming flags",
        &code,
        1,
        &image,
        &[Step {
            cpu: &[],
            ram: &[],
            exit: Exit::Fault(0x0004_0003_0000_4000),
        }],
    );
}

fn execute(flags: &[&str]) {
    let module = compile_interpreter_step().unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    let step = ModuleFile::new(&module);
    check_register_aliases(flags, &step);
    check_memory(flags, &step);
    check_faults(flags, &step);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn arithmetic_operands_execute_in_v8() {
    execute(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn arithmetic_operands_execute_in_optimizing_v8() {
    execute(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
