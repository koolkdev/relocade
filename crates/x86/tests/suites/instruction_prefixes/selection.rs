//! Prefix selection preserves byte widths and rejection-time fetch boundaries.
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    machine::{Exit, Image},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, CpuState, Gpr32};

fn incomplete_prefix_fetches(engine: Engine) {
    for code in [&[0xf3][..], &[0xf3, 0x67], &[0x67, 0xf3], &[0x67]] {
        let origin = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: origin,
                available: code.len(),
            })
        );
        // Prefixes are accepted independently of the opcode still to be fetched.
        let mut image = Image::new(&[]);
        image.cpu.eip = origin;
        image.cpu.registers.ecx = 0;
        image.data(0x3000 + (origin & 0xfff), code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("incomplete prefix order {code:02x?}"),
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn prefix_order_preserves_the_next_required_fetch() {
    incomplete_prefix_fetches(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_prefix_order_preserves_the_next_required_fetch() {
    incomplete_prefix_fetches(Engine::V8);
}

fn unsupported_prefixed_register_exchanges(engine: Engine) {
    // PAUSE occupies only F3 90. Other F2/F3 exchange encodings remain outside
    // the supported set, and neither frontend may fall back to the bare form.
    for (prefix, first_opcode) in [(0xf2, 0x90), (0xf3, 0x91)] {
        for opcode in first_opcode..=0x97 {
            let code = [prefix, opcode];
            assert_eq!(
                compile_block_from_bytes(0x1ffe, &code, 1).err(),
                Some(BlockError::UnsupportedInstruction {
                    address: 0x1ffe,
                    opcode: prefix,
                }),
            );
            let mut image = Image::empty();
            image.cpu.eip = 0x1ffe;
            image.map(1, 0x3000, false);
            image.data(0x3ffe, &code);
            image.check_unchanged_exit(
                engine,
                TestModule::interpreter(),
                &format!("prefix rejection without fetching a successor: {code:02x?}"),
                Exit::Other(match prefix {
                    0xf2 => 0x0008_00f2_0000_1ffe,
                    0xf3 => 0x0008_00f3_0000_1ffe,
                    _ => unreachable!(),
                }),
            );
        }
    }
}

#[test]
fn prefix_selection_does_not_extend_pause_to_other_register_exchanges() {
    unsupported_prefixed_register_exchanges(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_prefix_selection_does_not_extend_pause_to_other_register_exchanges() {
    unsupported_prefixed_register_exchanges(Engine::V8);
}

test_cases!(
    group1_prefix_selects_the_complete_form,
    [
        Case::preserving_flags("bare 90 keeps its ordinary form", &[0x90]),
        Case::preserving_flags("bare 91 still exchanges EAX and ECX", &[0x91])
            .register(Gpr32::Eax, 0x1234_5678, 0xabcd_ef01)
            .register(Gpr32::Ecx, 0xabcd_ef01, 0x1234_5678),
        // Duplicate group-1 prefixes follow the decoder's last-prefix policy.
        Case::preserving_flags("last F3 selects PAUSE after F2", &[0xf2, 0xf3, 0x90])
            .initial_register(Gpr32::Ecx, u32::MAX),
        Case::preserving_flags("an override after F3 keeps PAUSE", &[0xf3, 0x66, 0x90])
            .initial_register(Gpr32::Ecx, u32::MAX),
    ]
);

#[test]
fn f3_extended_map_rejection_respects_the_fifteen_byte_limit() {
    for (prefixes, snapshot_error, runtime_exit) in [
        (
            13,
            BlockError::UnsupportedInstruction {
                address: 0x1ff1,
                opcode: 0xf3,
            },
            Exit::Other(0x0008_00f3_0000_1ff1),
        ),
        (
            14,
            BlockError::InstructionTooLong { address: 0x1ff1 },
            Exit::Other(0x0002_0000_0000_0000),
        ),
    ] {
        let code = [vec![0x66; prefixes], vec![0xf3, 0x0f]].concat();
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
            &format!("F3 escape after {prefixes} operand overrides"),
            runtime_exit,
        );
    }
}

fn repeated_byte_forms() -> Vec<Case> {
    let mut flags = CpuState::filled(0xa5).flags;
    flags.status_source.kind = 0;
    flags.bytes.df = 0xfe;
    let mut cases = Vec::new();
    for prefixes in [
        &[0x66, 0xf3][..],
        &[0xf3, 0x66][..],
        &[0x66, 0xf3, 0x66, 0xf3][..],
        &[0xf3, 0x66, 0xf3, 0x66][..],
    ] {
        for opcode in [0xa4, 0xaa] {
            let code = [prefixes, &[opcode]].concat();
            let mut case = Case::preserving_flags(
                format!("mixed prefixes retain byte REP opcode {opcode:02x}: {prefixes:02x?}"),
                &code,
            )
            .stored_flags(flags)
            .initial_register(Gpr32::Eax, 0x7856_3412)
            .register(Gpr32::Ecx, 2, 0)
            .register(Gpr32::Edi, 0x6000, 0x6002)
            .memory(0x6000, &[0xa5; 4], ReadWrite);
            case = if opcode == 0xa4 {
                case.register(Gpr32::Esi, 0x4000, 0x4002)
                    .memory(0x4000, &[0x34, 0x56], ReadOnly)
                    .expect_memory(0x6000, &[0x34, 0x56])
            } else {
                case.initial_register(Gpr32::Esi, 0x9000)
                    .expect_memory(0x6000, &[0x12, 0x12])
            };
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    mixed_operand_and_repeat_prefixes_keep_byte_forms,
    repeated_byte_forms()
);
