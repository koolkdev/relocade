use crate::support::encoding::check_length;
use wasm86_x86::{compile_block_from_bytes, BlockError, CpuState, Gpr32::Eax, StoredFlags};
use wasm86_x86::{FlagBytes, StoredStatusSource};
use wasmparser::Validator;

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Preserved, Set},
    Flags, InstructionCase as Case,
    Permissions::ReadOnly,
};
use crate::support::machine;
use crate::support::step;
use machine::{check, Exit, Image, Step};
use step::TestModule;

#[test]
fn binary_lengths_follow_the_selected_operand_and_immediate_widths() {
    for code in [
        &[0x00, 0xd8][..],
        &[0x01, 0xd8],
        &[0x02, 0xc3],
        &[0x03, 0xc3],
        &[0x04, 0x80],
        &[0x05, 0x80, 0x81, 0x83, 0x66],
        &[0x38, 0xd8],
        &[0x39, 0xd8],
        &[0x3a, 0xc3],
        &[0x3b, 0xc3],
        &[0x3c, 0x80],
        &[0x3d, 0x80, 0x81, 0x83, 0x66],
        &[0x80, 0xc0, 0x80],
        &[0x80, 0xf8, 0xff],
        &[0x81, 0xc0, 0x80, 0x81, 0x83, 0x66],
        &[0x81, 0xf8, 0x80, 0x81, 0x83, 0x66],
        &[0x83, 0xc0, 0xff],
        &[0x83, 0xf8, 0x80],
        &[0x66, 0x05, 0x80, 0x81],
        &[0x66, 0x3d, 0x80, 0x81],
        &[0x66, 0x81, 0xc0, 0x80, 0x81],
        &[0x66, 0x81, 0xf8, 0x80, 0x81],
        &[0x66, 0x83, 0xc0, 0xff],
        &[0x66, 0x83, 0xf8, 0x80],
        &[0x66, 0x80, 0xc0, 0x80],
        &[0x66, 0x0f, 0x9f, 0xc4],
        &[0x81, 0x84, 0x8b, 0, 0x40, 0, 0, 0x80, 0x81, 0x83, 0x66],
        &[0x0f, 0x94, 0x84, 0x8b, 0, 0x40, 0, 0],
    ] {
        check_length(code);
    }
}

#[test]
fn binary_encodings_use_the_selected_immediate_width() {
    for base in [0x08, 0x10, 0x18, 0x20, 0x28, 0x30] {
        for code in [
            vec![base, 0xd8],
            vec![base + 1, 0xd8],
            vec![base + 2, 0xc3],
            vec![base + 3, 0xc3],
            vec![base + 4, 0x80],
            vec![base + 5, 0x80, 0x81, 0x83, 0x66],
            vec![0x66, base + 1, 0xd8],
            vec![0x66, base + 3, 0xc3],
            vec![0x66, base + 5, 0x80, 0x81],
            vec![0x66, base + 4, 0x80],
        ] {
            check_length(&code);
        }
    }
    for modrm in [0xc8, 0xd0, 0xd8, 0xe0, 0xe8, 0xf0] {
        for code in [
            vec![0x80, modrm, 0x80],
            vec![0x81, modrm, 0x80, 0x81, 0x83, 0x66],
            vec![0x83, modrm, 0xff],
            vec![0x66, 0x81, modrm, 0x80, 0x81],
            vec![0x66, 0x83, modrm, 0xff],
        ] {
            check_length(&code);
        }
    }
    for code in [
        &[0x84, 0xd8][..],
        &[0x85, 0xd8],
        &[0x66, 0x85, 0xd8],
        &[0xa8, 0xff],
        &[0xa9, 0x80, 0x81, 0x83, 0x66],
        &[0x66, 0xa9, 0x80, 0x81],
        &[0xf6, 0xc0, 0x80],
        &[0xf7, 0xc0, 0x80, 0x81, 0x83, 0x66],
        &[0x66, 0xf7, 0xc0, 0x80, 0x81],
        &[0xf7, 0x84, 0x8b, 0, 0x40, 0, 0, 0x80, 0x81, 0x83, 0x66],
        &[0x66, 0x81, 0xa4, 0x8b, 0, 0x40, 0, 0, 0x80, 0x81],
    ] {
        check_length(code);
    }
}

#[test]
fn setcc_ignores_modrm_reg_without_changing_its_destination() {
    for condition in 0..16 {
        let base = compile_block_from_bytes(0x1000, &[0x0f, 0x90 + condition, 0xc4], 1).unwrap();
        Validator::new().validate_all(&base.bytes).unwrap();
        let ignored = (condition & 7) << 3;
        let equivalent =
            compile_block_from_bytes(0x1000, &[0x0f, 0x90 + condition, 0xc4 | ignored], 1).unwrap();
        assert_eq!(base.bytes, equivalent.bytes, "condition {condition}");
    }
}

