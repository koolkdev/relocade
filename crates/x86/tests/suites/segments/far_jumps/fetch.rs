use super::*;
use wasm86_x86::{compile_block_from_bytes, BlockError};

fn complete_fields(engine: Engine) {
    for word in [false, true] {
        let code = [vec![0x67], immediate(word, 0x5678, 0x27)].concat();
        for split in 1..=code.len() {
            let mut image = Image::empty();
            image.cpu.eip = 0x2000 - split as u32;
            image.cpu.segments.cs.limit = image.cpu.eip + code.len() as u32 - 1;
            image.map(1, 0x3000, false);
            image.data(0x4000 - split as u32, &code[..split]);
            if split < code.len() {
                image.map(2, 0x8000, false);
                image.data(0x8000, &code[split..]);
            }
            let mut tables = DescriptorTables::default();
            tables.insert(0x27, descriptor(0x9000, 0xffff, SegmentDefaultSize::Bits32));
            let mut cpu = image.cpu;
            cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, 23);
            cpu.eip = 0x5678;
            cpu.instruction_count = 0;
            check_one(
                engine,
                SegmentProfile::Segmented32,
                &code,
                &image,
                &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn both_immediate_fields_cross_code_pages_and_ignore_the_address_size_prefix() {
    complete_fields(Engine::Wasmtime);
}

fn missing_fields(engine: Engine) {
    for word in [false, true] {
        let code = immediate(word, 0x9234_5678, 0xf327);
        for available in 1..code.len() {
            let start = 0x2000 - available as u32;
            assert_eq!(
                compile_block_from_bytes(start, &code[..available], 1).err(),
                Some(BlockError::TruncatedInstruction {
                    address: start,
                    available
                })
            );
            for limit in [0x1fff, 0x2000] {
                let mut image = Image::empty();
                image.cpu.eip = start;
                image.cpu.segments.cs.limit = limit;
                image.map(1, 0x3000, false);
                image.data(0x4000 - available as u32, &code[..available]);
                // An earlier absent byte wins over a later CS violation; at the
                // same byte, CS wins. Neither case reaches selector resolution.
                let exit = if limit == 0x1fff {
                    Exit::GeneralProtection { error: 0 }
                } else {
                    Exit::PageFault {
                        address: 0x2000,
                        error: 0x10,
                    }
                };
                image.check_unchanged_exit(
                    engine,
                    TestModule::interpreter_with_profile(SegmentProfile::Segmented32),
                    &format!(
                        "far JMP fields {code:02x?}, available {available}, CS limit {limit:x}"
                    ),
                    exit,
                );
            }
        }
    }
}

#[test]
fn partial_offsets_and_selectors_keep_instruction_fetch_faults_in_byte_order() {
    missing_fields(Engine::Wasmtime);
}

fn length_boundary(engine: Engine) {
    for word in [false, true] {
        let suffix = immediate(word, 0x5678, 0x27);
        for length in [15, 16] {
            let code = [vec![0x67; length - suffix.len()], suffix.clone()].concat();
            let mut image = Image::empty();
            image.cpu.eip = 0x1ff1;
            image.map(1, 0x3000, false);
            image.data(0x3ff1, &code[..15]);
            if length == 16 {
                assert_eq!(
                    compile_block_from_bytes(0x1ff1, &code[..15], 1).err(),
                    Some(BlockError::InstructionTooLong { address: 0x1ff1 })
                );
                image.check_unchanged_exit(
                    engine,
                    TestModule::interpreter(),
                    &format!("far JMP exceeds fifteen bytes, word={word}"),
                    Exit::GeneralProtection { error: 0 },
                );
            } else {
                let mut tables = DescriptorTables::default();
                tables.insert(0x27, descriptor(0x9000, 0xffff, SegmentDefaultSize::Bits32));
                let mut cpu = image.cpu;
                cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, 23);
                cpu.eip = 0x5678;
                cpu.instruction_count = 0;
                check_one(
                    engine,
                    SegmentProfile::Flat32,
                    &code,
                    &image,
                    &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip),
                    },
                );
            }
        }
    }
}

#[test]
fn a_fifteen_byte_jump_completes_without_fetching_a_sixteenth_byte() {
    length_boundary(Engine::Wasmtime);
}

fn register_modes(engine: Engine) {
    for rm in 0..8 {
        let code = [0xff, 0xe8 | rm];
        let mut image = Image::empty();
        image.cpu.eip = 0x1ffe;
        image.map(1, 0x3000, false);
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
            &format!("far JMP rejects register {rm}"),
            Exit::Other(0x0008_00ff_0000_1ffe),
        );
    }
}

#[test]
fn ff_group_five_rejects_every_register_mode_before_source_access() {
    register_modes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_jump_instruction_fetch_and_invalid_forms() {
    complete_fields(Engine::V8);
    missing_fields(Engine::V8);
    length_boundary(Engine::V8);
    register_modes(Engine::V8);
}
