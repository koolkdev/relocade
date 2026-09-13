use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Esp},
};

use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions},
    machine::{self, Exit, Image, Step},
    step::{Engine, TestModule},
};

const ENCODINGS: &[&[u8]] = &[
    &[0xe8, 0x66, 0xe8, 0xff, 0xc3],
    &[0x66, 0xe8, 0xc2, 0xff],
    &[0x66, 0x66, 0xe8, 0xc3, 0xff],
    &[0xff, 0xd4],
    &[0x66, 0xff, 0xd0],
    &[0xff, 0x10],
    &[0x66, 0xff, 0x54, 0x8c, 0x80],
    &[0xff, 0x94, 0x25, 0x66, 0xe8, 0xc3, 0xc2],
    &[0x66, 0xff, 0x15, 0xc3, 0xc2, 0xff, 0xe8],
    &[0xff, 0x14, 0x25, 0x66, 0xe8, 0xc3, 0xc2],
    &[0xff, 0xe7],
    &[0x66, 0xff, 0xe4],
    &[0xff, 0x20],
    &[0x66, 0xff, 0x64, 0x8c, 0x80],
    &[0xff, 0xa4, 0x25, 0x66, 0xe8, 0xc3, 0xc2],
    &[0x66, 0xff, 0x25, 0xc3, 0xc2, 0xff, 0xe8],
    &[0xff, 0x24, 0x25, 0x66, 0xe8, 0xc3, 0xc2],
    &[0xc3],
    &[0x66, 0xc3],
    &[0x66, 0x66, 0xc3],
    &[0xc2, 0xe8, 0xff],
    &[0x66, 0xc2, 0xc3, 0xff],
    &[0x66, 0x66, 0xc2, 0xff, 0xff],
];

#[test]
fn snapshots_require_every_field_and_stop_at_each_near_control_form() {
    for &code in ENCODINGS {
        for available in 0..code.len() {
            assert_eq!(
                compile_block_from_bytes(0x1000, &code[..available], 1).err(),
                Some(BlockError::TruncatedInstruction {
                    address: 0x1000,
                    available,
                }),
                "{code:02x?}, available {available}",
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        for input in [code.to_vec(), [code, &[0x62, 0x66, 0x0f]].concat()] {
            assert_eq!(
                compile_block_from_bytes(0x1000, &input, u32::MAX)
                    .unwrap()
                    .bytes,
                complete.bytes,
                "{code:02x?}",
            );
        }
    }
}

#[test]
fn a_near_transfer_ends_a_snapshot_after_an_earlier_instruction() {
    for suffix in [
        &[0xe8, 0, 0, 0, 0][..],
        &[0xff, 0xd0],
        &[0xff, 0xe0],
        &[0xc3],
        &[0xc2, 0x34, 0x12],
    ] {
        let code = [&[0xb0, 0x7a][..], suffix].concat();
        let complete = compile_block_from_bytes(0x1000, &code, 2).unwrap();
        assert_eq!(
            compile_block_from_bytes(0x1000, &code, u32::MAX)
                .unwrap()
                .bytes,
            complete.bytes,
        );
        let trailing = [&code[..], &[0x62]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, u32::MAX)
                .unwrap()
                .bytes,
            complete.bytes,
        );
        assert_eq!(
            compile_block_from_bytes(0x1000, &code, 1).unwrap().bytes,
            compile_block_from_bytes(0x1000, &[0xb0, 0x7a], 1)
                .unwrap()
                .bytes,
        );
    }
}

fn image_at_page_end(code: &[u8]) -> Image {
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x2000 - code.len() as u32;
    image.cpu.flags.status_source.kind = 0xff;
    image.cpu.registers.esp = 0x4000;
    image.data(0x4000 - code.len() as u32, code);
    image
}

fn check_exit(engine: Engine, name: &str, image: &Image, exit: Exit) {
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 1),
        machine::expected(
            image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        ),
        "{name}",
    );
}

