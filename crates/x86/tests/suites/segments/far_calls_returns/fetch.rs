use super::*;
use crate::support::encoding::check_length;
use wasm86_x86::{compile_block_from_bytes, BlockError};

fn encodings() -> Vec<Vec<u8>> {
    vec![
        immediate(false, 0x9234_5678, 0xf327),
        immediate(true, 0x5678, 0xf327),
        vec![0xff, 0x1b],
        vec![0x66, 0xff, 0x5c, 0x24, 0xf8],
        vec![0x67, 0xff, 0x1e, 0x34, 0x12],
        vec![0xff, 0x9c, 0x25, 0x78, 0x56, 0x34, 0x12],
        ret(false, None),
        ret(true, None),
        ret(false, Some(0xabcd)),
        ret(true, Some(0xabcd)),
    ]
}

#[test]
fn snapshot_forms_require_every_field_and_end_the_block() {
    for code in encodings() {
        let complete = check_length(&code);
        let trailing = [&code[..], &[0xf4]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, u32::MAX)
                .unwrap()
                .bytes,
            complete.bytes
        );
    }
}

fn missing_fields(engine: Engine) {
    for (code, available) in [
        (immediate(false, 0x9234_5678, 0xf327), 3), // Offset.
        (immediate(true, 0x5678, 0xf327), 5),       // Selector.
        (vec![0xff, 0x9c, 0x25, 0x78, 0x56, 0x34, 0x12], 6), // Displacement.
        (ret(false, Some(0xabcd)), 2),              // Cleanup.
    ] {
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - available as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - available as u32, &code[..available]);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("far control fields {code:02x?}, available {available}"),
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn incomplete_pointer_displacement_or_cleanup_faults_before_data_or_descriptor_checks() {
    missing_fields(Engine::Wasmtime);
}

fn complete_fields(engine: Engine) {
    for (returning, word, split) in [
        (false, false, 3), // Dword offset crosses the page.
        (false, true, 5),  // Word pointer's selector crosses the page.
        (true, false, 2),  // Cleanup crosses the page.
        (true, true, 4),   // Complete word return needs no next page.
    ] {
        let code = if returning {
            ret(word, Some(0x1234))
        } else {
            immediate(word, 0x200, 0x27)
        };
        let mut image = image_with_stack(&[], if returning { 0x9000 } else { 0x9008 });
        image.cpu.eip = 0x2000 - split as u32;
        image.data(0x4000 - split as u32, &code[..split]);
        if split < code.len() {
            image.map(2, 0x5000, false);
            image.data(0x5000, &code[split..]);
        }
        image.data(0x8000, &pointer(word, 0x200, 0x27));
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
        cpu.eip = 0x200;
        cpu.registers.esp = match (returning, word) {
            (true, true) => 0xa238,
            (true, false) => 0xa23c,
            (false, true) => 0x9004,
            (false, false) => 0x9000,
        };
        cpu.instruction_count = 0;
        let saved = pointer(word, image.cpu.eip + code.len() as u32, 0x1b);
        let ram = if returning {
            vec![]
        } else {
            vec![(if word { 0x8004 } else { 0x8000 }, saved.as_slice())]
        };
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &[SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)],
            Step {
                cpu,
                ram: &ram,
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn immediate_pointers_and_return_cleanup_cross_instruction_pages_in_order() {
    complete_fields(Engine::Wasmtime);
}

fn length_and_register_modes(engine: Engine) {
    for returning in [false, true] {
        for length in [15, 16] {
            let suffix = if returning {
                ret(false, Some(0))
            } else {
                immediate(false, 0x200, 0x27)
            };
            let code = [vec![0x67; length - suffix.len()], suffix].concat();
            let mut image = image_with_stack(&[], if returning { 0x9000 } else { 0x9008 });
            image.cpu.eip = 0x1ff1;
            image.data(0x3ff1, &code[..15]);
            image.data(0x8000, &pointer(false, 0x200, 0x27));
            if length == 16 {
                assert_eq!(
                    compile_block_from_bytes(0x1ff1, &code[..15], 1).err(),
                    Some(BlockError::InstructionTooLong { address: 0x1ff1 })
                );
                image.check_unchanged_exit(
                    engine,
                    TestModule::interpreter(),
                    &format!("far control exceeds fifteen bytes, returning={returning}"),
                    Exit::GeneralProtection { error: 0 },
                );
            } else {
                let mut cpu = image.cpu;
                cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
                cpu.eip = 0x200;
                cpu.registers.esp = if returning { 0x9008 } else { 0x9000 };
                cpu.instruction_count = 0;
                let ram: &[(u32, &[u8])] = if returning {
                    &[]
                } else {
                    &[(0x8000, &[0, 0x20, 0, 0, 0x1b, 0])]
                };
                check_one(
                    engine,
                    SegmentProfile::Flat32,
                    &code,
                    &image,
                    &[SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)],
                    Step {
                        cpu,
                        ram,
                        exit: Exit::Dispatch(cpu.eip),
                    },
                );
            }
        }
    }
    let code = [0xff, 0xdf];
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ffe;
    image.data(0x3ffe, &code);
    assert_eq!(
        compile_block_from_bytes(0x1ffe, &code, 1).err(),
        Some(BlockError::UnsupportedInstruction {
            address: 0x1ffe,
            opcode: 0xff
        })
    );
    image.check_unchanged_exit(
        engine,
        TestModule::interpreter(),
        "far CALL rejects a register operand",
        Exit::Other(0x0008_00ff_0000_1ffe),
    );
}

#[test]
fn byte_sixteen_and_register_mode_far_calls_are_rejected_before_data_access() {
    length_and_register_modes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_call_return_fetch_and_invalid_forms() {
    missing_fields(Engine::V8);
    complete_fields(Engine::V8);
    length_and_register_modes(Engine::V8);
}
