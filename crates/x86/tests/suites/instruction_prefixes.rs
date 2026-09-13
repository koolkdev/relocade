#[path = "instruction_prefixes/selection.rs"]
mod selection;

use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::Gpr32;
use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::Validator;

use crate::support::machine;
use crate::support::step;
use machine::{check, Exit, Image, Step};
use step::TestModule;

fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.registers.eax = 0x4433_2211;
    image.cpu.registers.ebx = 0x10ff_eedd;
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
        (1, &[0xf0, 0x8b, 0][..], 0xf0),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        assert!(
            matches!(compile_block_from_bytes(0x1000, &code, 1), Err(BlockError::UnsupportedInstruction { address: 0x1000, opcode: actual }) if actual == opcode)
        );
    }
}

#[test]
fn missing_prefixed_instruction_fields_fault_before_data_access() {
    let step = TestModule::interpreter();
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
        image.cpu.eip = start;
        image.cpu.registers.ebx = 0x4000;
        image.guest.clear();
        image.data(0x3000 + (start & 0xfff), available_bytes);
        let expected_cpu = image.cpu;
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
}

#[test]
fn prefix_length_limits_precede_fetch_and_unsupported_checks() {
    let step = TestModule::interpreter();
    // These fixtures leave the next page absent. A required byte below offset 15
    // can page-fault; a request at offset 15 reports GP before consulting memory.
    for (name, prefixes, suffix, start, expected) in [
        (
            "required ModRM is beyond the instruction limit",
            14,
            &[0x8b][..],
            0x1ff1,
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "required SIB is beyond the instruction limit",
            13,
            &[0x8b, 0x04][..],
            0x1ff1,
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "fifteen prefixes stop before byte sixteen",
            15,
            &[][..],
            0x1ff1,
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "word immediate reaches length limit before missing page",
            13,
            &[0xb8, 0x34][..],
            0x1ff1,
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "missing immediate byte below length limit wins",
            13,
            &[0xb8][..],
            0x1ff2,
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "dword address reaches length limit before missing page",
            11,
            &[0xa1, 0, 0x40, 0][..],
            0x1ff1,
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "missing address byte below length limit wins",
            11,
            &[0xa1, 0, 0x40][..],
            0x1ff2,
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        let mut image = image(&code);
        image.cpu.eip = start;
        image.guest.clear();
        image.data(0x3000 + (start & 0xfff), &code);
        let expected_cpu = image.cpu;
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: expected,
            }],
        );
    }
    for (name, prefixes, suffix, start, expected) in [
        (
            "unsupported opcode at last admitted byte",
            14,
            &[0x62][..],
            0x1ff1,
            Exit::Other(0x0008_0062_0000_1ff1),
        ),
        (
            "unsupported group at last admitted ModRM",
            13,
            &[0xc7, 0x0d][..],
            0x1ff1,
            Exit::Other(0x0008_00c7_0000_1ff1),
        ),
        (
            "address prefix continues scanning",
            1,
            &[0x67][..],
            0x1ffe,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
    ] {
        let mut code = vec![0x66; prefixes];
        code.extend_from_slice(suffix);
        let mut image = image(&code);
        image.cpu.eip = start;
        image.guest.clear();
        image.data(0x3000 + (start & 0xfff), &code);
        let expected_cpu = image.cpu;
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: expected,
            }],
        );
    }
}

#[test]
fn an_overlong_successor_preserves_completed_word_progress() {
    let step = TestModule::interpreter();
    let mut code = vec![0x66, 0xb8, 0x34, 0x12];
    code.extend_from_slice(&[0x66; 15]);
    let image = image(&code);
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.registers.eax = 0x4433_1234;
    expected_cpu.eip = 0x1004;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1004),
    });

    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Other(0x0002_0000_0000_0000),
    });

    check(
        step,
        "overlong next instruction preserves completed word progress",
        &image,
        &steps,
    );
}

