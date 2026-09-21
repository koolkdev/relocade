//! REP decoding boundaries without repeating the operand-width matrix.
use super::record;
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Ecx, Edi, Esi},
};
use wasmparser::Validator;

fn prefix_runs() -> Vec<Vec<u8>> {
    vec![
        vec![0xf3],
        vec![0xf3; 14],
        vec![0xf3; 15],
        (0..14)
            .map(|index| if index % 2 == 0 { 0x66 } else { 0xf3 })
            .collect(),
        (0..15)
            .map(|index| if index % 2 == 0 { 0xf3 } else { 0x66 })
            .collect(),
    ]
}

fn fetch_precedence(engine: Engine) {
    for prefixes in prefix_runs() {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x2000 - prefixes.len() as u32;
        image.cpu.registers.ecx = 0;
        image.cpu.registers.esi = 0x9000;
        image.cpu.registers.edi = 0xa000;
        image.data(0x4000 - prefixes.len() as u32, &prefixes);
        let exit = if prefixes.len() == 15 {
            Exit::Other(0x0002_0000_0000_0000)
        } else {
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            }
        };
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("prefix-only REP length {}", prefixes.len()),
            exit,
        );
    }
    for opcode in [0x90, 0x0f] {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffe;
        image.cpu.registers.ecx = 0;
        image.cpu.registers.esi = 0x9000;
        image.cpu.registers.edi = 0xa000;
        image.data(0x3ffe, &[0xf3, opcode]);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("unsupported F3 {opcode:02x} must not fetch the next page"),
            Exit::Other(0x0008_00f3_0000_1ffe),
        );
    }
}

#[test]
fn snapshot_prefix_truncation_and_byte_sixteen_precedence() {
    for prefixes in prefix_runs() {
        let code = [prefixes.clone(), vec![0xa4]].concat();
        for available in 0..code.len() {
            let expected = if available >= 15 {
                BlockError::InstructionTooLong { address: 0x1000 }
            } else {
                BlockError::TruncatedInstruction {
                    address: 0x1000,
                    available,
                }
            };
            assert_eq!(
                compile_block_from_bytes(0x1000, &code[..available], 1).err(),
                Some(expected)
            );
        }
        if prefixes.len() == 15 {
            assert_eq!(
                compile_block_from_bytes(0x1000, &code, 1).err(),
                Some(BlockError::InstructionTooLong { address: 0x1000 })
            );
        } else {
            let module = compile_block_from_bytes(0x1000, &code, 1).unwrap();
            Validator::new().validate_all(&module.bytes).unwrap();
            let next_eip = 0x1000 + code.len() as u32;
            let with_successor = [code, vec![0x0f]].concat();
            assert_eq!(
                module.bytes,
                compile_block_from_bytes(0x1000, &with_successor, 1)
                    .unwrap()
                    .bytes
            );
            assert_eq!(
                compile_block_from_bytes(0x1000, &with_successor, 2).err(),
                Some(BlockError::TruncatedInstruction {
                    address: next_eip,
                    available: 1
                })
            );
        }
    }
    for code in [[0xf3, 0x90], [0xf3, 0x0f]] {
        assert_eq!(
            compile_block_from_bytes(0x1000, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode: 0xf3
            })
        );
    }
}

fn completed_instruction_boundaries() -> Vec<Case> {
    let code = [vec![0x66; 13], vec![0xf3, 0xa5]].concat();
    [0x2000 - 15, u32::MAX]
        .into_iter()
        .map(|origin| {
            Case::preserving_flags(
                format!("fifteen-byte REP MOVSW ends at code boundary from {origin:08x}"),
                &code,
            )
            .at(origin)
            .stored_flags(record(0xfe))
            .register(Ecx, 1, 0)
            .register(Esi, 0x4000, 0x4002)
            .register(Edi, 0x6000, 0x6002)
            .memory(0x4000, &[0x12, 0x34], ReadOnly)
            .memory(0x6000, &[0xa5; 2], ReadWrite)
            .expect_memory(0x6000, &[0x12, 0x34])
        })
        .collect()
}

fn reset_image() -> Image {
    let mut image = Image::new(&[0x66, 0xf3, 0xab, 0xab]);
    image.cpu.flags = record(0xfe);
    image.cpu.instruction_count = 17;
    image.cpu.registers.eax = 0x7856_3412;
    image.cpu.registers.ecx = 2;
    image.cpu.registers.edi = 0x6000;
    image.map(6, 0x8000, true);
    image.data(0x8000, &[0xa5; 8]);
    image
}

fn per_instruction_reset(engine: Engine) {
    let image = reset_image();
    let mut first = image.cpu;
    first.registers.ecx = 0;
    first.registers.edi = 0x6004;
    first.eip = 0x1003;
    first.instruction_count = 18;
    let mut second = first;
    second.registers.edi = 0x6008;
    second.eip = 0x1004;
    second.instruction_count = 19;
    let first_bytes: &[u8] = &[0x12, 0x34, 0x12, 0x34];
    let second_bytes: &[u8] = &[0x12, 0x34, 0x56, 0x78];
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 2),
        expected(
            &image,
            &[
                Step {
                    cpu: first,
                    ram: &[(0x8000, first_bytes)],
                    exit: Exit::Dispatch(0x1003)
                },
                Step {
                    cpu: second,
                    ram: &[(0x8004, second_bytes)],
                    exit: Exit::Dispatch(0x1004)
                },
            ]
        )
    );
    let block =
        TestModule::new(&compile_block_from_bytes(0x1000, &[0x66, 0xf3, 0xab, 0xab], 2).unwrap());
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: second,
                ram: &[(0x8000, first_bytes), (0x8004, second_bytes)],
                exit: Exit::Dispatch(0x1004)
            },]
        )
    );
}

#[test]
fn runtime_prefix_fetch_faults_and_reset() {
    fetch_precedence(Engine::Wasmtime);
    per_instruction_reset(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn runtime_prefix_fetch_faults_and_reset_v8() {
    fetch_precedence(Engine::V8);
    per_instruction_reset(Engine::V8);
}

test_cases!(
    rep_fifteenth_byte_and_eip_wrap,
    completed_instruction_boundaries()
);
