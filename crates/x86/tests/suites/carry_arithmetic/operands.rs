use super::{
    code_with_width, image as flag_image,
    step::{Engine, TestModule},
    OPERATIONS, WIDTHS,
};
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    machine::Exit,
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

#[rustfmt::skip]
fn memory_roles() -> Vec<Case> {
    let mut cases = Vec::new();
    for next_frame in [0x9000, 0xa000] {
        for (name, source_code, destination_code, input, output, maximum, zero, result, flags) in [
            ("ADC byte", &[0x12, 0x03][..], &[0x10, 0x03][..], 0x4433_00ff, 0x4433_0000,
                &[0xff][..], &[0][..], &[0][..],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }),
            ("ADC word", &[0x66, 0x13, 0x03][..], &[0x66, 0x11, 0x03][..], 0x4433_ffff, 0x4433_0000,
                &[0xff, 0xff][..], &[0, 0][..], &[0, 0][..],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }),
            ("ADC dword", &[0x13, 0x03][..], &[0x11, 0x03][..], 0xffff_ffff, 0,
                &[0xff, 0xff, 0xff, 0xff][..], &[0, 0, 0, 0][..], &[0, 0, 0, 0][..],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }),
            ("SBB byte", &[0x1a, 0x03][..], &[0x18, 0x03][..], 0x4433_00ff, 0x4433_00fe,
                &[0xff][..], &[0][..], &[0xfe][..],
                Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear }),
            ("SBB word", &[0x66, 0x1b, 0x03][..], &[0x66, 0x19, 0x03][..], 0x4433_ffff, 0x4433_fffe,
                &[0xff, 0xff][..], &[0, 0][..], &[0xfe, 0xff][..],
                Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear }),
            ("SBB dword", &[0x1b, 0x03][..], &[0x19, 0x03][..], 0xffff_ffff, 0xffff_fffe,
                &[0xff, 0xff, 0xff, 0xff][..], &[0, 0, 0, 0][..], &[0xfe, 0xff, 0xff, 0xff][..],
                Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear }),
        ] {
            let source = Case::new(format!("{name} read source, frame {next_frame:#x}"), source_code, Flags::all(true), flags)
                .register(Eax, input, output).initial_register(Ebx, 0x4fff)
                .map_page(4, 0x8000, ReadOnly).map_page(5, next_frame, ReadOnly)
                .backing(0x8ffe, &[0xa5, zero[0]]).backing(next_frame, &zero[1..])
                .backing(next_frame + zero.len() as u32 - 1, &[0x5a]);
            // The literal destination inputs retain the original parent-register canaries.
            let eax = if maximum.len() == 4 { 0 } else { 0x4433_0000 };
            let destination = Case::new(format!("{name} RMW destination, frame {next_frame:#x}"), destination_code, Flags::all(true), flags)
                .initial_register(Eax, eax).initial_register(Ebx, 0x4fff)
                .map_page(4, 0x8000, ReadWrite).map_page(5, next_frame, ReadWrite)
                .backing(0x8ffe, &[0xa5, maximum[0]]).backing(next_frame, &maximum[1..])
                .backing(next_frame + maximum.len() as u32 - 1, &[0x5a])
                .expect_memory(0x4fff, result);
            cases.extend([source, destination]);
        }
    }
    cases
}