fn complete_prefix_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, eax, stored) in [
        (
            "byte opcode-register immediate",
            &[0x66, 0xb4, 0x80][..],
            0x4433_8011,
            false,
        ),
        (
            "byte register to r/m",
            &[0x66, 0x88, 0xd8][..],
            0x4433_22dd,
            false,
        ),
        (
            "byte r/m to register",
            &[0x66, 0x8a, 0xe3][..],
            0x4433_dd11,
            false,
        ),
        (
            "byte r/m immediate",
            &[0x66, 0xc6, 0xc4, 0x80][..],
            0x4433_8011,
            false,
        ),
        (
            "byte absolute load",
            &[0x66, 0xa0, 0x20, 0x40, 0, 0][..],
            0x4433_2280,
            false,
        ),
        (
            "byte absolute store",
            &[0x66, 0xa2, 0x20, 0x40, 0, 0][..],
            0x4433_2211,
            true,
        ),
    ] {
        let mut case = Case::preserving_flags(name, code)
            .register(Gpr32::Eax, 0x4433_2211, eax)
            .initial_register(Gpr32::Ebx, 0x10ff_eedd)
            .map_page(4, 0x8000, ReadWrite)
            .backing(0x801f, &[0xa5, 0x80, 0x5a]);
        if stored {
            case = case.expect_memory(0x4020, &[0x11]);
        }
        cases.push(case);
    }
    for (name, start, first_page, first_frame, second_page) in [
        ("prefix at page end", 0x1fff, 1, 0x3000, 2),
        (
            "instruction fetch wraps EIP",
            0xffff_fffd,
            0xfffff,
            0x8000,
            0,
        ),
    ] {
        cases.push(
            Case::preserving_flags(name, &[0x66, 0xb8, 0x34, 0x12])
                .at(start)
                .register(Gpr32::Eax, 0x4433_2211, 0x4433_1234)
                .initial_register(Gpr32::Ebx, 0x10ff_eedd)
                .map_page(first_page, first_frame, ReadOnly)
                .map_page(second_page, 0xa000, ReadOnly),
        );
    }
    cases.push(
        Case::preserving_flags(
            "complete word instruction needs no following page",
            &[0x66, 0xb8, 0x34, 0x12],
        )
        .at(0x1ffc)
        .register(Gpr32::Eax, 0x4433_2211, 0x4433_1234)
        .initial_register(Gpr32::Ebx, 0x10ff_eedd),
    );
    cases.push(
        Case::preserving_flags(
            "prefixed SIB displacement crosses scattered code pages",
            &[
                0x66, 0x66, 0x66, 0xc7, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0xa1, 0x88,
            ],
        )
        .at(0x1ff8)
        .initial_registers(&[
            (Gpr32::Eax, 0x4433_2211),
            (Gpr32::Ebx, 0xffff_fff0),
            (Gpr32::Ecx, 4),
        ])
        .map_page(1, 0x3000, ReadOnly)
        .map_page(2, 0xa000, ReadOnly)
        .map_page(4, 0x8000, ReadWrite)
        .backing(0x801f, &[0xa5, 0, 0, 0x5a])
        .expect_memory(0x4020, &[0xa1, 0x88]),
    );
    let maximum = [vec![0x66; 12], vec![0xb8, 0x34, 0x12]].concat();
    cases.push(
        Case::preserving_flags(
            "fifteen-byte word MOV retires without a sixteenth byte",
            &maximum,
        )
        .at(0x1ff1)
        .register(Gpr32::Eax, 0x4433_2211, 0x4433_1234)
        .initial_register(Gpr32::Ebx, 0x10ff_eedd),
    );
    cases
}
test_cases!(
    operand_sizes_and_complete_prefix_fetches,
    complete_prefix_cases()
);

test_sequences!(
    repeated_override_ends_with_its_instruction,
    [SequenceCase::preserving_flags(
        "repeated override is idempotent and ends with its instruction"
    )
    .initial_registers(&[(Gpr32::Eax, 0x4433_2211), (Gpr32::Ebx, 0x10ff_eedd)])
    .step(
        Checkpoint::preserving_flags(&[0x66, 0x66, 0xb8, 0x34, 0x12])
            .register(Gpr32::Eax, 0x4433_1234)
    )
    .step(
        Checkpoint::preserving_flags(&[0xb9, 0x55, 0x66, 0x77, 0x88])
            .register(Gpr32::Ecx, 0x8877_6655)
    )]
);
