use crate::support::encoding::check_length;
use wasm86_x86::{compile_block_from_bytes, BlockError, CpuState, Gpr32::Eax, StoredFlags};
use wasm86_x86::{FlagBytes, StoredStatusSource};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set, Undefined},
        Flags, InstructionCase as Case,
    },
    machine::{check, Exit, Image, Step},
    sequences::{test_sequences, Checkpoint, SequenceCase},
    step::TestModule,
};

#[test]
fn unary_lengths_stop_after_the_selected_register_or_address() {
    for code in [
        &[0x40][..],
        &[0x4f],
        &[0x66, 0x43],
        &[0x66, 0x66, 0x4c],
        &[0xfe, 0xc4],
        &[0x66, 0xfe, 0xcc],
        &[0xff, 0xc0],
        &[0x66, 0xff, 0xc8],
        &[0xf6, 0xd0],
        &[0x66, 0xf6, 0xdc],
        &[0xf7, 0xd0],
        &[0x66, 0xf7, 0xd8],
        &[0xfe, 0x44, 0x8b, 0x80],
        &[0xff, 0x84, 0x8b, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xff, 0x0d, 0x11, 0x22, 0x33, 0x44],
        &[0xf6, 0x54, 0x8b, 0x80],
        &[0xf7, 0x14, 0x25, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xf7, 0x9c, 0x8b, 0x11, 0x22, 0x33, 0x44],
        // The same F6/F7 opcodes still consume an immediate for TEST /0.
        &[0xf6, 0xc0, 0xf7],
        &[0xf7, 0xc0, 0xf6, 0xf7, 0xfe, 0xff],
        &[0x66, 0xf7, 0xc0, 0xf6, 0xf7],
    ] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn mixed_group_fields() -> Vec<SequenceCase> {
    vec![SequenceCase::from_opaque_flags("mixed F6/F7 forms consume their own fields")
        .stored_flags(StoredFlags {
            status_source: StoredStatusSource {
                kind: 0xff,
                ..(CpuState::filled(0xa5).flags).status_source
            },
            ..CpuState::filled(0xa5).flags
        })
        .initial_register(Eax, 0x1234_5678)
        .step(Checkpoint::new(&[0xf6, 0xc0, 0xf7],
            Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Clear, of: Clear }))
        .step(Checkpoint::preserving_flags(&[0xf6, 0xd0]).register(Eax, 0x1234_5687))
        .step(Checkpoint::new(&[0xf6, 0xd8],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x1234_5679))
        .step(Checkpoint::new(&[0xf7, 0xc0, 0xf6, 0xf7, 0xfe, 0xff],
            Flags { cf: Clear, pf: Clear, af: Undefined, zf: Clear, sf: Clear, of: Clear }))
        .step(Checkpoint::preserving_flags(&[0xf7, 0xd0]).register(Eax, 0xedcb_a986))
        .step(Checkpoint::new(&[0xf7, 0xd8],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x1234_567a))]
}

test_sequences!(mixed_group_opcode_fields, mixed_group_fields());

#[test]
fn unsupported_group_extensions_stop_before_sib_or_displacement_fetch() {
    for (opcode, modrm) in [(0xfe, 0x14), (0xff, 0x3c), (0xf6, 0x0c), (0xf7, 0x0d)] {
        for prefixes in [0, 13] {
            let code = [vec![0x66; prefixes], vec![opcode, modrm]].concat();
            let start = 0x2000 - code.len() as u32;
            assert_eq!(
                compile_block_from_bytes(start, &code, 1).err(),
                Some(BlockError::UnsupportedInstruction {
                    address: start,
                    opcode
                }),
            );
            let mut image = Image::new(&[]);
            image.cpu.flags.status_source.kind = 0xff;
            image.cpu.eip = start;
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                TestModule::interpreter(),
                "unsupported extension precedes address fields",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(
                        0x0008_0000_0000_0000 | (u64::from(opcode) << 32) | u64::from(start),
                    ),
                }],
            );
        }
    }
}

#[test]
fn required_unary_fields_fault_before_data_access() {
    for code in [
        &[0xfe][..],
        &[0xff, 0x04],
        &[0xf6, 0x14],
        &[0x66, 0xf7, 0x9c, 0x25, 0, 0x40, 0],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = Image::new(&[]);
        image.cpu.flags.status_source.kind = 0;
        image.cpu.registers.ebx = 0x4000;
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "required unary field crosses an unmapped code page",
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
    for suffix in [
        &[0xfe][..],
        &[0xff, 0x04],
        &[0xf7, 0x94, 0x25],
        &[0xf7, 0xc0, 1],
    ] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 }),
        );
        let mut image = Image::new(&[]);
        image.cpu.flags.status_source.kind = 0;
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "byte sixteen is rejected before page fetch",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0002_0000_0000_0000),
            }],
        );
    }
}

#[rustfmt::skip]
fn maximum_length() -> Vec<Case> {
    let mut cases = Vec::new();
    for (suffix, output) in [(&[0x40][..], 0x1234_0000), (&[0xfe, 0xc0][..], 0x1234_ff00)] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        cases.push(Case::new(format!("fifteen-byte INC {suffix:02x?}"), &code, Flags::all(true),
            Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .at(0x1ff1).register(Eax, 0x1234_ffff, output));
    }
    let invalid = StoredFlags {
        status_source: StoredStatusSource {
            kind: 0xff,
            ..(CpuState::filled(0xa5).flags).status_source
        },
        bytes: FlagBytes {
            cf: 1,
            ..(CpuState::filled(0xa5).flags).bytes
        },
    };
    cases.push(Case::preserving_flags("fifteen-byte NOT AL has no immediate",
        &[vec![0x66; 13], vec![0xf6, 0xd0]].concat())
        .stored_flags(invalid).at(0x1ff1).register(Eax, 0x1234_ffff, 0x1234_ff00));
    cases.push(Case::replacing_flags("fifteen-byte NEG AX has no immediate",
        &[vec![0x66; 13], vec![0xf7, 0xd8]].concat(),
        Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
        .stored_flags(invalid).at(0x1ff1).register(Eax, 0x1234_ffff, 0x1234_0001));
    cases
}

test_cases!(fifteen_byte_encodings, maximum_length());
