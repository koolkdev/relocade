//! Vector encodings follow the same value, effect and control-flow contracts as scalars.

#[path = "vectors/demand.rs"]
mod demand;
#[path = "vectors/floating.rs"]
mod floating;
#[path = "vectors/folding.rs"]
mod folding;
#[path = "vectors/lanes.rs"]
mod indexed_lanes;
#[path = "vectors/layout.rs"]
mod layout;
#[path = "vectors/masks.rs"]
mod masks;
#[path = "vectors/memory.rs"]
mod memory;
#[path = "vectors/packed.rs"]
mod packed;
#[path = "vectors/shifts.rs"]
mod shifts;
#[path = "vectors/shuffle_lowering.rs"]
mod shuffle_lowering;
#[path = "vectors/width.rs"]
mod width;

use crate::{
    fixture::{signature, Fixture},
    wasm::{MemoryBytes, TestModule, Value},
};
use relocade_compiler::{Argument, BuildError, Program, Type, Val, F64, I1, I32, I8, V128};

const LEFT: u128 = 0x01234567_89abcdef_fedcba98_76543210;
const RIGHT: u128 = 0xf0f00f0f_55aaaa55_3333cccc_00ffff00;

fn check(module: &TestModule, expected: &[u8], count: i32) {
    let arguments = [Value::I32(count)];

    let mut instance = module.instantiate();
    assert_eq!(instance.call_values("run", &arguments).unwrap(), []);
    assert_eq!(&instance.memory("state")[..expected.len()], expected);
}

#[test]
fn bitwise_operations_preserve_all_128_bits() {
    let mut bytes = vec![0xa5; 160];
    bytes[..16].copy_from_slice(&LEFT.to_le_bytes());
    bytes[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let choose = body.parameter::<I32>(0)?.ne(0);
        let literals = Val::<V128>::from(LEFT.to_le_bytes());
        let results = [
            left.and(&right),
            left.or(&right),
            left.xor(&right),
            literals.and(RIGHT),
            literals.or(RIGHT),
            literals.xor(RIGHT),
            choose.select(&left, &right),
            left.xor(&left),
        ];
        for (index, result) in results.into_iter().enumerate() {
            body.store(memory, 32 + index as u32 * 16, result)?;
        }
        body.return_(())
    });
    for count in [0, 1] {
        let mut expected = bytes.clone();
        for (index, value) in [
            LEFT & RIGHT,
            LEFT | RIGHT,
            LEFT ^ RIGHT,
            LEFT & RIGHT,
            LEFT | RIGHT,
            LEFT ^ RIGHT,
            if count == 0 { RIGHT } else { LEFT },
            0,
        ]
        .into_iter()
        .enumerate()
        {
            expected[32 + index * 16..48 + index * 16].copy_from_slice(&value.to_le_bytes());
        }
        check(&module, &expected, count);
    }
}

#[test]
fn vector_calls_selections_and_loop_backedges() {
    let bytes = vec![0xa5; 48];
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let helper =
        fixture
            .program
            .function::<fn(V128, V128) -> (V128, V128)>(move |body, (first, second)| {
                body.return_((second, first))
            });
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let count = body.parameter::<I32>(0)?;
        let (first, second) =
            body.call::<(V128, V128)>(helper, &[RIGHT.into(), LEFT.to_le_bytes().into()])?;
        let (first, second) = body.loop_::<(I32, V128, V128), (V128, V128)>(
            (&count, first, second),
            |mut iteration, labels, (remaining, first, second)| {
                iteration.branch_if(remaining.eq(0), &labels.exit, (&first, &second))?;
                iteration.branch(&labels.again, (remaining.sub(1), second, first))
            },
        )?;
        let joined = body.if_value::<V128>(
            count.eq(0),
            |arm| arm.yield_(0_u128),
            |arm| arm.yield_(u128::MAX),
        )?;
        body.store(memory, 0, first)?;
        body.store(memory, 16, second)?;
        body.store(memory, 32, joined)?;
        body.return_(())
    });
    for count in [0, 1, 2, 5] {
        let pair = if count % 2 == 0 {
            [LEFT, RIGHT]
        } else {
            [RIGHT, LEFT]
        };
        let mut expected = bytes.clone();
        expected[..16].copy_from_slice(&pair[0].to_le_bytes());
        expected[16..32].copy_from_slice(&pair[1].to_le_bytes());
        expected[32..].fill(if count == 0 { 0 } else { 0xff });
        check(&module, &expected, count);
    }
}

#[test]
fn vector_literals_with_zero_upper_bits_keep_vector_semantics() {
    let masks = [0, 1, 0xff, u128::from(u64::MAX), u128::MAX];
    let mut bytes = vec![0xa5; 16 + masks.len() * 3 * 16];
    bytes[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let input = body.load::<V128>(memory, 0)?;
        for (index, mask) in masks.into_iter().enumerate() {
            for (operation, result) in [input.and(mask), input.or(mask), input.xor(mask)]
                .into_iter()
                .enumerate()
            {
                body.store(memory, 16 + ((index * 3 + operation) * 16) as u32, result)?;
            }
        }
        body.return_(())
    });
    let mut expected = bytes.clone();
    for (index, mask) in masks.into_iter().enumerate() {
        for (operation, result) in [LEFT & mask, LEFT | mask, LEFT ^ mask]
            .into_iter()
            .enumerate()
        {
            let offset = 16 + (index * 3 + operation) * 16;
            expected[offset..offset + 16].copy_from_slice(&result.to_le_bytes());
        }
    }
    check(&module, &expected, 0);
}

