use super::*;
use crate::support::encoding::check_length;
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Segment, SegmentAttributes, StoredSegment};

fn fixed_accumulators() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, input, output, flags) in [
        (&[0x37][..], 0x120a, 0x1300, unpacked_flags(true)),
        (&[0x3f][..], 0x120a, 0x1104, unpacked_flags(true)),
        (&[0x27][..], 0x229a, 0x2200, packed_flags(0, true, true)),
        (&[0x2f][..], 0x229a, 0x2234, packed_flags(0x34, true, true)),
        (&[0xd4, 10][..], 0xab51, 0x0801, digit_flags(1)),
        (&[0xd5, 10][..], 0x0909, 0x0063, digit_flags(0x63)),
    ] {
        for code16 in [false, true] {
            for prefixes in [&[][..], &[0x66][..], &[0x64, 0x67, 0x66][..]] {
                let bytes = [prefixes, code].concat();
                let mut case = Case::new(
                    format!("{bytes:02x?} keeps its fixed accumulator, CS.D16={code16}"),
                    &bytes,
                    Flags::all(false),
                    flags,
                )
                .register(Eax, 0x4433_0000 | input, 0x4433_0000 | output);
                if code16 {
                    case = case.segmented_only().segment(
                        Segment::Cs,
                        StoredSegment {
                            attributes: SegmentAttributes::from_bits(0x07),
                            ..StoredSegment::flat_code32(0x1b)
                        },
                    );
                }
                cases.push(case);
            }
        }
        let bytes = [vec![0x66; 15 - code.len()], code.to_vec()].concat();
        cases.push(
            Case::new(
                format!("{code:02x?} ends at byte fifteen and the last mapped byte"),
                &bytes,
                Flags::all(false),
                flags,
            )
            .at(0x1ff1)
            .register(Eax, 0x4433_0000 | input, 0x4433_0000 | output),
        );
    }
    cases.push(
        Case::preserving_flags("AAM zero base needs no successor fetch", &[0xd4, 0])
            .at(0x1ffe)
            .initial_register(Eax, 0x4433_ab51)
            .divide_error(),
    );
    cases
}

#[test]
fn radix_forms_consume_exactly_one_immediate_byte() {
    for code in [&[0xd4, 0][..], &[0xd5, 255][..], &[0x66, 0xd4, 10][..]] {
        check_length(code);
    }
}

fn immediate_fetch_faults(engine: Engine) {
    for opcode in [0xd4, 0xd5] {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1fff;
        image.cpu.registers.eax = 0x4433_ab00;
        image.data(0x3fff, &[opcode]);
        assert_eq!(
            engine.observe(TestModule::interpreter(), &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x2000,
                        error: 16,
                    },
                }]
            )
        );

        let bytes = [vec![0x66; 14], vec![opcode]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &bytes, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 })
        );
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &bytes);
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
}

#[test]
fn interpreter_immediate_fetch_faults_precede_execution() {
    immediate_fetch_faults(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interpreter_immediate_fetch_faults_precede_execution() {
    immediate_fetch_faults(Engine::V8);
}

test_cases!(code_sizes_prefixes_and_final_byte, fixed_accumulators());
