//! XOR folding cancels values and combines masks without narrowing their carriers.

use super::*;
use crate::wasm::{Input, Observation, TestModule};
use wasm86_compiler::{IntType, I1};

fn xor_count(module: &TestModule) -> usize {
    Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
        .filter(|op| matches!(op, Operator::I32Xor | Operator::I64Xor))
        .count()
}

fn specialized<T: IntType>() -> TestModule {
    Fixture::new().function(&[T::TYPE, Type::I1], &[T::TYPE], |mut body| {
        let input = body.parameter::<T>(0)?;
        let choice = body.parameter::<I1>(1)?;
        let mask = choice.select::<T>(0x55, 0x33);
        let result = input.xor(0x55).xor(mask);
        body.if_(choice, |arm| arm.return_(&result))?;
        body.return_(result)
    })
}

fn specialized_results(v8: bool) {
    for (ty, module) in [
        (Type::I32, specialized::<I32>()),
        (Type::I64, specialized::<I64>()),
    ] {
        // The true branch cancels; the false branch toggles the combined 0x66 mask.
        assert_eq!(xor_count(&module), 1);
        for (input, combined) in [(0, 0x66), (0x66, 0), (-1, -103), (i64::MIN, i64::MIN + 102)] {
            for choice in [0, 1] {
                let value = |bits| {
                    if ty == Type::I64 {
                        Value::I64(bits)
                    } else {
                        Value::I32(bits as i32)
                    }
                };
                let args = [value(input), Value::I32(choice)];
                let expected = [value(if choice == 1 { input } else { combined })];
                if v8 {
                    assert_eq!(
                        module.run_v8(&Input::call("run", &args)),
                        Observation::returned(&expected)
                    );
                } else {
                    assert_eq!(
                        module.instantiate().call_values("run", &args).unwrap(),
                        expected
                    );
                }
            }
        }
    }
}

fn narrow_carriers() -> TestModule {
    Fixture::new().function(&[Type::I32, Type::I32], &[Type::I32, Type::I64], |body| {
        let input = body.parameter::<I32>(0)?.truncate::<I8>().add(1);
        let mask = body.parameter::<I32>(1)?.truncate::<I8>();
        let result = input.xor(&mask).xor(mask);
        body.return_((
            result.unsigned().extend::<I32>(),
            result.signed().extend::<I64>(),
        ))
    })
}

fn carrier_boundaries() -> TestModule {
    Fixture::new().function(&[Type::I64], &[Type::I64; 2], |body| {
        let input = body.parameter::<I64>(0)?;
        let narrow = input.xor(0x8000_0000_0000_0055_u64).truncate::<I32>();
        let widened = narrow
            .unsigned()
            .extend::<I64>()
            .xor(0x8000_0000_0000_0033_u64);
        let masked = input.and(0xff_u64).xor(0x55).xor(&input);
        body.return_((widened, masked))
    })
}

fn carrier_results(v8: bool) {
    let module = narrow_carriers();
    assert_eq!(xor_count(&module), 0);
    for (input, mask, unsigned, signed) in [
        (0, -1, 1, 1),
        (255, 0x100, 0, 0),
        (127, 0x80, 128, -128),
        (-2, 0x55, 255, -1),
    ] {
        let args = [Value::I32(input), Value::I32(mask)];
        let expected = [Value::I32(unsigned), Value::I64(signed)];
        if v8 {
            assert_eq!(
                module.run_v8(&Input::call("run", &args)),
                Observation::returned(&expected)
            );
        } else {
            assert_eq!(
                module.instantiate().call_values("run", &args).unwrap(),
                expected
            );
        }
    }
    let module = carrier_boundaries();
    for (input, first, second) in [
        (0_u64, 0x8000_0000_0000_0066_u64, 0x55_u64),
        (
            0xffff_ffff_1234_5678,
            0x8000_0000_1234_561e,
            0xffff_ffff_1234_5655,
        ),
        (u64::MAX, 0x8000_0000_ffff_ff99, 0xffff_ffff_ffff_ff55),
    ] {
        let args = [Value::I64(input as i64)];
        let expected = [Value::I64(first as i64), Value::I64(second as i64)];
        if v8 {
            assert_eq!(
                module.run_v8(&Input::call("run", &args)),
                Observation::returned(&expected)
            );
        } else {
            assert_eq!(
                module.instantiate().call_values("run", &args).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn path_specialization_cancels_and_combines_xor_masks() {
    specialized_results(false);
}

#[test]
fn xor_folding_preserves_narrow_and_changed_carriers() {
    carrier_results(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn folded_xor_executes_in_v8() {
    specialized_results(true);
    carrier_results(true);
}