#[test]
fn mixed_literal_calls_preserve_narrow_values_float_bits_and_vector_bits() {
    const NAN: u64 = 0x7ff8_0000_0000_0042;
    let bytes = vec![0xa5; 40];
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let helper = fixture
        .program
        .function::<fn(u8, f64, f64, V128) -> (V128, f64, f64, u8)>(
            move |body, (byte, first, second, vector)| body.return_((vector, first, second, byte)),
        );
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        // Pure expressions restore their typed literal payloads before call
        // arguments erase them and the result tuple binds each component again.
        let integer = Val::<I8>::from(255).add(2);
        let condition = Val::<I1>::from(false);
        let nan = condition.select::<F64>(0.0, f64::from_bits(NAN));
        let zero = condition.select::<F64>(1.0, -0.0);
        let vector = Val::<V128>::from(LEFT).xor(RIGHT);
        let (vector, nan, zero, integer) = body.call::<(V128, F64, F64, I8)>(
            helper,
            &[integer.into(), nan.into(), zero.into(), vector.into()],
        )?;
        body.store(memory, 0, vector)?;
        body.store(memory, 16, nan)?;
        body.store(memory, 24, zero)?;
        body.store(memory, 32, integer)?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    expected[..16].copy_from_slice(&(LEFT ^ RIGHT).to_le_bytes());
    expected[16..24].copy_from_slice(&NAN.to_le_bytes());
    expected[24..32].copy_from_slice(&0x8000_0000_0000_0000_u64.to_le_bytes());
    expected[32] = 1;
    check(&module, &expected, 0);
}

#[test]
fn vector_loads_keep_snapshots_across_overlapping_scalar_stores() {
    let mut bytes = vec![0xa5; 64];
    bytes[3..19].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let before = body.load::<V128>(memory, 3)?;
        body.store::<I32>(memory, 7, 0x11223344)?;
        let after = body.load::<V128>(memory, 3)?;
        body.store(memory, 27, before)?;
        body.store(memory, 45, after)?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    expected[7..11].copy_from_slice(&0x11223344_u32.to_le_bytes());
    expected[27..43].copy_from_slice(&LEFT.to_le_bytes());
    expected.copy_within(3..19, 45);
    check(&module, &expected, 0);
}

#[test]
fn vector_literals_require_vector_signatures() {
    for (argument, expected, actual) in [
        (Argument::from(LEFT), Type::I64, Type::V128),
        (Argument::from(0_u128), Type::I64, Type::V128),
        (Argument::from(1_u64), Type::V128, Type::I64),
        (Argument::from(1), Type::V128, Type::I32),
    ] {
        let mut program = Program::new();
        let function = program.raw_function(signature(&[], &[expected]), move |body| {
            body.return_(argument)
        });
        assert_eq!(
            {
                program.export("run", function).unwrap();
                program.compile().err()
            },
            Some(BuildError::TypeMismatch { expected, actual })
        );
    }
}

#[test]
fn vector_lanes_preserve_other_bits_and_use_low_byte_first_order() {
    use relocade_compiler::I64;
    let mut bytes = vec![0xa5; 240];
    bytes[..16].copy_from_slice(&LEFT.to_le_bytes());
    bytes[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let vector = body.load::<V128>(memory, 0)?;
        let word = body.load::<I32>(memory, 16)?;
        let qword = body.load::<I64>(memory, 24)?;
        for lane in 0_u32..4 {
            body.store(memory, 32 + lane * 16, vector.replace_lane(lane, &word))?;
            body.store(memory, 128 + lane * 4, vector.extract_lane::<I32>(lane))?;
        }
        for lane in 0_u32..2 {
            body.store(memory, 96 + lane * 16, vector.replace_lane(lane, &qword))?;
            body.store(memory, 144 + lane * 8, vector.extract_lane::<I64>(lane))?;
        }
        let literal = Val::<V128>::from(LEFT);
        body.store(memory, 160, literal.replace_lane::<I32>(3, 0x11223344))?;
        body.store(
            memory,
            176,
            literal.replace_lane::<I64>(0, 0x88776655_44332211_u64),
        )?;
        body.store(memory, 192, literal.extract_lane::<I32>(3))?;
        body.store(memory, 200, literal.extract_lane::<I64>(1))?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    for lane in 0..4 {
        let mut vector = LEFT.to_le_bytes();
        vector[lane * 4..lane * 4 + 4].copy_from_slice(&RIGHT.to_le_bytes()[..4]);
        expected[32 + lane * 16..48 + lane * 16].copy_from_slice(&vector);
    }
    for lane in 0..2 {
        let mut vector = LEFT.to_le_bytes();
        vector[lane * 8..lane * 8 + 8].copy_from_slice(&RIGHT.to_le_bytes()[8..]);
        expected[96 + lane * 16..112 + lane * 16].copy_from_slice(&vector);
    }
    expected[128..144].copy_from_slice(&LEFT.to_le_bytes());
    expected[144..160].copy_from_slice(&LEFT.to_le_bytes());
    expected[160..176].copy_from_slice(&LEFT.to_le_bytes());
    expected[172..176].copy_from_slice(&0x11223344_u32.to_le_bytes());
    expected[176..192].copy_from_slice(&LEFT.to_le_bytes());
    expected[176..184].copy_from_slice(&0x88776655_44332211_u64.to_le_bytes());
    expected[192..196].copy_from_slice(&LEFT.to_le_bytes()[12..]);
    expected[200..208].copy_from_slice(&LEFT.to_le_bytes()[8..]);
    check(&module, &expected, 0);
}
