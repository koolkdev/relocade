use crate::support::encoding::check_length;
use wasm86_x86::{compile_block_from_bytes, BlockError};

use super::{INITIAL_FLAGS, PRESERVED_FLAGS};
use crate::support::{
    arithmetic,
    cases::{test_cases, InstructionCase as Case},
    machine::{check, Exit, Step},
    step::TestModule,
};

#[test]
fn conditional_moves_require_the_selected_address_fields() {
    for code in [
        &[0x0f, 0x40, 0xc1][..],
        &[0x66, 0x0f, 0x4f, 0x44, 0x8b, 0x80][..],
        &[0x0f, 0x44, 0x05, 0x20, 0x40, 0, 0][..],
    ] {
        check_length(code);
    }
}

#[test]
fn repeated_operand_prefixes_reject_a_sixteenth_byte_before_fetch() {
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

fn complete_prefix_cases() -> Vec<Case> {
    let mut code = vec![0x66; 12];
    code.extend_from_slice(&[0x0f, 0x44, 0xc1]);
    vec![Case::new(
        "CMOVE finishes at the last available encoding byte",
        &code,
        INITIAL_FLAGS,
        PRESERVED_FLAGS,
    )
    .at(0x1ff1)
    .preserve_flag_record()
    .register(wasm86_x86::Gpr32::Eax, 0x4433_2211, 0x4433_6655)
    .initial_register(wasm86_x86::Gpr32::Ecx, 0x8877_6655)]
}

test_cases!(fifteen_byte_instruction, complete_prefix_cases());
