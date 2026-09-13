//! Prefix selection preserves byte widths and rejection-time fetch boundaries.
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    machine::{check, Exit, Image, Step},
    step::TestModule,
};
use wasm86_x86::{compile_block_from_bytes, BlockError, CpuState, Gpr32};

#[test]
fn prefix_order_preserves_the_next_required_fetch() {
    for code in [&[0xf3, 0x67][..], &[0x67, 0xf3][..], &[0x67][..]] {
        let origin = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(origin, code, 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: origin,
                available: code.len(),
            })
        );
        // Both prefixes are accepted; decoding still needs an opcode.
        let mut image = Image::new(&[]);
        image.cpu.eip = origin;
        image.cpu.registers.ecx = 0;
        image.data(0x3000 + (origin & 0xfff), code);
        check(
            TestModule::interpreter(),
            &format!("incomplete prefix order {code:02x?}"),
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            }],
        );
    }
}

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
        check(
            TestModule::interpreter(),
            &format!("F3 escape after {prefixes} operand overrides"),
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: runtime_exit,
            }],
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
