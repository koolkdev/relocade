//! LOCK selects eligible memory updates and preserves required fetch boundaries.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadWrite,
    },
    machine::{Exit, Image},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32::*};

fn memory_updates() -> Vec<Case> {
    let clear = Flags::all(Clear);
    let preserve = Flags::all(Preserved);
    let mut cases = Vec::new();
    for (name, code, input, output, carry, flags) in [
        (
            "ADD",
            &[0xf0, 0x01, 0x03][..],
            0x10u32,
            0x13u32,
            false,
            clear,
        ),
        (
            "ADC",
            &[0xf0, 0x11, 0x03],
            0x10,
            0x14,
            true,
            Flags { pf: Set, ..clear },
        ),
        (
            "SUB",
            &[0xf0, 0x29, 0x03],
            0x10,
            0x0d,
            false,
            Flags { af: Set, ..clear },
        ),
        (
            "SBB",
            &[0xf0, 0x19, 0x03],
            0x10,
            0x0c,
            true,
            Flags {
                af: Set,
                pf: Set,
                ..clear
            },
        ),
        (
            "AND",
            &[0xf0, 0x21, 0x03],
            0x10,
            0,
            false,
            Flags {
                pf: Set,
                zf: Set,
                ..clear
            },
        ),
        ("OR", &[0xf0, 0x09, 0x03], 0x10, 0x13, false, clear),
        ("XOR", &[0xf0, 0x31, 0x03], 0x10, 0x13, false, clear),
        (
            "INC",
            &[0xf0, 0xff, 0x03],
            0x10,
            0x11,
            true,
            Flags {
                cf: Set,
                pf: Set,
                ..clear
            },
        ),
        (
            "DEC",
            &[0xf0, 0xff, 0x0b],
            0x10,
            0x0f,
            true,
            Flags {
                cf: Set,
                pf: Set,
                af: Set,
                ..clear
            },
        ),
        (
            "NOT",
            &[0xf0, 0xf7, 0x13],
            0x10,
            0xffff_ffef,
            true,
            preserve,
        ),
        (
            "NEG",
            &[0xf0, 0xf7, 0x1b],
            0x10,
            0xffff_fff0,
            false,
            Flags {
                cf: Set,
                pf: Set,
                sf: Set,
                ..clear
            },
        ),
        (
            "BTS",
            &[0xf0, 0x0f, 0xab, 0x03],
            0x10,
            0x18,
            false,
            Flags {
                cf: Clear,
                ..preserve
            },
        ),
        (
            "BTR",
            &[0xf0, 0x0f, 0xb3, 0x03],
            0x18,
            0x10,
            false,
            Flags {
                cf: Set,
                ..preserve
            },
        ),
        (
            "BTC",
            &[0xf0, 0x0f, 0xbb, 0x03],
            0x18,
            0x10,
            false,
            Flags {
                cf: Set,
                ..preserve
            },
        ),
    ] {
        cases.push(
            Case::new(
                format!("LOCK {name} uses its ordinary memory semantics"),
                code,
                Flags {
                    cf: carry,
                    ..Flags::all(false)
                },
                flags,
            )
            .initial_registers(&[(Eax, 3), (Ebx, 0x4000)])
            .memory(0x4000, &input.to_le_bytes(), ReadWrite)
            .expect_memory(0x4000, &output.to_le_bytes()),
        );
    }
    cases.extend([
        Case::preserving_flags("explicit LOCK XCHG", &[0xf0, 0x87, 0x03])
            .register(Eax, 3, 0x10)
            .initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0x10, 0, 0, 0], ReadWrite)
            .expect_memory(0x4000, &[3, 0, 0, 0]),
        Case::replacing_flags("LOCK XADD", &[0xf0, 0x0f, 0xc1, 0x03], clear)
            .register(Eax, 3, 0x10)
            .initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0x10, 0, 0, 0], ReadWrite)
            .expect_memory(0x4000, &[0x13, 0, 0, 0]),
        Case::replacing_flags(
            "LOCK CMPXCHG",
            &[0xf0, 0x0f, 0xb1, 0x0b],
            Flags {
                pf: Set,
                zf: Set,
                ..clear
            },
        )
        .initial_registers(&[(Eax, 0x10), (Ecx, 3), (Ebx, 0x4000)])
        .memory(0x4000, &[0x10, 0, 0, 0], ReadWrite)
        .expect_memory(0x4000, &[3, 0, 0, 0]),
        Case::replacing_flags(
            "LOCK word ADD immediate",
            &[0x66, 0xf0, 0x83, 0x03, 3],
            clear,
        )
        .initial_register(Ebx, 0x4000)
        .memory(0x4000, &[0x10, 0, 0x55, 0xaa], ReadWrite)
        .expect_memory(0x4000, &[0x13, 0]),
        Case::replacing_flags(
            "last group-1 prefix selects a byte LOCK form",
            &[0xf3, 0xf0, 0x00, 0x23],
            clear,
        )
        .initial_registers(&[(Eax, 0x300), (Ebx, 0x4000)])
        .memory(0x4000, &[0x10, 0x55], ReadWrite)
        .expect_memory(0x4000, &[0x13]),
        Case::preserving_flags("last F3 selects PAUSE after F0", &[0xf0, 0xf3, 0x90]),
    ]);
    cases
}

