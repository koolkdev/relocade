use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::Validator;

use crate::support::{
    arithmetic,
    machine::{both, check, Exit, Step},
    step::TestModule,
};

#[test]
fn conditional_moves_require_the_selected_address_fields() {
    for code in [
        &[0x0f, 0x40, 0xc1][..],
        &[0x66, 0x0f, 0x4f, 0x44, 0x8b, 0x80][..],
        &[0x0f, 0x44, 0x05, 0x20, 0x40, 0, 0][..],
    ] {
        for available in 0..code.len() {
            assert!(
                matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                        if actual == available
                ),
                "{code:02x?}, available {available}",
            );
        }
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut with_suffix = code.to_vec();
        with_suffix.push(0x0f);
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes,
        );
    }
}

#[test]
fn repeated_operand_prefixes_reach_but_do_not_exceed_fifteen_bytes() {
    let mut code = vec![0x66; 12];
    code.extend_from_slice(&[0x0f, 0x44, 0xc1]);
    let mut image = arithmetic::image(&[]);
    image.cpu.eip = 0x1ff1;
    image.cpu.registers.eax = 0x4433_2211;
    image.cpu.registers.ecx = 0x8877_6655;
    image.data(0x3ff1, &code);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_6655;
    cpu.eip = 0x2000;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "CMOVE finishes at the last available encoding byte",
        &code,
        1,
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(0x2000),
        }],
    );

    let mut code = vec![0x66; 13];
    code.extend_from_slice(&[0x0f, 0x44]);
    assert!(matches!(
        compile_block_from_bytes(0x1ff1, &code, 1),
        Err(BlockError::InstructionTooLong { address: 0x1ff1 }),
    ));
    let mut image = arithmetic::image(&[]);
    image.cpu.eip = 0x1ff1;
    image.data(0x3ff1, &code);
    check(
        TestModule::interpreter(),
        "CMOV ModRM beyond byte fifteen reports GP before fetching",
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::Other(0x0002_0000_0000_0000),
        }],
    );
}

#[test]
fn false_conditions_still_fetch_the_complete_instruction() {
    for code in [
        &[0x0f, 0x45][..],
        &[0x66, 0x0f, 0x45, 0x05, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = arithmetic::image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "false CMOVNE requires its ModRM and full displacement",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            }],
        );
    }
}
