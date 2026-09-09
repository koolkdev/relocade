use wasm86_x86::compile_block_from_bytes;
use wasmparser::Validator;

use crate::support::arithmetic;
use crate::support::conditions;
use crate::support::machine;
use crate::support::step;
use arithmetic::{image, recipe};
use conditions::check_conditions;
use machine::{both, check, Exit, Image, Step};
use step::TestModule;
#[path = "carry_arithmetic/operands.rs"]
mod operands;

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
    // Literal CPU order: CF, PF, AF, ZF, SF, OF.
    status: [u8; 6],
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
        status: [cf, pf, af, zf, sf, of].map(u8::from),
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

fn concrete_updates(image: &Image, expected: &Expected) -> Vec<(usize, u32)> {
    let mut cpu = image.cpu;
    cpu[0] = 0;
    cpu[12..18].copy_from_slice(&expected.status);
    [0, 12, 16]
        .map(|offset| {
            (
                offset,
                u32::from_le_bytes(cpu[offset..offset + 4].try_into().unwrap()),
            )
        })
        .to_vec()
}

fn check_result(
    step: &TestModule,
    name: &str,
    code: &[u8],
    image: &Image,
    expected: &Expected,
    register: (usize, u32),
) {
    let next = 0x1000 + code.len() as u32;
    let mut updates = concrete_updates(image, expected);
    updates.extend_from_slice(&[register, (56, next), (144, 0)]);
    both(
        step,
        name,
        code,
        1,
        image,
        &[Step {
            cpu: &updates,
            ram: &[],
            exit: Exit::Dispatch(next),
        }],
    );
}

#[test]
fn edge_conditions() {
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
                    image.cpu[12] = u8::from(carry);
                    image.register(24, eax);
                    image.register(36, right);
                    let mut updates = concrete_updates(&image, &expected);
                    updates.push((24, register_result(eax, bits, expected.result)));
                    check_conditions(
                        step,
                        &format!("{op:?}/{bits}: {left:#x}, {right:#x}, carry {carry}"),
                        &code,
                        &mut image,
                        &updates,
                        expected.conditions,
                    );
                }
            }
        }
    }
}

#[test]
fn register_and_immediate_forms() {
    let step = TestModule::interpreter();
    for op in OPERATIONS {
        for bits in WIDTHS {
            let wide = u8::from(bits != 8);
            let group = 0xc0 | (op.extension() << 3);
            let immediate = mask(bits).to_le_bytes();
            let mut forms = vec![
                (
                    code_with_width(bits, op.opcode() + wide, &[0xd8]),
                    mask(bits),
                ),
                (
                    code_with_width(bits, op.opcode() + 2 + wide, &[0xc3]),
                    mask(bits),
                ),
                (
                    code_with_width(
                        bits,
                        op.opcode() + 4 + wide,
                        &immediate[..(bits / 8) as usize],
                    ),
                    mask(bits),
                ),
                (
                    code_with_width(
                        bits,
                        0x80 + wide,
                        &[&[group], &immediate[..(bits / 8) as usize]].concat(),
                    ),
                    mask(bits),
                ),
            ];
            if bits != 8 {
                for byte in [0x7f, 0x80, 0xff] {
                    forms.push((
                        code_with_width(bits, 0x83, &[group, byte]),
                        (byte as i8 as u32) & mask(bits),
                    ));
                }
            } else {
                forms.push((vec![0x66, op.opcode() + 4, 0xff], 0xff));
            }
            for (code, right) in forms {
                let mut image = image(&code);
                let eax = register_result(0x4433_2200, bits, 1);
                image.register(24, eax);
                image.register(36, right);
                let expected = expected(op, bits, 1, right, true);
                check_result(
                    step,
                    &format!("carry encoding {code:02x?}"),
                    &code,
                    &image,
                    &expected,
                    (24, register_result(eax, bits, expected.result)),
                );
            }
        }
        for (modrm, left_shift, right_shift) in [(0xe0, 0, 8), (0xc4, 8, 0), (0xe4, 8, 8)] {
            let code = [op.opcode(), modrm];
            let mut image = image(&code);
            let eax = 0x4433_7f80u32;
            image.register(24, eax);
            let expected = expected(op, 8, eax >> left_shift, eax >> right_shift, true);
            let result = (eax & !(0xff << left_shift)) | (expected.result << left_shift);
            check_result(
                step,
                "carry operation reads byte aliases before writing",
                &code,
                &image,
                &expected,
                (24, result),
            );
        }
        for bits in [16, 32] {
            let code = code_with_width(bits, op.opcode() + 1, &[0xc0]);
            let mut image = image(&code);
            let eax = 0x8000_8000;
            image.register(24, eax);
            let expected = expected(op, bits, eax, eax, true);
            check_result(
                step,
                "carry operation reads old self operand",
                &code,
                &image,
                &expected,
                (24, register_result(eax, bits, expected.result)),
            );
        }
    }
}

