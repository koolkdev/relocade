use crate::support::encoding::check_length;
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ebx, Edx},
};

use crate::support::{
    cases::{test_cases, InstructionCase as Case},
    machine::{Exit, Image},
    step::{Engine, TestModule},
};

use super::successful_division as division;

#[test]
fn complete_divisor_encodings_have_no_immediate_or_successor_dependency() {
    for code in [
        &[0xf6, 0xf0][..],
        &[0xf6, 0xfc][..],
        &[0x66, 0xf7, 0x34, 0x8b][..],
        &[0x66, 0xf7, 0xbc, 0x8b, 0x20, 0x40, 0, 0][..],
        &[0xf7, 0x75, 0x80][..],
        &[0xf7, 0x3d, 0x20, 0x40, 0, 0][..],
        &[0x66, 0x66, 0xf6, 0xf3][..],
    ] {
        check_length(code);
    }
}

struct EncodedResult {
    name: &'static str,
    code: &'static [u8],
    accumulator: u32,
    high: u32,
    divisor: u32,
    quotient: u32,
    remainder: Option<u32>,
}

#[rustfmt::skip]
fn final_byte_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for result in [
        EncodedResult { name: "DIV byte", code: &[0xf6, 0xf3], accumulator: 0x4433_0101,
            high: 0xccbb_aa99, divisor: 3, quotient: 0x4433_0255, remainder: None },
        EncodedResult { name: "IDIV byte", code: &[0xf6, 0xfb], accumulator: 0x4433_ff9c,
            high: 0xccbb_aa99, divisor: 7, quotient: 0x4433_fef2, remainder: None },
        EncodedResult { name: "DIV word", code: &[0x66, 0xf7, 0xf3], accumulator: 0x4433_0001,
            high: 0xccbb_0001, divisor: 3, quotient: 0x4433_5555, remainder: Some(0xccbb_0002) },
        EncodedResult { name: "IDIV word", code: &[0x66, 0xf7, 0xfb], accumulator: 0x4433_ff9c,
            high: 0xccbb_ffff, divisor: 7, quotient: 0x4433_fff2, remainder: Some(0xccbb_fffe) },
        EncodedResult { name: "DIV dword", code: &[0xf7, 0xf3], accumulator: 1,
            high: 1, divisor: 3, quotient: 0x5555_5555, remainder: Some(2) },
        EncodedResult { name: "IDIV dword", code: &[0xf7, 0xfb], accumulator: 0xffff_ff9c,
            high: 0xffff_ffff, divisor: 7, quotient: 0xffff_fff2, remainder: Some(0xffff_fffe) },
    ] {
        let origin = 0x2000 - result.code.len() as u32;
        let mut case = division(format!("{} consumes the final mapped byte", result.name), result.code)
            .at(origin).register(Eax, result.accumulator, result.quotient).initial_register(Ebx, result.divisor);
        case = if let Some(remainder) = result.remainder { case.register(Edx, result.high, remainder) }
            else { case.initial_register(Edx, result.high) };
        cases.push(case);
        cases.push(Case::preserving_flags(format!("{} zero divisor needs no successor fetch", result.name), result.code)
            .at(origin).initial_registers(&[(Eax, result.accumulator), (Edx, result.high), (Ebx, 0)]).divide_error());
    }
    cases.push(division("DIV dword ModRM crosses EIP wrap", &[0xf7, 0xf3])
        .at(0xffff_ffff).register(Eax, 1, 0x5555_5555).register(Edx, 1, 2).initial_register(Ebx, 3));
    cases.push(division("IDIV word prefix and ModRM cross EIP wrap", &[0x66, 0xf7, 0xfb])
        .at(0xffff_fffe).register(Eax, 0x4433_ff9c, 0x4433_fff2)
        .register(Edx, 0xccbb_ffff, 0xccbb_fffe).initial_register(Ebx, 7));
    cases
}

#[rustfmt::skip]
fn maximum_length_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for result in [
        EncodedResult { name: "DIV byte ignores repeated operand prefixes", code: &[0xf6, 0xf3],
            accumulator: 0x4433_0101, high: 0xccbb_aa99, divisor: 3, quotient: 0x4433_0255, remainder: None },
        EncodedResult { name: "IDIV byte ignores repeated operand prefixes", code: &[0xf6, 0xfb],
            accumulator: 0x4433_ff9c, high: 0xccbb_aa99, divisor: 7, quotient: 0x4433_fef2, remainder: None },
        EncodedResult { name: "DIV word uses repeated operand prefixes", code: &[0xf7, 0xf3],
            accumulator: 0x4433_0001, high: 0xccbb_0001, divisor: 3, quotient: 0x4433_5555, remainder: Some(0xccbb_0002) },
        EncodedResult { name: "IDIV word uses repeated operand prefixes", code: &[0xf7, 0xfb],
            accumulator: 0x4433_ff9c, high: 0xccbb_ffff, divisor: 7, quotient: 0x4433_fff2, remainder: Some(0xccbb_fffe) },
    ] {
        let code = [vec![0x66; 15 - result.code.len()], result.code.to_vec()].concat();
        let mut case = division(format!("{} and ends at byte fifteen", result.name), &code)
            .at(0x1ff1).register(Eax, result.accumulator, result.quotient).initial_register(Ebx, result.divisor);
        case = if let Some(remainder) = result.remainder { case.register(Edx, result.high, remainder) }
            else { case.initial_register(Edx, result.high) };
        cases.push(case);
    }
    cases
}

#[test]
fn missing_divisor_address_fields_fault_before_operand_access_or_divide_error() {
    for code in [
        &[0xf6][..],
        &[0xf7][..],
        &[0x66, 0xf7][..],
        &[0xf6, 0x34][..],
        &[0xf7, 0x7c, 0x8b][..],
        &[0x66, 0xf7, 0xb5, 0x20, 0x40, 0][..],
        &[0xf7, 0x3d, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.cpu.registers.eax = 0x4433_8000;
        image.cpu.registers.edx = 0x8000_0000;
        image.cpu.registers.ebx = 0x4020;
        image.cpu.registers.ecx = 0;
        image.data(0x3000 + (start & 0xfff), code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            &format!("missing division field in {code:02x?}"),
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn a_sixteenth_divisor_field_byte_reports_the_length_limit_before_fetch() {
    for (prefixes, suffix) in [
        (14, &[0xf6][..]),
        (14, &[0xf7][..]),
        (13, &[0xf7, 0x34][..]),
        (12, &[0xf7, 0x7c, 0x8b][..]),
        (10, &[0xf7, 0xbc, 0x8b, 0x20, 0x40][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "division field exceeds fifteen bytes",
            Exit::Other(0x0002_0000_0000_0000),
        );
    }
}

test_cases!(
    the_complete_instruction_needs_no_successor_fetch,
    final_byte_cases()
);
test_cases!(repeated_prefixes_and_maximum_length, maximum_length_cases());
