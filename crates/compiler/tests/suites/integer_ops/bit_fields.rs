use super::{inspect, Fixture};
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{AtLeast, IntType, Type, Val, I1, I32, I64, I8};

fn rejoined<T: IntType>(count: u32, low_mask: u64, high_mask: u64) -> TestModule
where
    I64: AtLeast<T>,
{
    Fixture::new().expression(&[T::TYPE], |body| {
        let input = body.parameter::<T>(0).unwrap();
        let low = input.and(Val::<I64>::from(low_mask).truncate::<T>());
        let high = input
            .unsigned()
            .shr(count)
            .and(Val::<I64>::from(high_mask).truncate::<T>())
            .shl(count);
        low.or(high)
    })
}

#[test]
fn rejoining_masked_fields_preserves_holes_and_modulo_shift_counts() {
    fn check<T: IntType>()
    where
        I64: AtLeast<T>,
    {
        let width = if T::TYPE == Type::I64 { 64 } else { 32 };
        for count in [0, 1, 8, 16, 31, 32, 33, 63, 64, 65] {
            for (low_mask, high_mask) in [(0xff, 0xff), (0x55, 0xaaaa_5555), (0xffff, u64::MAX)] {
                let module = rejoined::<T>(count, low_mask, high_mask);
                assert_eq!(inspect(module.bytes()).shifts, 0);
                let mut instance = module.instantiate();
                for input in [0, 0x1234_5678_9abc_def0, 0x8000_0000_8000_0001, u64::MAX] {
                    let shift = count % width;
                    // Observe each destination bit independently of the folded mask.
                    let mut expected = 0_u64;
                    for bit in 0..width {
                        if (low_mask >> bit) & 1 != 0
                            || (bit >= shift && (high_mask >> (bit - shift)) & 1 != 0)
                        {
                            expected |= input & (1 << bit);
                        }
                    }
                    let (input, expected) = if width == 64 {
                        (Value::I64(input as i64), Value::I64(expected as i64))
                    } else {
                        (Value::I32(input as i32), Value::I32(expected as i32))
                    };
                    assert_eq!(
                        instance.call_values("run", &[input]).unwrap(),
                        vec![expected]
                    );
                }
            }
        }
    }
    check::<I32>();
    check::<I64>();
}

#[test]
fn different_sources_and_changed_bit_positions_are_not_rejoined() {
    let module = Fixture::new().function(&[Type::I32; 2], &[Type::I32; 3], |body| {
        let a = body.parameter::<I32>(0)?;
        let b = body.parameter::<I32>(1)?;
        body.return_((
            a.and(0xff).or(b.unsigned().shr(8).and(0xff).shl(8)),
            a.and(0xff).or(a.unsigned().shr(8).and(0xff).shl(9)),
            a.and(0xff).or(a
                .unsigned()
                .extend::<I64>()
                .unsigned()
                .shr(24)
                .truncate::<I32>()
                .shl(8)),
        ))
    });
    assert_eq!(
        module
            .instantiate()
            .call::<(i32, i32, i32)>((0x1234_5678, 0x7654_3210))
            .unwrap(),
        (0x3278, 0xac78, 0x1278)
    );
}

#[test]
fn rejoining_respects_masks_before_extraction_and_after_restoration() {
    let module = Fixture::new().expression(&[Type::I32], |body| {
        let input = body.parameter::<I32>(0).unwrap();
        let high = input
            .and(0xf0ff)
            .unsigned()
            .shr(8)
            .and(0xff)
            .shl(8)
            .and(0x3000);
        input.and(0xff).or(high)
    });
    assert_eq!(inspect(module.bytes()).shifts, 0);
    let mut instance = module.instantiate();
    for (input, expected) in [(-1, 0x30ff), (0x1234_5678, 0x1078), (0xff00, 0x3000)] {
        assert_eq!(instance.call::<i32>(input).unwrap(), expected);
    }
}

fn specialized_fields() -> TestModule {
    Fixture::new().function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let low = input.truncate::<I8>().unsigned().extend::<I32>();
        let high = input
            .unsigned()
            .shr(8)
            .truncate::<I8>()
            .unsigned()
            .extend::<I32>();
        let result = low.or(high.shl(enabled.select(8, 1)));
        body.if_(enabled, |arm| arm.return_(result))?;
        body.return_(0)
    })
}

#[test]
fn rejoining_type_views_refolds_after_path_specialization() {
    let module = specialized_fields();
    assert_eq!(inspect(module.bytes()).shifts, 0);
    for input in [0, -1, 0x1234_5678] {
        assert_eq!(
            module.instantiate().call::<i32>((input, 1)).unwrap(),
            input & 0xffff
        );
        assert_eq!(module.instantiate().call::<i32>((input, 0)).unwrap(), 0);
    }
}

#[test]
fn specialized_rejoining_masks_dirty_narrow_arithmetic_before_extraction() {
    let module = Fixture::new().function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?.truncate::<I8>().add(1);
        let enabled = body.parameter::<I1>(1)?;
        let low = input.and(0xf).unsigned().extend::<I32>();
        let high = input
            .unsigned()
            .shr(enabled.select(4, 5))
            .unsigned()
            .extend::<I32>()
            .shl(4);
        body.if_(enabled, |arm| arm.return_(low.or(high)))?;
        body.return_(0)
    });
    assert_eq!(inspect(module.bytes()).shifts, 0);
    for (input, expected) in [(0, 1), (0xff, 0), (0x100, 1), (0x1234_5678, 0x79), (-1, 0)] {
        assert_eq!(
            module.instantiate().call::<i32>((input, 1)).unwrap(),
            expected
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn rejoined_fields_execute_in_v8() {
    let module = rejoined::<I64>(40, 0xff, 0x55);
    assert_eq!(
        module.run_v8(&Input::call("run", &[Value::I64(-1)])),
        Observation::returned(&[Value::I64(0x5500_0000_00ff)])
    );
    let module = specialized_fields();
    assert_eq!(
        module.run_v8(&Input::call(
            "run",
            &[Value::I32(0x1234_5678), Value::I32(1)]
        )),
        Observation::returned(&[Value::I32(0x5678)])
    );
}
