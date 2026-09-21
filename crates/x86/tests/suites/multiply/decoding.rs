use crate::support::encoding::check_length;
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ebx, Edx},
};

use crate::support::{
    cases::{test_cases, FlagExpectation::Clear, InstructionCase as Case},
    machine::{check, Exit, Image, Step},
    step::TestModule,
};

use super::product_flags;

#[test]
fn snapshots_require_the_source_fields_and_the_selected_immediate_width() {
    for code in [
        &[0xf6, 0xe4][..],
        &[0x66, 0xf7, 0x24, 0x8b][..],
        &[0xf7, 0x2d, 0x20, 0x40, 0, 0][..],
        &[0x0f, 0xaf, 0xc3][..],
        &[0x66, 0x0f, 0xaf, 0x84, 0x8b, 0x20, 0x40, 0, 0][..],
        &[0x69, 0xc0, 0xfe, 0xff, 0xff, 0xff][..],
        &[0x66, 0x69, 0x45, 0x80, 0xfe, 0xff][..],
        &[0x6b, 0xc7, 0xff][..],
        &[0x66, 0x6b, 0x84, 0x8b, 0x20, 0x40, 0, 0, 0x80][..],
        &[0x66, 0x66, 0xf6, 0xec][..],
        &[0x69, 0x04, 0x25, 0x20, 0x40, 0, 0, 0xfe, 0xff, 0xff, 0xff][..],
    ] {
        check_length(code);
    }
}

#[rustfmt::skip]
fn page_end_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, input, product, high) in [
        ("MUL byte", &[0xf6, 0xe3][..], 0x4433_2202, 0x4433_0006, None),
        ("IMUL byte", &[0xf6, 0xeb][..], 0x4433_2202, 0x4433_0006, None),
        ("MUL word", &[0x66, 0xf7, 0xe3][..], 0x4433_0002, 0x4433_0006, Some(0xccbb_0000)),
        ("IMUL word", &[0x66, 0xf7, 0xeb][..], 0x4433_0002, 0x4433_0006, Some(0xccbb_0000)),
        ("MUL dword", &[0xf7, 0xe3][..], 2, 6, Some(0)),
        ("IMUL dword", &[0xf7, 0xeb][..], 2, 6, Some(0)),
        ("IMUL word two operands", &[0x66, 0x0f, 0xaf, 0xc3][..], 0x4433_0002, 0x4433_0006, None),
        ("IMUL dword two operands", &[0x0f, 0xaf, 0xc3][..], 2, 6, None),
        ("IMUL word wide immediate", &[0x66, 0x69, 0xc3, 0xfe, 0xff][..], 0x4433_dead, 0x4433_fffa, None),
        ("IMUL dword wide immediate", &[0x69, 0xc3, 0xfe, 0xff, 0xff, 0xff][..], 0x4433_dead, 0xffff_fffa, None),
        ("IMUL word byte immediate", &[0x66, 0x6b, 0xc3, 0xfe][..], 0x4433_dead, 0x4433_fffa, None),
        ("IMUL dword byte immediate", &[0x6b, 0xc3, 0xfe][..], 0x4433_dead, 0xffff_fffa, None),
    ] {
        let mut case = Case::replacing_flags(format!("{name} consumes its final mapped byte"), code, product_flags(Clear))
            .at(0x2000 - code.len() as u32)
            .register(Eax, input, product).initial_register(Ebx, 3);
        case = if let Some(high) = high { case.register(Edx, 0xccbb_aa99, high) }
            else { case.initial_register(Edx, 0xccbb_aa99) };
        cases.push(case);
    }
    cases.push(Case::replacing_flags("IMUL two-byte opcode crosses EIP wrap", &[0x0f, 0xaf, 0xc3], product_flags(Clear))
        .at(0xffff_ffff).register(Eax, 2, 6).initial_register(Ebx, 3));
    cases.push(Case::replacing_flags("IMUL wide immediate crosses EIP wrap", &[0x69, 0xc3, 0xfe, 0xff, 0xff, 0xff], product_flags(Clear))
        .at(0xffff_fffc).register(Eax, 0x4433_dead, 0xffff_fffa).initial_register(Ebx, 3));
    cases
}

fn maximum_length_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, suffix, source, low, high) in [
        (
            "MUL byte ignores repeated operand prefixes",
            &[0xf6, 0xe3][..],
            3,
            0x4433_0006,
            None,
        ),
        (
            "IMUL byte ignores repeated operand prefixes",
            &[0xf6, 0xeb][..],
            0xffff_fffd,
            0x4433_fffa,
            None,
        ),
        (
            "MUL word",
            &[0xf7, 0xe3][..],
            3,
            0x4433_0006,
            Some(0xccbb_0000),
        ),
        (
            "IMUL word",
            &[0xf7, 0xeb][..],
            0xffff_fffd,
            0x4433_fffa,
            Some(0xccbb_ffff),
        ),
        (
            "IMUL word two operands",
            &[0x0f, 0xaf, 0xc3][..],
            0xffff_fffd,
            0x4433_fffa,
            None,
        ),
        (
            "IMUL word wide immediate",
            &[0x69, 0xc3, 0xfe, 0xff][..],
            0xffff_fffd,
            0x4433_0006,
            None,
        ),
        (
            "IMUL word byte immediate",
            &[0x6b, 0xc3, 0xfe][..],
            0xffff_fffd,
            0x4433_0006,
            None,
        ),
    ] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        let mut case = Case::replacing_flags(
            format!("{name} ends at byte fifteen"),
            &code,
            product_flags(Clear),
        )
        .at(0x1ff1)
        .register(Eax, 0x4433_0002, low)
        .initial_register(Ebx, source);
        case = if let Some(high) = high {
            case.register(Edx, 0xccbb_aa99, high)
        } else {
            case.initial_register(Edx, 0xccbb_aa99)
        };
        cases.push(case);
    }
    cases
}

#[test]
fn missing_source_or_immediate_fields_fault_before_operand_access() {
    for code in [
        &[0xf6][..],
        &[0xf7, 0x24][..],
        &[0x66, 0xf7, 0x6c, 0x8b][..],
        &[0x0f][..],
        &[0x0f, 0xaf][..],
        &[0x0f, 0xaf, 0x05, 0x20, 0x40, 0][..],
        &[0x66, 0x69, 0xc3, 0xa1][..],
        &[0x69, 0xc3, 1, 2, 3][..],
        &[0x6b, 0xc3][..],
        &[0x66, 0x6b, 0x84, 0x8b, 0x20, 0x40, 0, 0][..],
        &[0x69, 0x84, 0x8b, 0x20, 0x40, 0, 0, 1, 2, 3][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.cpu.registers.eax = 0x4433_2280;
        image.cpu.registers.ebx = 0x4020;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            &format!("missing multiply field in {code:02x?}"),
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
fn a_required_sixteenth_byte_reports_the_length_limit_before_fetch() {
    for (prefixes, suffix) in [
        (14, &[0xf6][..]),
        (13, &[0xf7, 0x24][..]),
        (13, &[0x0f, 0xaf][..]),
        (13, &[0x69, 0xc3][..]),
        (12, &[0x69, 0xc3, 1][..]),
        (13, &[0x6b, 0xc3][..]),
        (8, &[0x69, 0x84, 0x8b, 0x20, 0x40, 0, 0][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "multiply field exceeds fifteen bytes",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0002_0000_0000_0000),
            }],
        );
    }
}

test_cases!(complete_encodings_need_no_successor_fetch, page_end_cases());
test_cases!(repeated_prefixes_and_maximum_length, maximum_length_cases());
