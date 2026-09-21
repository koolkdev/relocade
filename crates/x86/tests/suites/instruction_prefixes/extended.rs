//! Prefix admission determines which extended opcode fields must be fetched.

use crate::support::{
    machine::{Exit, Image},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError};

fn required_extended_fields(engine: Engine) {
    for code in [
        &[0xf3, 0x0f][..],
        &[0xf3, 0x0f, 0xb8],
        &[0xf3, 0x0f, 0xb8, 0x04],
        &[0xf3, 0x0f, 0xb8, 0x84, 0x8b, 0x78, 0x56, 0x34],
    ] {
        let origin = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: origin,
                available: code.len(),
            }),
        );
        let mut image = Image::empty();
        image.cpu.eip = origin;
        image.cpu.registers.ebx = 0x6000;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("required F3 extended field faults before data access: {code:02x?}"),
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

fn rejected_extended_forms(engine: Engine) {
    for (code, diagnostic, exit) in [
        (&[0x0f, 0xb8][..], 0x0f, 0x0008_000f_0000_1ffe),
        (&[0xf2, 0x0f, 0xb8][..], 0xf2, 0x0008_00f2_0000_1ffd),
        (&[0xf3, 0x0f, 0xbc][..], 0xf3, 0x0008_00f3_0000_1ffd),
    ] {
        let origin = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: origin,
                opcode: diagnostic
            }),
        );
        let mut image = Image::empty();
        image.cpu.eip = origin;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("unavailable prefixed form rejects without a ModRM fetch: {code:02x?}"),
            Exit::Other(exit),
        );
    }
}

fn extended_field_length_limit(engine: Engine) {
    for suffix in [
        &[0xf3, 0x0f][..],
        &[0xf3, 0x0f, 0xb8],
        &[0xf3, 0x0f, 0xb8, 0x04],
    ] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 }),
        );
        let mut image = Image::empty();
        image.cpu.eip = 0x1ff1;
        image.map(1, 0x3000, false);
        image.data(0x3ff1, &code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("required extended field exceeds the instruction limit: {suffix:02x?}"),
            Exit::GeneralProtection { error: 0 },
        );
    }
}

#[test]
fn admitted_extended_forms_require_their_opcode_and_operand_fields() {
    required_extended_fields(Engine::Wasmtime);
    extended_field_length_limit(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_admitted_extended_forms_require_their_opcode_and_operand_fields() {
    required_extended_fields(Engine::V8);
    extended_field_length_limit(Engine::V8);
}

#[test]
fn prefix_requirements_reject_unavailable_forms_before_operand_fields() {
    rejected_extended_forms(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_prefix_requirements_reject_unavailable_forms_before_operand_fields() {
    rejected_extended_forms(Engine::V8);
}

#[test]
fn f2_extended_map_rejection_respects_the_fifteen_byte_limit() {
    for (prefixes, snapshot_error, runtime_exit) in [
        (
            13,
            BlockError::UnsupportedInstruction {
                address: 0x1ff1,
                opcode: 0xf2,
            },
            Exit::Other(0x0008_00f2_0000_1ff1),
        ),
        (
            14,
            BlockError::InstructionTooLong { address: 0x1ff1 },
            Exit::Other(0x0002_0000_0000_0000),
        ),
    ] {
        let code = [vec![0x66; prefixes], vec![0xf2, 0x0f]].concat();
        for available in 15..=code.len() {
            assert_eq!(
                compile_block_from_bytes(0x1ff1, &code[..available], 1)
                    .err()
                    .as_ref(),
                Some(&snapshot_error)
            );
        }
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ff1;
        image.cpu.registers.ecx = 0;
        image.data(0x3ff1, &code[..15]);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            &format!("F2 escape after {prefixes} operand overrides"),
            runtime_exit,
        );
    }
}