#[test]
fn unsupported_extensions_stop_before_address_and_immediate_fields() {
    for (code, opcode) in [
        (&[0xf6, 0x0c][..], 0xf6),
        (&[0xf7, 0x0c][..], 0xf7),
        (&[0x66, 0xf7, 0x0d][..], 0xf7),
        (&[0xf6, 0x0d][..], 0xf6),
        (&[0x0f, 0x28][..], 0x0f), // MOVAPS is outside the integer subset.
        (&[0x66, 0x0f, 0x28][..], 0x0f), // MOVAPD
    ] {
        assert_eq!(
            compile_block_from_bytes(0x1000, code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode
            })
        );
    }
    for (prefixes, suffix) in [
        (14, &[0x0f][..]),
        (13, &[0x0f, 0x94][..]),
        (12, &[0x0f, 0x94, 0x04][..]),
        (12, &[0x81, 0xc0, 1][..]),
        (12, &[0xf7, 0xc0, 1][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert_eq!(code.len(), 15);
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 })
        );
    }
}

#[test]
fn binary_fetch_faults_follow_decode_precedence() {
    let step = TestModule::interpreter();
    for (name, start, code, fault) in [
        (
            "missing second opcode",
            0x1fff,
            vec![0x0f],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "missing SETcc ModRM",
            0x1ffe,
            vec![0x0f, 0x94],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "missing SETcc SIB",
            0x1ffd,
            vec![0x0f, 0x94, 0x04],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "missing SETcc displacement before data denial",
            0x1ffa,
            vec![0x0f, 0x94, 0x05, 0, 0x40, 0],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "missing ADD immediate before data denial",
            0x1ff7,
            vec![0x81, 0x04, 0x25, 0, 0x40, 0, 0, 1, 0],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "missing byte TEST immediate before data denial",
            0x1ff9,
            vec![0xf6, 0x04, 0x25, 0, 0x40, 0, 0],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "missing dword TEST immediate before data denial",
            0x1ff7,
            vec![0xf7, 0x04, 0x25, 0, 0x40, 0, 0, 0xff, 0xff],
            Exit::PageFault {
                address: 0x00002000,
                error: 0x10,
            },
        ),
        (
            "unsupported F6 extension before SIB",
            0x1ffe,
            vec![0xf6, 0x0c],
            Exit::Other(0x0008_00f6_0000_1ffe),
        ),
        (
            "unsupported F7 extension before displacement",
            0x1ffe,
            vec![0xf7, 0x0d],
            Exit::Other(0x0008_00f7_0000_1ffe),
        ),
        (
            "unsupported F7 extension before SIB",
            0x1ffe,
            vec![0xf7, 0x0c],
            Exit::Other(0x0008_00f7_0000_1ffe),
        ),
        (
            "unsupported MOVAPS opcode before ModRM",
            0x1ffe,
            vec![0x0f, 0x28],
            Exit::Other(0x0008_000f_0000_1ffe),
        ),
        (
            "second opcode beyond length limit",
            0x1ff1,
            [vec![0x66; 14], vec![0x0f]].concat(),
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "SETcc ModRM beyond length limit",
            0x1ff1,
            [vec![0x66; 13], vec![0x0f, 0x94]].concat(),
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "SETcc SIB beyond length limit",
            0x1ff1,
            [vec![0x66; 12], vec![0x0f, 0x94, 0x04]].concat(),
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "ADD immediate beyond length limit",
            0x1ff1,
            [vec![0x66; 12], vec![0x81, 0xc0, 1]].concat(),
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "last-byte unsupported group avoids a length fault",
            0x1ff1,
            [vec![0x66; 13], vec![0xf7, 0x0c]].concat(),
            Exit::Other(0x0008_00f7_0000_1ff1),
        ),
        (
            "TEST immediate beyond length limit",
            0x1ff1,
            [vec![0x66; 12], vec![0xf7, 0xc0, 1]].concat(),
            Exit::Other(0x0002_0000_0000_0000),
        ),
        (
            "last-byte F6 extension rejection avoids a length fault",
            0x1ff1,
            [vec![0x66; 13], vec![0xf6, 0x0c]].concat(),
            Exit::Other(0x0008_00f6_0000_1ff1),
        ),
        (
            "last-byte MOVAPD opcode rejection avoids a ModRM length fault",
            0x1ff1,
            [vec![0x66; 13], vec![0x0f, 0x28]].concat(),
            Exit::Other(0x0008_000f_0000_1ff1),
        ),
    ] {
        let mut image = Image::new(&[]);
        image.cpu.flags.status_source.kind = 0;
        image.cpu.eip = start;
        image.cpu.registers.ebx = 0x4000;
        image.data(0x3000 + (start & 0xfff), &code);
        let expected_cpu = image.cpu;
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: fault,
            }],
        );
    }
}

#[rustfmt::skip]
fn code_page_boundaries() -> Vec<Case> {
    vec![
        Case::new("extended opcode spans scattered code pages", &[0x0f, 0x94, 0xc4], Flags::all(true), Flags::all(Preserved))
            .stored_flags(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 0,
                    ..(CpuState::filled(0xa5).flags).status_source
                },
                bytes: FlagBytes {
                    zf: 1,
                    ..(CpuState::filled(0xa5).flags).bytes
                },
            }).preserve_flag_record()
            .at(0x1fff).map_page(1, 0x3000, ReadOnly).map_page(2, 0xa000, ReadOnly)
            .register(Eax, 0x4433_2211, 0x4433_0111),
        Case::new("ADD immediate wraps instruction addresses", &[0x05, 1, 0, 0, 0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .at(0xffff_fffd).map_page(0xfffff, 0x8000, ReadOnly).map_page(0, 0xa000, ReadOnly)
            .register(Eax, 0xffff_ffff, 0),
    ]
}

test_cases!(scattered_and_wrapping_code, code_page_boundaries());
