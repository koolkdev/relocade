use super::{input_flags, FORMS};
use crate::support::{
    cases::{test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case},
    machine::{self, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32::Ecx};

fn encodings() -> Vec<Vec<u8>> {
    FORMS
        .into_iter()
        .flat_map(|(opcode, _)| {
            [
                vec![opcode, 0x66],
                vec![0x66, opcode, 0xe3],
                vec![0x66, 0x66, opcode, 0xe0],
            ]
        })
        .collect()
}

#[test]
fn snapshots_consume_one_displacement_and_stop_at_the_branch() {
    for code in encodings() {
        for available in 0..code.len() {
            assert_eq!(
                compile_block_from_bytes(0x1000, &code[..available], 1).err(),
                Some(BlockError::TruncatedInstruction {
                    address: 0x1000,
                    available
                }),
                "{code:02x?}, available {available}",
            );
        }
        let complete = compile_block_from_bytes(0x1000, &code, 1).unwrap();
        for input in [code.clone(), [&code[..], &[0xf4, 0x66, 0x0f]].concat()] {
            assert_eq!(
                compile_block_from_bytes(0x1000, &input, u32::MAX)
                    .unwrap()
                    .bytes,
                complete.bytes,
                "{code:02x?}",
            );
        }
        let preceding = [&[0xb1, 7][..], &code].concat();
        let bounded = compile_block_from_bytes(0x1000, &preceding, 2).unwrap();
        assert_eq!(
            compile_block_from_bytes(0x1000, &preceding, u32::MAX)
                .unwrap()
                .bytes,
            bounded.bytes,
        );
        assert_eq!(
            compile_block_from_bytes(0x1000, &preceding, 1)
                .unwrap()
                .bytes,
            compile_block_from_bytes(0x1000, &[0xb1, 7], 1)
                .unwrap()
                .bytes,
        );
    }
}

fn at_page_end(code: &[u8], count: u32, zero: bool) -> Image {
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x2000 - code.len() as u32;
    image.cpu.registers.ecx = count;
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.zf = u8::from(zero);
    image.data(0x4000 - code.len() as u32, code);
    image
}

fn unchanged_exit(engine: Engine, image: &Image, name: &str, exit: Exit) {
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 1),
        machine::expected(
            image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit
            }]
        ),
        "{name}",
    );
}

fn check_missing_bytes(engine: Engine) {
    for code in encodings() {
        for available in 1..code.len() {
            // Both branch outcomes must fetch the displacement before changing ECX.
            for (count, zero) in [(0, false), (0, true), (1, false), (1, true)] {
                let image = at_page_end(&code[..available], count, zero);
                unchanged_exit(
                    engine,
                    &image,
                    &format!(
                        "missing byte after {:02x?}, ECX={count}, ZF={zero}",
                        &code[..available]
                    ),
                    Exit::PageFault {
                        address: 0x2000,
                        error: 0x10,
                    },
                );
            }
        }
    }
}

#[test]
fn missing_bytes_preserve_state_in_wasmtime() {
    check_missing_bytes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn missing_bytes_preserve_state_in_v8() {
    check_missing_bytes(Engine::V8);
}

fn check_rejected_encodings(engine: Engine) {
    for (opcode, name) in FORMS {
        let overlong = [vec![0x66; 14], vec![opcode]].concat();
        for code in [overlong.clone(), [&overlong[..], &[0x7f]].concat()] {
            assert_eq!(
                compile_block_from_bytes(0x1ff1, &code, 1).err(),
                Some(BlockError::InstructionTooLong { address: 0x1ff1 }),
                "{name}",
            );
        }
        for (count, zero) in [(0, false), (1, true)] {
            let image = at_page_end(&overlong, count, zero);
            unchanged_exit(engine, &image, name, Exit::Other(0x0002_0000_0000_0000));
        }
    }

    for code in [
        vec![0x67],
        vec![0x66, 0x67],
        [vec![0x66; 14], vec![0x67]].concat(),
    ] {
        let image = at_page_end(&code, 0x0001_0000, true);
        assert_eq!(
            compile_block_from_bytes(image.cpu.eip, &code, 1).err(),
            Some(if code.len() == 15 {
                BlockError::InstructionTooLong {
                    address: image.cpu.eip,
                }
            } else {
                BlockError::TruncatedInstruction {
                    address: image.cpu.eip,
                    available: code.len(),
                }
            }),
        );
        unchanged_exit(
            engine,
            &image,
            "address-size prefix still requires an opcode",
            if code.len() == 15 {
                Exit::Other(0x0002_0000_0000_0000)
            } else {
                Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                }
            },
        );
    }
}

#[test]
fn overlong_and_incomplete_encodings_preserve_state_in_wasmtime() {
    check_rejected_encodings(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn overlong_and_incomplete_encodings_preserve_state_in_v8() {
    check_rejected_encodings(Engine::V8);
}

#[rustfmt::skip]
fn completed_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (origin, prefix, displacement, taken_target, fallthrough) in [
        (0x1ffe, vec![], 0x7f, 0x207f, 0x2000),
        (0x1fff, vec![], 0x80, 0x1f81, 0x2001),
        (0x1ffd, vec![0x66], 0x7f, 0x207f, 0x2000),
        (0x1ffe, vec![0x66], 0x80, 0x1f81, 0x2001),
        (0x1ff1, vec![0x66; 13], 0x7f, 0x207f, 0x2000),
    ] {
        for (opcode, count, after, zero, taken) in [
            (0xe3, 0, 0, false, true), (0xe3, 1, 1, false, false),
            (0xe2, 2, 1, false, true), (0xe2, 1, 0, false, false),
            (0xe1, 2, 1, true, true), (0xe1, 2, 1, false, false),
            (0xe0, 2, 1, false, true), (0xe0, 2, 1, true, false),
        ] {
            let code = [&prefix[..], &[opcode, displacement]].concat();
            cases.push(Case::new(format!("complete {code:02x?} at {origin:08x}, taken={taken}"),
                &code, input_flags(zero), Flags::all(Preserved))
                .at(origin).register(Ecx, count, after).preserve_flag_record()
                .dispatch(if taken { taken_target } else { fallthrough }));
        }
    }
    cases
}

test_cases!(
    page_boundaries_and_fifteen_byte_encodings,
    completed_cases()
);
