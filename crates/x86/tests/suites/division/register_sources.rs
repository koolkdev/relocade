use wasm86_x86::Gpr32::{self, Eax, Ebx, Ecx, Edx};

use crate::support::cases::{test_cases, InstructionCase as Case};

use super::successful_division as division;

struct ByteSource {
    parent: Gpr32,
    input: u32,
    accumulator: u32,
    output: Option<u32>,
}

#[rustfmt::skip]
fn byte_sources() -> Vec<Case> {
    let mut cases = Vec::new();
    for (mnemonic, extension, operands) in [
        ("DIV", 6, [
            ByteSource { parent: Eax, input: 0x4433_0130, accumulator: 0x4433_0130, output: Some(0x4433_1006) },
            ByteSource { parent: Ecx, input: 0x8877_6603, accumulator: 0x4433_0101, output: Some(0x4433_0255) },
            ByteSource { parent: Edx, input: 0xccbb_aa03, accumulator: 0x4433_0101, output: Some(0x4433_0255) },
            ByteSource { parent: Ebx, input: 0x10ff_ee03, accumulator: 0x4433_0101, output: Some(0x4433_0255) },
            ByteSource { parent: Eax, input: 0x4433_0101, accumulator: 0x4433_0101, output: None },
            ByteSource { parent: Ecx, input: 0x8877_0355, accumulator: 0x4433_0101, output: Some(0x4433_0255) },
            ByteSource { parent: Edx, input: 0xccbb_0399, accumulator: 0x4433_0101, output: Some(0x4433_0255) },
            ByteSource { parent: Ebx, input: 0x10ff_03dd, accumulator: 0x4433_0101, output: Some(0x4433_0255) },
        ]),
        ("IDIV", 7, [
            ByteSource { parent: Eax, input: 0x4433_ff9c, accumulator: 0x4433_ff9c, output: Some(0x4433_0001) },
            ByteSource { parent: Ecx, input: 0x8877_6607, accumulator: 0x4433_ff9c, output: Some(0x4433_fef2) },
            ByteSource { parent: Edx, input: 0xccbb_aa07, accumulator: 0x4433_ff9c, output: Some(0x4433_fef2) },
            ByteSource { parent: Ebx, input: 0x10ff_ee07, accumulator: 0x4433_ff9c, output: Some(0x4433_fef2) },
            ByteSource { parent: Eax, input: 0x4433_ff9c, accumulator: 0x4433_ff9c, output: Some(0x4433_0064) },
            ByteSource { parent: Ecx, input: 0x8877_0755, accumulator: 0x4433_ff9c, output: Some(0x4433_fef2) },
            ByteSource { parent: Edx, input: 0xccbb_0799, accumulator: 0x4433_ff9c, output: Some(0x4433_fef2) },
            ByteSource { parent: Ebx, input: 0x10ff_07dd, accumulator: 0x4433_ff9c, output: Some(0x4433_fef2) },
        ]),
    ] {
        for (selector, source) in operands.into_iter().enumerate() {
            let code = [0xf6, 0xc0 | (extension << 3) | selector as u8];
            let name = format!("{mnemonic} byte selector {selector} reads its old divisor");
            let mut case = if let Some(output) = source.output {
                division(name, &code).register(Eax, source.accumulator, output)
            } else {
                // A nonzero AH divisor makes the unsigned quotient at least 256.
                Case::preserving_flags(name, &code).initial_register(Eax, source.accumulator).divide_error()
            };
            if source.parent != Eax {
                case = case.initial_register(source.parent, source.input);
            }
            cases.push(case);
        }
    }
    cases
}

struct WideSources {
    name: &'static str,
    prefix: &'static [u8],
    extension: u8,
    accumulator: u32,
    high: u32,
    divisor: u32,
    quotient: u32,
    remainder: u32,
    quotient_from_high: Option<u32>,
}

#[rustfmt::skip]
fn wide_sources() -> Vec<Case> {
    let mut cases = Vec::new();
    for operands in [
        WideSources { name: "DIV word", prefix: &[0x66], extension: 6,
            accumulator: 0x4433_0030, high: 0xccbb_0001, divisor: 0x8877_0030,
            quotient: 0x4433_0556, remainder: 0xccbb_0010, quotient_from_high: None },
        WideSources { name: "DIV dword", prefix: &[], extension: 6,
            accumulator: 0x30, high: 1, divisor: 0x30,
            quotient: 0x0555_5556, remainder: 0x10, quotient_from_high: None },
        WideSources { name: "IDIV word", prefix: &[0x66], extension: 7,
            accumulator: 0x4433_ff9c, high: 0xccbb_ffff, divisor: 0x8877_ff9c,
            quotient: 0x4433_0001, remainder: 0xccbb_0000, quotient_from_high: Some(0x4433_0064) },
        WideSources { name: "IDIV dword", prefix: &[], extension: 7,
            accumulator: 0xffff_ff9c, high: 0xffff_ffff, divisor: 0xffff_ff9c,
            quotient: 1, remainder: 0, quotient_from_high: Some(100) },
    ] {
        for (selector, source) in Gpr32::ALL.into_iter().enumerate() {
            let mut code = operands.prefix.to_vec();
            code.extend_from_slice(&[0xf7, 0xc0 | (operands.extension << 3) | selector as u8]);
            let name = format!("{} reads the old {source:?} divisor", operands.name);
            let quotient = if source == Edx { operands.quotient_from_high } else { Some(operands.quotient) };
            let mut case = if let Some(quotient) = quotient {
                division(name, &code).register(Eax, operands.accumulator, quotient)
                    .register(Edx, operands.high, operands.remainder)
            } else {
                // A nonzero high-half divisor makes an unsigned quotient too wide.
                Case::preserving_flags(name, &code)
                    .initial_registers(&[(Eax, operands.accumulator), (Edx, operands.high)]).divide_error()
            };
            if source != Eax && source != Edx {
                case = case.initial_register(source, operands.divisor);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(every_low_and_high_byte_divisor, byte_sources());
test_cases!(every_word_and_dword_divisor, wide_sources());