#[test]
fn incoming_sources() {
    let step = TestModule::interpreter();
    for source_bits in WIDTHS {
        let width_tag = match source_bits {
            8 => 0,
            16 => 4,
            _ => 8,
        };
        let max = mask(source_bits);
        // Narrow records deliberately contain unrelated upper payload bits.
        let high = !max & 0x7e57_ae00;
        for (kind, left, right, carry) in [
            (2, max, 1, true),
            (2, 0, 1, false),
            (1, 0, 1, true),
            (1, max, 1, false),
            (3, max, 0xdead_beef, false),
        ] {
            for bits in WIDTHS {
                for op in OPERATIONS {
                    let code = code_with_width(
                        bits,
                        op.opcode() + 4 + u8::from(bits != 8),
                        &vec![0; (bits / 8) as usize],
                    );
                    let mut image = image(&code);
                    for (offset, value) in recipe(width_tag | kind, left | high, right | high) {
                        image.register(offset, value);
                    }
                    image.cpu[12] = u8::from(!carry);
                    let eax = register_result(0x4433_2200, bits, 0);
                    image.register(24, eax);
                    let expected = expected(op, bits, 0, 0, carry);
                    check_result(
                        step,
                        &format!("stored kind {}, {op:?}/{bits}", width_tag | kind),
                        &code,
                        &image,
                        &expected,
                        (24, register_result(eax, bits, expected.result)),
                    );
                }
            }
        }
    }
}

#[test]
fn local_sources_and_publication() {
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
                image.cpu[12] = u8::from(!carry);
                image.register(24, 0xffff_ffff);
                image.register(28, register_result(0x4433_2200, source_bits, left));
                image.register(32, right);
                image.register(36, 0);
                let ecx = register_result(0x4433_2200, source_bits, source_result);
                let tag = kind
                    | match source_bits {
                        8 => 0,
                        16 => 4,
                        _ => 8,
                    };
                let mut first = if kind == 3 {
                    vec![(0, 0xa5a5_a500 | u32::from(tag)), (4, source_result)]
                } else {
                    recipe(tag, left, right).to_vec()
                };
                first.extend_from_slice(&[
                    (28, ecx),
                    (56, 0x1000 + producer.len() as u32),
                    (144, 0),
                ]);
                let expected = expected(op, 32, 0xffff_ffff, 0, carry);
                let next = 0x1000 + code.len() as u32;
                let mut last = concrete_updates(&image, &expected);
                last.extend_from_slice(&[(24, expected.result), (56, next), (144, 1)]);
                check(
                    step,
                    "carry consumes the prior completed source",
                    &image,
                    &[
                        Step {
                            cpu: &first,
                            ram: &[],
                            exit: Exit::Dispatch(0x1000 + producer.len() as u32),
                        },
                        Step {
                            cpu: &last,
                            ram: &[],
                            exit: Exit::Dispatch(next),
                        },
                    ],
                );
                // A single snapshot publishes only its final flags. The overwritten
                // source's unused payload must retain its original backing bytes.
                let snapshot = compile_block_from_bytes(0x1000, &code, 2).unwrap();
                Validator::new().validate_all(&snapshot.bytes).unwrap();
                last.push((28, ecx));
                check(
                    &TestModule::new(&snapshot),
                    "local carry source publishes concrete flags",
                    &image,
                    &[Step {
                        cpu: &last,
                        ram: &[],
                        exit: Exit::Dispatch(next),
                    }],
                );
            }
        }
    }
}

fn mixed_carry_updates(image: &Image) -> Vec<Vec<(usize, u32)>> {
    let mut eax = 0xffff_ffff;
    let mut carry = true;
    let mut updates = Vec::new();
    for (count, (op, bits, next)) in [
        (Operation::Adc, 8, 0x1002),
        (Operation::Sbb, 16, 0x1005),
        (Operation::Adc, 32, 0x1007),
    ]
    .into_iter()
    .enumerate()
    {
        let result = expected(op, bits, eax, 0, carry);
        eax = register_result(eax, bits, result.result);
        carry = result.status[0] != 0;
        let mut changes = concrete_updates(image, &result);
        changes.extend_from_slice(&[(24, eax), (56, next), (144, count as u32)]);
        updates.push(changes);
    }
    updates
}

#[test]
fn mixed_carry_chain_replaces_incoming_carry() {
    let step = TestModule::interpreter();
    let code = [0x10, 0xd8, 0x66, 0x19, 0xd8, 0x11, 0xd8];
    let mut image = image(&code);
    image.register(24, 0xffff_ffff);
    image.register(36, 0);
    let updates = mixed_carry_updates(&image);
    let steps = updates
        .iter()
        .zip([0x1002, 0x1005, 0x1007])
        .map(|(cpu, next)| Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(next),
        })
        .collect::<Vec<_>>();
    both(
        step,
        "mixed-width carry chain replaces cached incoming CF",
        &code,
        3,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn mixed_carry_chain_executes_in_optimizing_v8() {
    let code = [0x10, 0xd8, 0x66, 0x19, 0xd8, 0x11, 0xd8];
    let mut image = image(&code);
    image.register(24, 0xffff_ffff);
    image.register(36, 0);
    let updates = mixed_carry_updates(&image);
    let steps = updates
        .iter()
        .zip([0x1002, 0x1005, 0x1007])
        .map(|(cpu, next)| Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(next),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), 3),
        machine::expected(&image, &steps),
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: updates.last().unwrap(),
                ram: &[],
                exit: Exit::Dispatch(0x1007)
            }]
        ),
    );
}
