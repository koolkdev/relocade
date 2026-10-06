//! Share surviving scalar work across control-flow exits.

use super::*;
use crate::wasm::{Input, Observation};
use wasm86_compiler::{I64, I8};

fn count(module: &TestModule, predicate: impl Fn(&Operator<'_>) -> bool) -> usize {
    Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
        .filter(&predicate)
        .count()
}

fn accumulator(depth: u32) -> TestModule {
    Fixture::new().function(&[Type::I32; 3], &[Type::I32], |mut body| {
        let stop = body.parameter::<I32>(0)?;
        let input = body.parameter::<I32>(1)?;
        let mut value = body.parameter::<I32>(2)?;
        for step in 0..depth {
            value = value.xor(input.add(step)).and(0x8000);
            body.if_(stop.eq(step), |arm| arm.return_(value.or(step)))?;
        }
        body.return_(value)
    })
}

fn accumulator_results(v8: bool) {
    for depth in [1, 4, 16, 32] {
        let module = accumulator(depth);
        assert_eq!(
            count(&module, |op| matches!(op, Operator::I32Xor)),
            depth as usize
        );
        for (input, seed) in [(0, 0), (0x7fff_u32, 0xffff_ffff), (0xffff_fff0, 0x8000)] {
            for stop in 0..=depth {
                let mut expected = seed;
                for step in 0..depth {
                    expected = (expected ^ input.wrapping_add(step)) & 0x8000;
                    if stop == step {
                        expected |= step;
                        break;
                    }
                }
                let args = [
                    Value::I32(stop as i32),
                    Value::I32(input as i32),
                    Value::I32(seed as i32),
                ];
                let expected = [Value::I32(expected as i32)];
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

#[test]
fn successive_exits_share_each_accumulator_update_once() {
    accumulator_results(false);
}

#[test]
fn shared_narrow_values_retain_their_full_wasm_carrier() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    let module = fixture.function(&[Type::I1, Type::I32], &[Type::I64], |mut body| {
        let flag = body.parameter::<I1>(0)?;
        let value = body.parameter::<I32>(1)?.truncate::<I8>().add(1).xor(0x80);
        body.if_else(
            flag,
            |mut arm| arm.store(memory, 0, value.signed().extend::<I32>()),
            |mut arm| arm.store(memory, 4, value.unsigned().extend::<I32>()),
        )?;
        body.return_(value.signed().extend::<I64>())
    });
    for flag in [0, 1] {
        for (input, signed, unsigned) in [(255, -128_i64, 128_u32), (127, 0, 0), (-2, 127, 127)] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i64>((flag, input)).unwrap(), signed);
            let stored = if flag == 1 { signed as u32 } else { unsigned };
            let offset = if flag == 1 { 0 } else { 4 };
            assert_eq!(
                &instance.memory("state")[offset..offset + 4],
                &stored.to_le_bytes()
            );
        }
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn surviving_bitwise_sharing_executes_in_v8() {
    accumulator_results(true);
}