test_cases!(eligible_memory_updates, memory_updates());

fn rejected_forms(engine: Engine) {
    for code in [
        &[0xf0, 0x90][..],
        &[0xf0, 0x8b],
        &[0xf0, 0x03],
        &[0xf0, 0x01, 0xc0],
        &[0xf0, 0x80, 0xf8],
        &[0xf0, 0xc1],
        &[0xf0, 0x0f, 0xa3],
        &[0xf0, 0x0f, 0xc7, 0xc8],
        &[0xf0, 0x0f, 0xc7, 0x04],
        &[0xf0, 0x0f, 0xba, 0x24],
        &[0x0f, 0xc7, 0xc8],
    ] {
        let origin = 0x2000 - code.len() as u32;
        let diagnostic = code[0];
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: origin,
                opcode: diagnostic
            })
        );
        let mut image = Image::empty();
        image.cpu.eip = origin;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("rejected form stops before unused fields: {code:02x?}"),
            Exit::Other(0x0008_0000_0000_0000 | u64::from(diagnostic) << 32 | u64::from(origin)),
        );
    }
}

fn required_fields(engine: Engine) {
    for code in [
        &[0xf0][..],
        &[0xf0, 0x01],
        &[0xf0, 0x0f],
        &[0xf0, 0x0f, 0xc7],
        &[0xf0, 0x0f, 0xc7, 0x0c],
        &[0xf0, 0x0f, 0xc7, 0x8c, 0x90, 0, 0, 0],
        &[0xf0, 0x83, 0x03],
    ] {
        let origin = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: origin,
                available: code.len()
            })
        );
        let mut image = Image::empty();
        image.cpu.eip = origin;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("LOCK requires its next field: {code:02x?}"),
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
    for suffix in [
        &[0xf0][..],
        &[0xf0, 0x01],
        &[0xf0, 0x0f],
        &[0xf0, 0x0f, 0xc7, 0x0c],
    ] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 })
        );
        let mut image = Image::empty();
        image.cpu.eip = 0x1ff1;
        image.map(1, 0x3000, false);
        image.data(0x3ff1, &code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("LOCK field exceeds fifteen bytes: {suffix:02x?}"),
            Exit::GeneralProtection { error: 0 },
        );
    }
}

#[test]
fn unavailable_lock_forms_reject_before_unused_operand_fields() {
    rejected_forms(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_unavailable_lock_forms_reject_before_unused_operand_fields() {
    rejected_forms(Engine::V8);
}

#[test]
fn lock_prefixes_preserve_required_fetch_and_length_boundaries() {
    required_fields(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_lock_prefixes_preserve_required_fetch_and_length_boundaries() {
    required_fields(Engine::V8);
}