fn check_missing_fields(engine: Engine) {
    for code in [
        &[0xe8][..],
        &[0xe8, 0x78, 0x56, 0x34],
        &[0x66, 0xe8, 0x78],
        &[0xff],
        &[0xff, 0x14],
        &[0xff, 0x64, 0x8c],
        &[0xff, 0x15, 0x08, 0x90, 0],
        &[0x66, 0xff, 0x24, 0x25, 0x08, 0x90, 0],
        &[0xc2],
        &[0xc2, 0x34],
        &[0x66, 0xc2, 0x34],
    ] {
        let image = image_at_page_end(code);
        check_exit(
            engine,
            &format!("missing near-control field after {code:02x?}"),
            &image,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn missing_fields_fault_before_target_or_stack_access_in_wasmtime() {
    check_missing_fields(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn missing_fields_fault_before_target_or_stack_access_in_v8() {
    check_missing_fields(Engine::V8);
}

// Each suffix still needs an opcode, ModRM, SIB, displacement or immediate byte.
const INCOMPLETE_SUFFIXES: &[&[u8]] = &[
    &[],
    &[0xff],
    &[0xff, 0x14],
    &[0xff, 0x54, 0x24],
    &[0xff, 0xa4, 0x25, 0x08, 0x90, 0],
    &[0xe8, 0x34],
    &[0xc2, 0x34],
];

fn fifteen_bytes(suffix: &[u8]) -> Vec<u8> {
    [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat()
}

#[test]
fn snapshots_reject_a_required_sixteenth_instruction_byte() {
    for &suffix in INCOMPLETE_SUFFIXES {
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &fifteen_bytes(suffix), 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 }),
            "{suffix:02x?}",
        );
    }
}

fn check_length_limit(engine: Engine) {
    for &suffix in INCOMPLETE_SUFFIXES {
        let image = image_at_page_end(&fifteen_bytes(suffix));
        check_exit(
            engine,
            &format!("length limit before fetching byte sixteen after {suffix:02x?}"),
            &image,
            Exit::Other(0x0002_0000_0000_0000),
        );
    }
}

#[test]
fn the_length_limit_precedes_the_absent_code_page_in_wasmtime() {
    check_length_limit(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn the_length_limit_precedes_the_absent_code_page_in_v8() {
    check_length_limit(Engine::V8);
}

// Far forms remain outside the subset.
const UNSUPPORTED: &[&[u8]] = &[
    &[0x9a],
    &[0xea],
    &[0xca],
    &[0xcb],
    &[0xff, 0x1c],
    &[0xff, 0x2c],
];

#[test]
fn unsupported_forms_do_not_require_far_pointer_immediate_or_sib_fields() {
    for &suffix in UNSUPPORTED {
        for code in [suffix.to_vec(), fifteen_bytes(suffix)] {
            let start = 0x2000 - code.len() as u32;
            assert_eq!(
                compile_block_from_bytes(start, &code, 1).err(),
                Some(BlockError::UnsupportedInstruction {
                    address: start,
                    opcode: suffix[0],
                }),
                "{code:02x?}",
            );
        }
    }
}

fn check_unsupported_forms(engine: Engine) {
    for &suffix in UNSUPPORTED {
        let image = image_at_page_end(&fifteen_bytes(suffix));
        check_exit(
            engine,
            &format!("unsupported near-control neighbor {suffix:02x?}"),
            &image,
            Exit::Other(
                0x0008_0000_0000_0000 | (u64::from(suffix[0]) << 32) | u64::from(image.cpu.eip),
            ),
        );
    }
}

#[test]
fn unsupported_forms_stop_before_missing_fields_in_wasmtime() {
    check_unsupported_forms(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn unsupported_forms_stop_before_missing_fields_in_v8() {
    check_unsupported_forms(Engine::V8);
}

struct BoundaryForm {
    code: &'static [u8],
    stack_before: u32,
    stack_after: u32,
    target: u32,
    pushed: &'static [u8],
}

fn boundary_case(form: &BoundaryForm, code: &[u8]) -> Case {
    let mut case =
        Case::preserving_flags(format!("near control ends at code page: {code:02x?}"), code)
            .at(0x2000 - code.len() as u32)
            .instruction_count(u32::MAX)
            .initial_register(Eax, 0x8123_4567)
            .register(Esp, form.stack_before, form.stack_after)
            .memory(
                0x9000,
                &[
                    0x67, 0x45, 0x23, 0x81, 0xa5, 0xa5, 0xa5, 0xa5, 0x67, 0x45, 0x23, 0x81,
                ],
                if form.pushed.is_empty() {
                    Permissions::ReadOnly
                } else {
                    Permissions::ReadWrite
                },
            )
            .dispatch(form.target);
    if !form.pushed.is_empty() {
        case = case.expect_memory(form.stack_after, form.pushed);
    }
    case
}

#[rustfmt::skip]
fn page_end_cases() -> Vec<Case> {
    [
        BoundaryForm { code: &[0xe8, 0x34, 0x12, 0, 0], stack_before: 0x9004, stack_after: 0x9000, target: 0x3234, pushed: &[0, 0x20, 0, 0] },
        BoundaryForm { code: &[0x66, 0x66, 0xe8, 0x34, 0x12], stack_before: 0x9004, stack_after: 0x9002, target: 0x3234, pushed: &[0, 0x20] },
        BoundaryForm { code: &[0xff, 0xd0], stack_before: 0x9004, stack_after: 0x9000, target: 0x8123_4567, pushed: &[0, 0x20, 0, 0] },
        BoundaryForm { code: &[0x66, 0xff, 0xd0], stack_before: 0x9004, stack_after: 0x9002, target: 0x4567, pushed: &[0, 0x20] },
        BoundaryForm { code: &[0xff, 0x54, 0x24, 4], stack_before: 0x9004, stack_after: 0x9000, target: 0x8123_4567, pushed: &[0, 0x20, 0, 0] },
        BoundaryForm { code: &[0xff, 0xe0], stack_before: 0x9004, stack_after: 0x9004, target: 0x8123_4567, pushed: &[] },
        BoundaryForm { code: &[0x66, 0xff, 0xe0], stack_before: 0x9004, stack_after: 0x9004, target: 0x4567, pushed: &[] },
        BoundaryForm { code: &[0xff, 0x25, 8, 0x90, 0, 0], stack_before: 0x9004, stack_after: 0x9004, target: 0x8123_4567, pushed: &[] },
        BoundaryForm { code: &[0xc3], stack_before: 0x9000, stack_after: 0x9004, target: 0x8123_4567, pushed: &[] },
        BoundaryForm { code: &[0x66, 0xc3], stack_before: 0x9000, stack_after: 0x9002, target: 0x4567, pushed: &[] },
        BoundaryForm { code: &[0xc2, 0x34, 0x12], stack_before: 0x9000, stack_after: 0xa238, target: 0x8123_4567, pushed: &[] },
        BoundaryForm { code: &[0x66, 0x66, 0xc2, 0x34, 0x12], stack_before: 0x9000, stack_after: 0xa236, target: 0x4567, pushed: &[] },
    ].iter().map(|form| boundary_case(form, form.code)).collect()
}

#[rustfmt::skip]
fn maximum_length_cases() -> Vec<Case> {
    [
        BoundaryForm { code: &[0xe8, 0x34, 0x12], stack_before: 0x9004, stack_after: 0x9002, target: 0x3234, pushed: &[0, 0x20] },
        BoundaryForm { code: &[0xff, 0xd0], stack_before: 0x9004, stack_after: 0x9002, target: 0x4567, pushed: &[0, 0x20] },
        BoundaryForm { code: &[0xff, 0x14, 0x25, 8, 0x90, 0, 0], stack_before: 0x9004, stack_after: 0x9002, target: 0x4567, pushed: &[0, 0x20] },
        BoundaryForm { code: &[0xff, 0xe0], stack_before: 0x9004, stack_after: 0x9004, target: 0x4567, pushed: &[] },
        BoundaryForm { code: &[0xff, 0x25, 8, 0x90, 0, 0], stack_before: 0x9004, stack_after: 0x9004, target: 0x4567, pushed: &[] },
        BoundaryForm { code: &[0xc3], stack_before: 0x9000, stack_after: 0x9002, target: 0x4567, pushed: &[] },
        BoundaryForm { code: &[0xc2, 0x34, 0x12], stack_before: 0x9000, stack_after: 0xa236, target: 0x4567, pushed: &[] },
    ].iter().map(|form| boundary_case(form, &fifteen_bytes(form.code))).collect()
}

test_cases!(no_successor_or_destination_fetch, page_end_cases());
test_cases!(complete_encodings_at_byte_fifteen, maximum_length_cases());
