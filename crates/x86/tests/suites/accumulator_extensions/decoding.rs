use crate::support::{
    cases::{test_cases, InstructionCase as Case},
    machine::{check, Exit, Image, Step},
    step::TestModule,
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Edx},
};
use wasmparser::Validator;

use super::ENCODINGS;

#[test]
fn opcode_only_encodings_need_neither_operands_nor_successor_bytes() {
    for (_, code) in ENCODINGS {
        for available in 0..code.len() {
            assert!(matches!(
                compile_block_from_bytes(0x1000, &code[..available], 1),
                Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                    if actual == available
            ));
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&complete.bytes).unwrap();
        assert_eq!(
            compile_block_from_bytes(0x1000, &[code, &[0x0f]].concat(), 1)
                .unwrap()
                .bytes,
            complete.bytes
        );
    }
}

fn page_and_wrap_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for ((name, code), (eax, edx)) in ENCODINGS.into_iter().zip([
        (0x8000_ff81, 0xccbb_aa99),
        (0xffff_8081, 0xccbb_aa99),
        (0x8000_8081, 0xccbb_ffff),
        (0x8000_8081, 0xffff_ffff),
    ]) {
        for origin in [0x2000 - code.len() as u32, u32::MAX] {
            cases.push(
                Case::preserving_flags(format!("{name} at {origin:08x}"), code)
                    .at(origin)
                    .instruction_count(u32::MAX)
                    .register(Eax, 0x8000_8081, eax)
                    .register(Edx, 0xccbb_aa99, edx),
            );
        }
    }
    cases
}

fn maximum_length_cases() -> Vec<Case> {
    [
        (0x98, 0x4433_ff80, 0xccbb_aa99),
        (0x99, 0x4433_0080, 0xccbb_0000),
    ]
    .into_iter()
    .map(|(opcode, eax, edx)| {
        let code = [vec![0x66; 14], vec![opcode]].concat();
        Case::preserving_flags(
            format!("fourteen operand prefixes before {opcode:02x}"),
            &code,
        )
        .at(0x1ff1)
        .register(Eax, 0x4433_0080, eax)
        .register(Edx, 0xccbb_aa99, edx)
    })
    .collect()
}

#[test]
fn extension_completion_survives_the_next_instruction_fetch_fault() {
    for ((name, code), (eax, edx)) in ENCODINGS.into_iter().zip([
        (0x4433_ff80, 0xccbb_aa99),
        (0xffff_8080, 0xccbb_aa99),
        (0x4433_8080, 0xccbb_ffff),
        (0x4433_8080, 0),
    ]) {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.cpu.registers.eax = 0x4433_8080;
        image.cpu.registers.edx = 0xccbb_aa99;
        image.data(0x4000 - code.len() as u32, code);
        let mut completed = image.cpu;
        completed.registers.eax = eax;
        completed.registers.edx = edx;
        completed.eip = 0x2000;
        completed.instruction_count = 0;
        check(
            TestModule::interpreter(),
            name,
            &image,
            &[
                Step {
                    cpu: completed,
                    ram: &[],
                    exit: Exit::Dispatch(0x2000),
                },
                Step {
                    cpu: completed,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x2000,
                        error: 0x10,
                    },
                },
            ],
        );
    }
}

#[test]
fn a_sixteenth_opcode_byte_is_rejected_before_fetch() {
    let prefixes = [0x66; 15];
    for opcode in [0x98, 0x99] {
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &[&prefixes[..], &[opcode]].concat(), 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
    }
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ff1;
    image.data(0x3ff1, &prefixes);
    check(
        TestModule::interpreter(),
        "the length limit precedes fetching the opcode from the absent page",
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::Other(0x0002_0000_0000_0000),
        }],
    );
}

test_cases!(
    final_opcode_fetch_and_fallthrough_wrap,
    page_and_wrap_cases()
);
test_cases!(
    repeated_prefixes_keep_the_word_operand_size,
    maximum_length_cases()
);