#[rustfmt::skip]
fn immediate_and_address_operands() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, before, after, flags) in [
        ("ADC byte full immediate", &[0x80, 0x53, 0x80, 0xff][..], &[1][..], &[1][..],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("ADC word full immediate", &[0x66, 0x81, 0x53, 0x80, 0xff, 0xff][..], &[1, 0][..], &[1, 0][..],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("ADC word signed byte immediate", &[0x66, 0x83, 0x53, 0x80, 0x80][..], &[1, 0][..], &[0x82, 0xff][..],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }),
        ("ADC dword full immediate", &[0x81, 0x53, 0x80, 0xff, 0xff, 0xff, 0xff][..], &[1, 0, 0, 0][..], &[1, 0, 0, 0][..],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("ADC dword signed byte immediate", &[0x83, 0x53, 0x80, 0x80][..], &[1, 0, 0, 0][..], &[0x82, 0xff, 0xff, 0xff][..],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }),
        ("SBB byte full immediate", &[0x80, 0x5b, 0x80, 0xff][..], &[1][..], &[1][..],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("SBB word full immediate", &[0x66, 0x81, 0x5b, 0x80, 0xff, 0xff][..], &[1, 0][..], &[1, 0][..],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("SBB word signed byte immediate", &[0x66, 0x83, 0x5b, 0x80, 0x80][..], &[1, 0][..], &[0x80, 0][..],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }),
        ("SBB dword full immediate", &[0x81, 0x5b, 0x80, 0xff, 0xff, 0xff, 0xff][..], &[1, 0, 0, 0][..], &[1, 0, 0, 0][..],
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("SBB dword signed byte immediate", &[0x83, 0x5b, 0x80, 0x80][..], &[1, 0, 0, 0][..], &[0x80, 0, 0, 0][..],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear }),
    ] {
        cases.push(Case::new(name, code, Flags::all(true), flags).initial_register(Ebx, 0x40a0)
            .map_page(4, 0x8000, ReadWrite).backing(0x801f, &[0xa5; 6]).backing(0x8020, before)
            .expect_memory(0x4020, after));
    }
    cases.extend([
        Case::new("ADC [EAX],AL reads the original address register", &[0x10, 0x00], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4020).memory(0x401f, &[0xa5, 0xdf, 0xa5, 0xa5, 0xa5, 0xa5], ReadWrite)
            .expect_memory(0x4020, &[0]),
        Case::new("ADC EAX,[EAX] reads the original address register", &[0x13, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4020, 0x4026).memory(0x401f, &[0xa5, 5, 0, 0, 0, 0xa5], ReadOnly),
        Case::new("SBB [EAX],AL reads the original address register", &[0x18, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .initial_register(Eax, 0x4020).memory(0x401f, &[0xa5, 0xdf, 0xa5, 0xa5, 0xa5, 0xa5], ReadWrite)
            .expect_memory(0x4020, &[0xbe]),
        Case::new("SBB EAX,[EAX] reads the original address register", &[0x1b, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4020, 0x401a).memory(0x401f, &[0xa5, 5, 0, 0, 0, 0xa5], ReadOnly),
    ]);
    cases
}

test_cases!(memory_directions_and_layouts, memory_roles());
test_cases!(
    group_immediates_and_address_aliases,
    immediate_and_address_operands()
);

#[rustfmt::skip]
fn access_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for op in OPERATIONS {
        for bits in WIDTHS {
            for (destination, first, second, address, error) in [
                (true, None, None, 0x4fff, 2),
                (false, None, None, 0x4fff, 0),
                (true, Some(ReadOnly), None, 0x4fff, 3),
                (true, Some(ReadWrite), None, 0x5000, 2),
                (true, Some(ReadWrite), Some(ReadOnly), 0x5000, 3),
                (false, Some(ReadOnly), None, 0x5000, 0),
            ] {
                if bits == 8 && address == 0x5000 { continue; }
                let code = code_with_width(bits, op.opcode() + u8::from(bits != 8) + if destination { 0 } else { 2 }, &[0x03]);
                let mut case = Case::new(format!("{op:?}/{bits}, destination {destination}, fault {address:#x}/{error}"),
                    &code, Flags::all(true), Flags::all(Preserved)).preserve_flag_record()
                    .initial_register(Ebx, 0x4fff)
                    .backing(0x8ffe, &[0xa5, 0xff]).backing(0xa000, &[0xff, 0xff, 0xff, 0x5a])
                    .fault(address, error);
                if let Some(permissions) = first { case = case.map_page(4, 0x8000, permissions); }
                if let Some(permissions) = second { case = case.map_page(5, 0xa000, permissions); }
                cases.push(case);
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn prior_progress() -> Vec<SequenceCase> {
    let mut cases = Vec::new();
    for (name, opcode, carry_result, carry_flags) in [
        ("ADC", 0x11, 1, Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }),
        ("SBB", 0x19, 0xffff_fffd, Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear }),
    ] {
        cases.push(SequenceCase::new(format!("failed {name} RMW preserves prior ADD"), Flags::all(true))
            .initial_register(Eax, 0).initial_register(Ecx, 0xffff_ffff).initial_register(Edx, 1).initial_register(Ebx, 0x4fff)
            .map_page(4, 0x8000, ReadWrite).backing(0x8ffe, &[0xa5, 0xff])
            .step(Checkpoint::new(&[0x01, 0xd1],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Ecx, 0))
            .step(Checkpoint::preserving_flags(&[opcode, 0x03]).fault(0x5000, 2)));
        cases.push(SequenceCase::new(format!("failed {name} RMW preserves prior {name}"), Flags::all(true))
            .initial_register(Ecx, 0xffff_ffff).initial_register(Edx, 1).initial_register(Ebx, 0x4fff)
            .map_page(4, 0x8000, ReadWrite).backing(0x8ffe, &[0xa5, 0xff])
            .step(Checkpoint::new(&[opcode, 0xd1], carry_flags).register(Ecx, carry_result))
            .step(Checkpoint::preserving_flags(&[opcode, 0x03]).fault(0x5000, 2)));
    }
    cases
}

test_cases!(operand_access_faults_preserve_entry_state, access_faults());
test_sequences!(
    operand_faults_preserve_completed_arithmetic,
    prior_progress()
);

#[test]
fn fetch_and_length_faults_precede_operand_access() {
    let step = TestModule::interpreter();
    for op in OPERATIONS {
        let group = op.extension() << 3;
        for code in [
            vec![op.opcode()],
            vec![op.opcode() + 1, 0x04],
            vec![op.opcode() + 3, 0x05, 0, 0x40, 0],
            vec![0x80, group | 0x05, 0, 0x40, 0, 0],
            vec![0x81, group | 0x04, 0x25, 0, 0x40, 0, 0, 0xff, 0xff],
            vec![0x66, 0x81, group | 0x05, 0, 0x40, 0, 0, 0xff],
            vec![0x83, group | 0x05, 0, 0x40, 0, 0],
        ] {
            let start = 0x2000 - code.len() as u32;
            let mut image = flag_image(&[]);
            image.cpu.eip = start;
            image.data(0x3000 + (start & 0xfff), &code);
            image.check_unchanged_exit(
                Engine::Wasmtime,
                step,
                "carry fetch fault preserves flags and precedes operand access",
                Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            );
        }
        for suffix in [vec![op.opcode()], vec![0x81, group | 0xc0, 1]] {
            let code = [vec![0x66; 15 - suffix.len()], suffix].concat();
            let mut image = flag_image(&[]);
            image.cpu.eip = 0x1ff1;
            image.data(0x3ff1, &code);
            image.check_unchanged_exit(
                Engine::Wasmtime,
                step,
                "carry field beyond byte fifteen raises length before fetch",
                Exit::Other(0x0002_0000_0000_0000),
            );
        }
    }
}
