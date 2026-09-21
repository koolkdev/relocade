use crate::support::encoding::check_length;
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ebx},
};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
    },
    machine::{check, Exit, Step},
    step::{Engine, TestModule},
};

use super::image;

#[test]
fn snapshot_forms_require_their_encoding_but_no_successor() {
    for code in [
        &[0x0f, 0xc0, 0xe0][..],
        &[0x66, 0x0f, 0xc1, 0xd8][..],
        &[0x0f, 0xb0, 0xe3][..],
        &[0x66, 0x0f, 0xb1, 0x44, 0x8b, 0x80][..],
        &[0x0f, 0xc1, 0x04, 0x25, 0x20, 0x40, 0, 0][..],
    ] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn maximum_length() -> Vec<Case> {
    vec![
        Case::new("fifteen-byte XADD AX,AX", &[vec![0x66; 12], vec![0x0f, 0xc1, 0xc0]].concat(), Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .at(0x1ff1).register(Eax, 0x4433_2211, 0x4433_4422),
        Case::new("fifteen-byte CMPXCHG AX,BX", &[vec![0x66; 12], vec![0x0f, 0xb1, 0xd8]].concat(), Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .at(0x1ff1).register(Eax, 0x4433_2211, 0x4433_eedd).initial_register(Ebx, 0x10ff_eedd),
    ]
}

test_cases!(fifteen_byte_exchanges, maximum_length());

#[test]
fn length_limit_precedes_fetching_an_unavailable_field() {
    for (prefixes, suffix) in [
        (14, &[0x0f][..]),
        (13, &[0x0f, 0xc0][..]),
        (12, &[0x0f, 0xb1, 0x04][..]),
        (11, &[0x0f, 0xc1, 0x44, 0x8b][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        let mut image = image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "a required exchange field would exceed fifteen bytes",
            Exit::Other(0x0002_0000_0000_0000),
        );
    }
}

#[test]
fn missing_encoding_bytes_fault_before_operand_effects() {
    for code in [
        &[0x0f][..],
        &[0x0f, 0xc0][..],
        &[0x66, 0x0f, 0xb1, 0x04][..],
        &[0x0f, 0xc1, 0x44, 0x8b][..],
        &[0x0f, 0xb1, 0x05, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "required exchange encoding byte is unmapped",
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn completed_addition_survives_a_later_modrm_fetch_fault() {
    let mut image = image(&[]);
    image.cpu.eip = 0x1ffb;
    image.data(0x3ffb, &[0x0f, 0xc0, 0xe0, 0x0f, 0xb0]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_1133;
    cpu.flags.status_source.kind = 2;
    cpu.flags.status_source.left = 0x11;
    cpu.flags.status_source.right = 0x22;
    cpu.eip = 0x1ffe;
    cpu.instruction_count = 0;
    check(
        TestModule::interpreter(),
        "CMPXCHG cannot fetch its ModRM after a completed XADD",
        &image,
        &[
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            },
        ],
    );
}
