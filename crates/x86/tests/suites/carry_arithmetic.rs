use wasm86_x86::{compile_block_from_bytes, CpuState, StatusFlags};
use wasmparser::Validator;

use crate::support::arithmetic;
use crate::support::conditions;
use crate::support::machine;
use crate::support::step;
use arithmetic::image;
use conditions::check_conditions;
use machine::{check, Exit, Step};
use step::TestModule;
#[path = "carry_arithmetic/operands.rs"]
mod operands;
#[path = "carry_arithmetic/registers.rs"]
mod registers;
#[path = "carry_arithmetic/sources.rs"]
mod sources;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Adc,
    Sbb,
}

impl Operation {
    fn opcode(self) -> u8 {
        match self {
            Self::Adc => 0x10,
            Self::Sbb => 0x18,
        }
    }

    fn extension(self) -> u8 {
        match self {
            Self::Adc => 2,
            Self::Sbb => 3,
        }
    }
}

const OPERATIONS: [Operation; 2] = [Operation::Adc, Operation::Sbb];
const WIDTHS: [u32; 3] = [8, 16, 32];

struct Expected {
    result: u32,
    status: StatusFlags,
    conditions: u16,
}

// Use widened unsigned, signed, and nibble arithmetic independently. In particular,
// the carry is never combined with a width-truncated right operand.
fn expected(op: Operation, bits: u32, left: u32, right: u32, carry: bool) -> Expected {
    let modulus = 1i64 << bits;
    let mask = modulus - 1;
    let left = i64::from(left) & mask;
    let right = i64::from(right) & mask;
    let carry = i64::from(carry);
    let signed = |value: i64| {
        if value >= modulus / 2 {
            value - modulus
        } else {
            value
        }
    };
    let (wide, signed_wide, nibble) = match op {
        Operation::Adc => (
            left + right + carry,
            signed(left) + signed(right) + carry,
            (left % 16) + (right % 16) + carry,
        ),
        Operation::Sbb => (
            left - right - carry,
            signed(left) - signed(right) - carry,
            (left % 16) - (right % 16) - carry,
        ),
    };
    let result = wide.rem_euclid(modulus) as u32;
    let cf = !(0..modulus).contains(&wide);
    let pf = (result as u8).count_ones() % 2 == 0;
    let af = !(0..16).contains(&nibble);
    let zf = result == 0;
    let sf = i64::from(result) >= modulus / 2;
    let of = !(-modulus / 2..modulus / 2).contains(&signed_wide);
    let mut conditions = 0;
    for (pair, value) in [of, cf, zf, cf || zf, sf, pf, sf != of, zf || sf != of]
        .into_iter()
        .enumerate()
    {
        conditions |= 1 << (2 * pair + usize::from(!value));
    }
    Expected {
        result,
        status: StatusFlags {
            cf: u8::from(cf),
            pf: u8::from(pf),
            af: u8::from(af),
            zf: u8::from(zf),
            sf: u8::from(sf),
            of: u8::from(of),
        },
        conditions,
    }
}

fn mask(bits: u32) -> u32 {
    ((1u64 << bits) - 1) as u32
}

fn register_result(original: u32, bits: u32, result: u32) -> u32 {
    (original & !mask(bits)) | result
}

fn code_with_width(bits: u32, opcode: u8, tail: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    if bits == 16 {
        code.push(0x66);
    }
    code.push(opcode);
    code.extend_from_slice(tail);
    code
}

fn concrete_cpu(mut cpu: CpuState, expected: &Expected) -> CpuState {
    cpu.flags.kind = 0;
    cpu.flags.status = expected.status;
    cpu
}

#[test]
fn widened_arithmetic_model_checks_edge_results_and_conditions() {
    let step = TestModule::interpreter();
    for op in OPERATIONS {
        for bits in WIDTHS {
            let max = mask(bits);
            let sign = 1 << (bits - 1);
            for (left, right) in [
                (0, 0),
                (max, 0),
                (0, max),
                (max, max),
                (sign - 1, 0),
                (0, sign - 1),
                (sign, sign - 1),
                (sign - 1, sign),
                (sign, 0),
                (sign, sign),
                (0x0f, 0),
            ] {
                for carry in [false, true] {
                    let code = code_with_width(bits, op.opcode() + u8::from(bits != 8), &[0xd8]);
                    let eax = register_result(0x4433_2200, bits, left);
                    let expected = expected(op, bits, left, right, carry);
                    let mut image = image(&code);
                    image.cpu.flags.status.cf = u8::from(carry);
                    image.cpu.registers.eax = eax;
                    image.cpu.registers.ebx = right;
                    let mut cpu = concrete_cpu(image.cpu, &expected);
                    cpu.registers.eax = register_result(eax, bits, expected.result);
                    check_conditions(
                        step,
                        &format!("{op:?}/{bits}: {left:#x}, {right:#x}, carry {carry}"),
                        &code,
                        &mut image,
                        &cpu,
                        expected.conditions,
                    );
                }
            }
        }
    }
}

