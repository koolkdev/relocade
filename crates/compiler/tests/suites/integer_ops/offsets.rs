use crate::{
    fixture::Fixture,
    wasm::{Input, Observation, TestModule, Value},
};
use wasm86_compiler::{AtLeast, IntType, Type, I1, I16, I32, I64, I8};

fn check_result(module: &TestModule, input: Value, expected: &[Value], v8: bool) {
    let actual = if v8 {
        module.run_v8(&Input::call("run", &[input]))
    } else {
        Observation::returned(&module.instantiate().call_values("run", &[input]).unwrap())
    };
    assert_eq!(actual, Observation::returned(expected));
}

fn narrow<T: IntType>(cases: &[(i32, i64, i64)], v8: bool)
where
    I64: AtLeast<T>,
{
    let module = Fixture::new().function(&[T::TYPE], &[Type::I64, Type::I64], |body| {
        let value = body.parameter::<T>(0)?.sub(4).add(6).sub(1);
        body.return_((
            value.unsigned().extend::<I64>(),
            value.signed().extend::<I64>(),
        ))
    });
    for &(input, unsigned, signed) in cases {
        check_result(
            &module,
            Value::I32(input),
            &[Value::I64(unsigned), Value::I64(signed)],
            v8,
        );
    }
}

fn check_offsets(v8: bool) {
    narrow::<I1>(&[(0, 1, -1), (1, 0, 0)], v8);
    narrow::<I8>(&[(0, 1, 1), (127, 128, -128), (255, 0, 0)], v8);
    narrow::<I16>(&[(0, 1, 1), (32767, 32768, -32768), (65535, 0, 0)], v8);
    narrow::<I32>(
        &[(0, 1, 1), (2147483647, 2147483648, -2147483648), (-1, 0, 0)],
        v8,
    );
    let wide = Fixture::new().expression(&[Type::I64], |body| {
        body.parameter::<I64>(0)
            .unwrap()
            .sub(u64::MAX)
            .add(4)
            .sub(4)
    });
    for (input, expected) in [
        (0, 1),
        (-1, 0),
        (i64::MAX, i64::MIN),
        (i64::MIN, i64::MIN + 1),
    ] {
        check_result(&wide, Value::I64(input), &[Value::I64(expected)], v8);
    }
    let widened = Fixture::new().expression(&[Type::I8], |body| {
        body.parameter::<I8>(0)
            .unwrap()
            .add(1)
            .unsigned()
            .extend::<I32>()
            .add(255)
    });
    for (input, expected) in [(0, 256), (127, 383), (255, 255)] {
        check_result(&widened, Value::I32(input), &[Value::I32(expected)], v8);
    }
    let masked = Fixture::new().expression(&[Type::I32], |body| {
        body.parameter::<I32>(0).unwrap().add(4).and(255).sub(4)
    });
    for (input, expected) in [(0, 0), (252, -4), (253, -3), (255, -1)] {
        check_result(&masked, Value::I32(input), &[Value::I32(expected)], v8);
    }
    check_masked_offsets(v8);
    check_mask_widths(v8);
}

fn check_mask_widths(v8: bool) {
    let module = Fixture::new().function(&[Type::I64], &[Type::I64; 6], |body| {
        let input = body.parameter::<I64>(0)?;
        body.return_([
            input.and(0_u64),
            input.and(0x7fff_ffff_u64),
            input.and(0xffff_ffff_u64),
            input.and(0x1_ffff_ffff_u64),
            input.and(u64::MAX),
            input
                .truncate::<I32>()
                .unsigned()
                .extend::<I64>()
                .and(0x1_ffff_ffff_u64),
        ])
    });
    for (input, expected) in [
        (0, [0; 6]),
        (
            -1,
            [0, 0x7fff_ffff, 0xffff_ffff, 0x1_ffff_ffff, -1, 0xffff_ffff],
        ),
        (i64::MIN, [0, 0, 0, 0, i64::MIN, 0]),
        (0x1_0000_0001, [0, 1, 1, 0x1_0000_0001, 0x1_0000_0001, 1]),
        (
            i64::MAX,
            [
                0,
                0x7fff_ffff,
                0xffff_ffff,
                0x1_ffff_ffff,
                i64::MAX,
                0xffff_ffff,
            ],
        ),
    ] {
        check_result(&module, Value::I64(input), &expected.map(Value::I64), v8);
    }
}

fn check_masked_offsets(v8: bool) {
    let module = Fixture::new().function(&[Type::I32], &[Type::I32; 6], |body| {
        let input = body.parameter::<I32>(0)?;
        body.return_([
            input.add(7).and(7).add(1).and(7),
            input.add(5).and(15).add(6).and(7),
            input.add(5).and(5).add(6).and(7),
            input
                .add(120)
                .and(255)
                .truncate::<I8>()
                .signed()
                .extend::<I32>()
                .add(10)
                .and(511),
            input
                .add(65530)
                .and(65535)
                .truncate::<I8>()
                .unsigned()
                .extend::<I32>()
                .add(7)
                .and(511),
            input.add(5).and(7).sub(5).and(7),
        ])
    });
    for (input, expected) in [
        (0, [0, 3, 3, 130, 257, 0]),
        (1, [1, 4, 2, 131, 258, 1]),
        (7, [7, 2, 2, 137, 8, 7]),
        (8, [0, 3, 3, 394, 9, 0]),
        (127, [7, 2, 2, 1, 128, 7]),
        (255, [7, 2, 2, 129, 256, 7]),
        (256, [0, 3, 3, 130, 257, 0]),
        (i32::MAX, [7, 2, 2, 129, 256, 7]),
        (-1, [7, 2, 2, 129, 256, 7]),
    ] {
        check_result(&module, Value::I32(input), &expected.map(Value::I32), v8);
    }
    let wide = Fixture::new().expression(&[Type::I64], |body| {
        body.parameter::<I64>(0)
            .unwrap()
            .add(7)
            .and(i64::MAX as u64)
            .sub(7)
            .and(i64::MAX as u64)
    });
    for (input, expected) in [
        (0, 0),
        (-1, i64::MAX),
        (i64::MIN, 0),
        (i64::MIN + 5, 5),
        (i64::MAX, i64::MAX),
    ] {
        check_result(&wide, Value::I64(input), &[Value::I64(expected)], v8);
    }
}

#[test]
fn constant_offsets_wrap_at_logical_width_and_stop_at_observation_boundaries() {
    check_offsets(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_constant_offsets_wrap_at_logical_width_and_stop_at_observation_boundaries() {
    check_offsets(true);
}
