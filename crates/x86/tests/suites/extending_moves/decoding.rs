use crate::support::cases::{test_cases, InstructionCase as Case};
use crate::support::encoding::check_length;
use wasm86_x86::Gpr32;
use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::machine::{byte_register_image, check, Exit, Step};
use crate::support::step::TestModule;

#[test]
fn encoded_fields_are_required_but_bytes_after_the_instruction_are_not() {
    for code in [
        &[0x0f, 0xb6, 0xc4][..],
        &[0x66, 0x0f, 0xbe, 0xc4][..],
        &[0x0f, 0xb7, 0x05, 0x20, 0x40, 0, 0][..],
        &[0x66, 0x0f, 0xbf, 0x44, 0x8b, 0x80][..],
        &[0x66, 0x66, 0x0f, 0xb7, 0x04, 0x25, 0x20, 0x40, 0, 0][..],
    ] {
        check_length(code);
    }
}

fn maximum_length_cases() -> Vec<Case> {
    let mut code = vec![0x66; 12];
    code.extend_from_slice(&[0x0f, 0xbe, 0xc4]);
    vec![Case::preserving_flags("maximum-length MOVSX", &code)
        .register(Gpr32::Eax, 0x4433_8011, 0x4433_ff80)
        .at(0x1ff1)]
}
test_cases!(
    fifteen_byte_instruction_retires_without_fetching_a_sixteenth_byte,
    maximum_length_cases()
);

#[test]
fn length_limit_precedes_fetching_an_unavailable_field() {
    let step = TestModule::interpreter();
    for (prefixes, suffix) in [
        (14, &[0x0f][..]),
        (13, &[0x0f, 0xbe][..]),
        (12, &[0x0f, 0xb6, 0x04][..]),
        (12, &[0x0f, 0xbf, 0x45][..]),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 }),
        ));
        let mut image = byte_register_image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            step,
            "required field begins beyond offset fourteen",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0002_0000_0000_0000),
            }],
        );
    }
}

#[test]
fn incomplete_encoding_faults_before_reading_data() {
    let step = TestModule::interpreter();
    for code in [
        &[0x0f, 0xb6][..],
        &[0x66, 0x0f, 0xbe, 0x04][..],
        &[0x66, 0x0f, 0xb7, 0x05, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = byte_register_image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            step,
            "missing encoding byte precedes an unmapped data operand",
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

#[test]
fn completed_extension_is_published_before_a_later_fetch_fault() {
    let step = TestModule::interpreter();
    let mut image = byte_register_image(&[]);
    image.cpu.eip = 0x1ffa;
    image.data(0x3ffa, &[0x0f, 0xb6, 0xc4, 0x66, 0x0f, 0xbe]);
    let mut expected = image.cpu;
    expected.registers.eax = 0x22;
    expected.eip = 0x1ffd;
    expected.instruction_count = 0;
    check(
        step,
        "second MOVSX cannot fetch its ModRM",
        &image,
        &[
            Step {
                cpu: expected,
                ram: &[],
                exit: Exit::Dispatch(0x1ffd),
            },
            Step {
                cpu: expected,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            },
        ],
    );
}
