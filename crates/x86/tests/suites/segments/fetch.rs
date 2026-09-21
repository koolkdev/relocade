use super::data;
use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly},
    machine::{Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ebx},
    Segment, SegmentAttributes, StoredSegment,
};

fn prefix_transport() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [
        &[0x66, 0x8b, 0x03][..],
        &[0x66, 0x0f, 0xb6, 0x03],
        &[0x64, 0x66, 0x8b, 0x03],
        &[0x66, 0x64, 0x8b, 0x03],
        &[0x65, 0x64, 0x66, 0x8b, 0x03],
        &[0x66, 0x64, 0x0f, 0xb6, 0x03],
    ] {
        let value = if code.contains(&0x0f) {
            0xaaaa_0078
        } else {
            0xaaaa_5678
        };
        let address = if code.contains(&0x64) { 0x8020 } else { 0x20 };
        for origin in [0x1000, 0x1fff, 0xffff_fffe] {
            cases.push(Case::preserving_flags(format!("segment prefix state across decoder transitions {code:02x?}, at {origin:08x}"), code)
                .at(origin).segment(Segment::Fs, data(0x8000, 0xff)).segment(Segment::Gs, data(0xc000, 0xff))
                .initial_register(Ebx, 0x20).register(Eax, 0xaaaa_bbbb, value)
                .memory(address, &[0x78, 0x56], ReadOnly).memory(0xc020, &[0xff; 2], ReadOnly));
        }
    }
    let code = [vec![0x64; 12], vec![0x66, 0x8b, 0x03]].concat();
    cases.push(
        Case::preserving_flags(
            "fifteen-byte segment-prefixed instruction needs no next byte",
            &code,
        )
        .at(0x1ff1)
        .segment(Segment::Fs, data(0x8000, 0xff))
        .initial_register(Ebx, 0x20)
        .register(Eax, 0xaaaa_bbbb, 0xaaaa_5678)
        .memory(0x8020, &[0x78, 0x56], ReadOnly),
    );
    cases
}

test_cases!(
    override_presence_survives_operand_size_maps_and_checked_fetch,
    prefix_transport()
);

test_cases!(
    execute_only_cs_permits_instruction_fetch,
    vec![
        Case::preserving_flags("execute-only CS permits NOP", &[0x90])
            .segmented_only()
            .segment(
                Segment::Cs,
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(0x13),
                    ..StoredSegment::flat_code32(0x1b)
                },
            )
    ]
);

fn check_prefix_scope(engine: Engine) {
    use crate::support::machine::expected;
    let code = [0x64, 0x8b, 0x03, 0x8b, 0x0b];
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x20;
    image.cpu.segments.fs = data(0x4000, 0xff);
    image.map(0, 0x8000, false);
    image.map(4, 0xa000, false);
    image.data(0x8020, &[0x22; 4]);
    image.data(0xa020, &[0x11; 4]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1111_1111;
    cpu.eip = 0x1003;
    cpu.instruction_count = 0;
    let first = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1003),
    };
    cpu.registers.ecx = 0x2222_2222;
    cpu.eip = 0x1005;
    cpu.instruction_count = 1;
    let second = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    };
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 2),
        expected(&image, &[first, second])
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x1005)
            }]
        )
    );
}

#[test]
fn segment_overrides_end_with_their_instruction() {
    check_prefix_scope(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_overrides_end_with_their_instruction() {
    check_prefix_scope(Engine::V8);
}

#[test]
fn segment_prefixes_count_toward_fetch_and_length_boundaries() {
    for (code, start, exit) in [
        (
            vec![0x64],
            0x1fff,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            vec![0x64, 0x0f],
            0x1ffe,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            vec![0x64, 0x66, 0x8b],
            0x1ffd,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (vec![0x64; 15], 0x1ff1, Exit::GeneralProtection { error: 0 }),
    ] {
        let compiled = compile_block_from_bytes(start, &code, 1);
        if code.len() == 15 {
            assert!(matches!(
                compiled,
                Err(BlockError::InstructionTooLong { .. })
            ));
        } else {
            assert!(
                matches!(compiled, Err(BlockError::TruncatedInstruction { available, .. }) if available == code.len())
            );
        }
        let mut image = Image::empty();
        image.cpu.eip = start;
        image.map(1, 0x3000, false);
        image.data(0x3000 + (start & 0xfff), &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "prefix fault keeps the instruction start",
            exit,
        );
    }
}