#[test]
fn discarded_local_carry_operands_remain_unpublished() {
    let step = TestModule::interpreter();
    for source_bits in WIDTHS {
        for (opcode, kind, left, right, source_result, carry) in [
            (0x00, 2, mask(source_bits), 1, 0, true),
            (0x28, 1, 0, 1, mask(source_bits), true),
            (0x30, 3, mask(source_bits), 1, mask(source_bits) - 1, false),
        ] {
            for op in OPERATIONS {
                let producer =
                    code_with_width(source_bits, opcode + u8::from(source_bits != 8), &[0xd1]);
                let consumer = [op.opcode() + 1, 0xd8];
                let code = [producer.as_slice(), &consumer].concat();
                let mut image = image(&code);
                image.cpu.flags.status.cf = u8::from(!carry);
                image.cpu.registers.eax = 0xffff_ffff;
                image.cpu.registers.ecx = register_result(0x4433_2200, source_bits, left);
                image.cpu.registers.edx = right;
                image.cpu.registers.ebx = 0;
                let ecx = register_result(0x4433_2200, source_bits, source_result);
                let tag = kind
                    | match source_bits {
                        8 => 0,
                        16 => 4,
                        _ => 8,
                    };
                let mut expected_cpu = image.cpu;
                let mut steps = Vec::new();

                if kind == 3 {
                    expected_cpu.flags.kind = tag;
                    expected_cpu.flags.left = source_result;
                } else {
                    expected_cpu.flags.kind = tag;
                    expected_cpu.flags.left = left;
                    expected_cpu.flags.right = right;
                }
                expected_cpu.registers.ecx = ecx;
                expected_cpu.eip = 0x1000 + producer.len() as u32;
                expected_cpu.instruction_count = 0;
                steps.push(Step {
                    cpu: expected_cpu,
                    ram: &[],
                    exit: Exit::Dispatch(expected_cpu.eip),
                });

                let expected = expected(op, 32, 0xffff_ffff, 0, carry);
                let next = 0x1000 + code.len() as u32;
                expected_cpu.flags.kind = 0;
                expected_cpu.flags.status = expected.status;
                expected_cpu.registers.eax = expected.result;
                expected_cpu.eip = next;
                expected_cpu.instruction_count = 1;
                steps.push(Step {
                    cpu: expected_cpu,
                    ram: &[],
                    exit: Exit::Dispatch(next),
                });

                check(
                    step,
                    "carry consumes the prior completed source",
                    &image,
                    &steps,
                );

                // A single snapshot publishes only its final flags. The overwritten
                // source's unused payload must retain its original backing bytes.
                let snapshot = compile_block_from_bytes(0x1000, &code, 2).unwrap();
                Validator::new().validate_all(&snapshot.bytes).unwrap();
                expected_cpu.flags.left = image.cpu.flags.left;
                expected_cpu.flags.right = image.cpu.flags.right;

                check(
                    &TestModule::new(&snapshot),
                    "local carry source publishes concrete flags",
                    &image,
                    &[Step {
                        cpu: expected_cpu,
                        ram: &[],
                        exit: Exit::Dispatch(next),
                    }],
                );
            }
        }
    }
}

#[rustfmt::skip]
fn mixed_carry_sequence() -> Vec<crate::support::sequences::SequenceCase> {
    use wasm86_x86::Gpr32::{Eax, Ebx};
    use crate::support::{
        cases::{Flags, FlagExpectation::{Clear, Set}},
        sequences::{Checkpoint, SequenceCase},
    };
    vec![SequenceCase::new("mixed-width carry chain replaces cached incoming CF", Flags::all(true))
        .initial_register(Eax, 0xffff_ffff).initial_register(Ebx, 0)
        .step(Checkpoint::new(&[0x10, 0xd8],
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0xffff_ff00))
        .step(Checkpoint::new(&[0x66, 0x19, 0xd8],
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xffff_feff))
        .step(Checkpoint::new(&[0x11, 0xd8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }))]
}

crate::support::sequences::test_sequences!(mixed_carry_chain, mixed_carry_sequence());
