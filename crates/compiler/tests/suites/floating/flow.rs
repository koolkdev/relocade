//! Floating encodings survive ordinary memory, calls and control-flow values.

use super::*;
use crate::wasm::MemoryBytes;

fn transport(v8: bool) {
    let module = Fixture::new().function(&[Type::I64, Type::I1], &[Type::I64; 5], |body| {
        let bits = body.parameter::<I64>(0)?;
        let choose = body.parameter::<I1>(1)?;
        let value = Val::<F64>::from_bits(&bits);
        // Raw floating bits 1 and 0 are not the numerical integer values 1 and 0.
        let tiny = choose.select(Val::<F64>::from_bits(1_u64), 0.0);
        let zero = choose.select::<F64>(-0.0, 0.0);
        body.return_([
            value.to_bits(),
            value.neg().to_bits(),
            value.abs().to_bits(),
            tiny.to_bits(),
            zero.to_bits(),
        ])
    });
    for bits in [
        0,
        SIGN,
        1,
        SIGN | 1,
        INFINITY,
        0xfff8_1234_5678_9abc,
        0x7ff0_0000_0000_0042,
    ] {
        let constants = Fixture::new().function(&[], &[Type::I64; 3], |body| {
            let value = Val::<F64>::from_bits(bits);
            body.return_([
                value.to_bits(),
                value.neg().to_bits(),
                value.abs().to_bits(),
            ])
        });
        check(
            &constants,
            &[],
            &[bits, bits ^ SIGN, bits & !SIGN].map(|bits| Value::I64(bits as i64)),
            v8,
        );
        for choose in [0, 1] {
            let expected = [
                bits,
                bits ^ SIGN,
                bits & !SIGN,
                choose as u64,
                (choose as u64) << 63,
            ];
            check(
                &module,
                &[Value::I64(bits as i64), Value::I32(choose)],
                &expected.map(|bits| Value::I64(bits as i64)),
                v8,
            );
        }
    }
}

fn mixed_calls_and_loops(v8: bool) {
    let mut fixture = Fixture::new();
    let helper = fixture
        .program
        .function(
            signature(
                &[Type::F64, Type::I64, Type::I32],
                &[Type::I64, Type::F64, Type::I32],
            ),
            |body| {
                let values = (
                    body.parameter::<I64>(1)?,
                    body.parameter::<F64>(0)?.neg(),
                    body.parameter::<I32>(2)?,
                );
                body.return_(values)
            },
        )
        .unwrap();
    let module = fixture.function(&[Type::I64, Type::I32], &[Type::I64; 3], |mut body| {
        let raw = body.parameter::<I64>(0)?;
        let count = body.parameter::<I32>(1)?;
        let (wide, first, count) = body.call::<(I64, F64, I32)>(
            helper,
            &[
                Val::<F64>::from_bits(&raw).into(),
                (&raw).into(),
                count.into(),
            ],
        )?;
        let second = body.if_value::<F64>(
            count.eq(0),
            |arm| arm.yield_(-0.0),
            |arm| arm.yield_(Val::<F64>::from_bits(1_u64)),
        )?;
        let (first, second, wide) = body.loop_::<(I32, F64, F64, I64), (F64, F64, I64)>(
            (count, first, second, wide),
            |mut iteration, labels, (remaining, first, second, wide)| {
                iteration.if_(remaining.eq(0), |done| {
                    done.branch(&labels.exit, (&first, &second, &wide))
                })?;
                iteration.branch(
                    &labels.again,
                    (remaining.sub(1), second, first, wide.add(3)),
                )
            },
        )?;
        body.return_([first.to_bits(), second.to_bits(), wide])
    });
    for bits in [SIGN, 1.25_f64.to_bits(), 0xfff0_0000_0000_0042] {
        for count in [0, 1, 2, 5] {
            let mut pair = [bits ^ SIGN, if count == 0 { SIGN } else { 1 }];
            if count % 2 != 0 {
                pair.swap(0, 1);
            }
            let expected = [pair[0], pair[1], bits.wrapping_add(3 * count as u64)];
            check(
                &module,
                &[Value::I64(bits as i64), Value::I32(count)],
                &expected.map(|bits| Value::I64(bits as i64)),
                v8,
            );
        }
    }
}

fn memory_snapshots(v8: bool) {
    let original = 0xfff0_0000_0000_0042_u64;
    let replacement = SIGN;
    let mut bytes = vec![0xa5; 24];
    bytes[3..11].copy_from_slice(&original.to_le_bytes());
    let input = Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &bytes)]);
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[], &[Type::I64; 2], |mut body| {
        let before = body.load::<F64>(memory, 3)?;
        body.store::<I64>(memory, 3, replacement)?;
        let after = body.load::<F64>(memory, 3)?;
        body.store(memory, 11, &before)?;
        body.return_([before.to_bits(), after.to_bits()])
    });
    bytes[3..11].copy_from_slice(&replacement.to_le_bytes());
    bytes[11..19].copy_from_slice(&original.to_le_bytes());
    let expected = [Value::I64(original as i64), Value::I64(replacement as i64)];
    if v8 {
        assert_eq!(
            module.run_v8(&input),
            Observation::returned(&expected).with_memories(&[MemoryBytes::new("state", &bytes)])
        );
    } else {
        let mut instance = module.instantiate();
        assert_eq!(instance.call_values("run", &[]).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..bytes.len()], bytes);
    }
}

#[test]
fn floating_transport_preserves_zero_signs_and_nan_payloads() {
    transport(false);
}

#[test]
fn mixed_scalar_calls_joins_and_backedges_keep_their_carriers() {
    mixed_calls_and_loops(false);
}

#[test]
fn floating_loads_keep_their_snapshot_across_integer_stores() {
    memory_snapshots(false);
}

#[test]
fn floating_function_boundaries_accept_native_host_values() {
    let module = Fixture::new().function(
        &[Type::I32, Type::F64, Type::I64],
        &[Type::F64, Type::I64, Type::I32],
        |body| {
            let values = (
                body.parameter::<F64>(1)?.add(0.5),
                body.parameter::<I64>(2)?.add(1),
                body.parameter::<I32>(0)?.add(2),
            );
            body.return_(values)
        },
    );
    let result = module
        .instantiate()
        .call::<(f64, i64, i32)>((7_i32, 1.25_f64, 0x100000001_i64))
        .unwrap();
    assert_eq!(result, (1.75, 0x100000002, 9));
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_floating_transport_calls_control_and_memory() {
    transport(true);
    mixed_calls_and_loops(true);
    memory_snapshots(true);
}
