use super::*;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, SegmentProfile};

#[test]
fn encodings_consume_exactly_the_memory_address() {
    for code in [
        &[0x62, 0x03][..],
        &[0x66, 0x62, 0x44, 0x8b, 0x80],
        &[0x62, 0x3d, 0, 0x40, 0, 0],
        &[0x67, 0x62, 0x06, 0, 0x40],
        &[0x64, 0x67, 0x66, 0x62, 0x42, 0x80],
        &[0x62, 0x8c, 0x25, 0x78, 0x56, 0x34, 0x12],
    ] {
        for available in 0..code.len() {
            assert_eq!(
                compile_block_from_bytes(0x1000, &code[..available], 1).err(),
                Some(BlockError::TruncatedInstruction {
                    address: 0x1000,
                    available
                })
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        assert_eq!(
            complete.bytes,
            compile_block_from_bytes(0x1000, &[code, &[0x0f]].concat(), 1)
                .unwrap()
                .bytes
        );
    }
}

fn fetch_boundaries(engine: Engine) {
    for modrm in 0xc0..=0xff {
        let code = [0x62, modrm];
        assert_eq!(
            compile_block_from_bytes(0x1ffe, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1ffe,
                opcode: 0x62
            })
        );
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffe;
        image.data(0x3ffe, &code);
        assert_eq!(
            engine.observe(TestModule::interpreter(), &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0008_0062_0000_1ffe),
                }]
            )
        );
    }
    for code in [
        &[0x62][..],
        &[0x62, 0x04],
        &[0x62, 0x85, 0, 0x40],
        &[0x67, 0x62, 0x06, 0],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.cpu.segments.ds = StoredSegment::unusable(0);
        image.data(0x3000 + (start & 0xfff), code);
        assert_eq!(
            engine.observe(
                TestModule::interpreter_with_profile(SegmentProfile::Segmented32),
                &image.input(),
                1
            ),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x2000,
                        error: 16
                    },
                }]
            )
        );
    }
    let code = [vec![0x66; 14], vec![0x62]].concat();
    assert_eq!(
        compile_block_from_bytes(0x1ff1, &code, 1).err(),
        Some(BlockError::InstructionTooLong { address: 0x1ff1 })
    );
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ff1;
    image.data(0x3ff1, &code);
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            }]
        )
    );
}

#[test]
fn address_fetches_and_invalid_forms_have_precise_boundaries() {
    fetch_boundaries(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_address_fetches_and_invalid_forms() {
    fetch_boundaries(Engine::V8);
}

fn maximum_length() -> Vec<Case> {
    [false, true]
        .into_iter()
        .map(|fails| {
            let code = [vec![0x66; 13], vec![0x62, 0x03]].concat();
            let case = Case::preserving_flags(
                format!("BOUND completes at byte fifteen, fails={fails}"),
                &code,
            )
            .at(0x1ff1)
            .initial_registers(&[(Eax, if fails { 8 } else { 7 }), (Ebx, 0x4000)])
            .memory(0x4000, &pair(true, -5, 7), ReadOnly);
            if fails {
                case.bound_range_exceeded()
            } else {
                case
            }
        })
        .collect()
}

test_cases!(fifteen_byte_instruction, maximum_length());
