//! Returning guards leave calculations with the paths that use their results.
use crate::fixture::Fixture;
use wasm86_compiler::{Type, I1, I32, I8};

#[test]
fn returning_guards_keep_shared_calculations_with_their_consumers() {
    for (depth, expected) in [(0, 10), (1, 31), (4, 850)] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &[0; 4]);
        let module = fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
            let flag = body.parameter::<I1>(0)?;
            let input = body.parameter::<I32>(1)?;
            let mut value = input.unsigned().div(input.sub(29));
            for _ in 0..depth {
                value = value.mul(3).add(1);
            }
            let value = flag.select(value, 7);
            body.if_else(
                &flag,
                |mut arm| arm.store(memory, 0, &value),
                |arm| arm.return_(7),
            )?;
            body.return_(value)
        });
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((0, 29)), Ok(7));
        assert_eq!(&instance.memory("state")[..4], &[0; 4]);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((1, 32)), Ok(expected));
        assert_eq!(&instance.memory("state")[..4], &expected.to_le_bytes());
    }
}

#[test]
fn nested_returning_guards_preserve_results_and_publication() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let outer = body.parameter::<I1>(0)?;
            let inner = body.parameter::<I1>(1)?;
            let divisor = body.parameter::<I32>(2)?;
            let value = divisor.unsigned().div(&divisor).mul(3);
            body.if_else(
                outer,
                |mut arm| {
                    arm.if_else(
                        inner,
                        |mut taken| taken.store(memory, 0, &value),
                        |exit| exit.return_(7),
                    )?;
                    arm.store(memory, 4, &value)
                },
                |exit| exit.return_(9),
            )?;
            body.return_(value)
        },
    );
    for (outer, inner, divisor, expected, memory) in [
        (0, 0, 0, 9, [0; 8]),
        (1, 0, 0, 7, [0; 8]),
        (1, 1, 2, 3, [3, 0, 0, 0, 3, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((outer, inner, divisor)), Ok(expected));
        assert_eq!(&instance.memory("state")[..8], &memory);
    }
}

#[test]
fn a_nonzero_guard_preserves_quotients_and_remainders_across_returning_paths() {
    for positive_guard in [false, true] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &[0; 4]);
        let module = fixture.function(&[Type::I1, Type::I32], &[Type::I32; 2], |mut body| {
            let first = body.parameter::<I1>(0)?;
            let divisor = body.parameter::<I32>(1)?;
            let nonzero = if positive_guard {
                wasm86_compiler::Val::<I32>::from(0).unsigned().lt(&divisor)
            } else {
                divisor.ne(0)
            };
            body.if_(nonzero.eq(false), |arm| arm.return_((17, 19)))?;
            let numerator = divisor.add(101);
            let quotient = numerator.unsigned().div(&divisor);
            let remainder = numerator.unsigned().rem(&divisor);
            body.if_else(
                first,
                |mut arm| {
                    arm.store::<I32>(memory, 0, 1)?;
                    arm.return_((&quotient, &remainder))
                },
                |mut arm| {
                    arm.store::<I32>(memory, 0, 2)?;
                    arm.return_((&quotient, &remainder))
                },
            )?;
            body.trap()
        });
        for (divisor, expected) in [(0, (17, 19)), (3, (34, 2)), (20, (6, 1))] {
            for first in [0_i32, 1] {
                let mut instance = module.instantiate();
                assert_eq!(instance.call::<(i32, i32)>((first, divisor)), Ok(expected));
                let stored = if divisor == 0 { 0 } else { 2 - first as u8 };
                assert_eq!(&instance.memory("state")[..4], &[stored, 0, 0, 0]);
            }
        }
    }
}

#[test]
fn a_guard_observes_the_divisors_logical_width() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    let module = fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let first = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        let divisor = input.truncate::<I8>();
        body.if_(divisor.eq(0), |arm| arm.return_(17))?;
        let quotient = divisor
            .add(100)
            .unsigned()
            .div(&divisor)
            .unsigned()
            .extend::<I32>();
        body.if_else(
            first,
            |mut arm| {
                arm.store::<I32>(memory, 0, 1)?;
                arm.return_(&quotient)
            },
            |mut arm| {
                arm.store::<I32>(memory, 0, 2)?;
                arm.return_(&quotient)
            },
        )?;
        body.trap()
    });
    for first in [0_i32, 1] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((first, 0)), Ok(17));
        assert_eq!(&instance.memory("state")[..4], &[0; 4]);
        for (input, expected, stored) in [(1, 101, 2 - first as u8), (256, 17, 0)] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>((first, input)), Ok(expected));
            assert_eq!(&instance.memory("state")[..4], &[stored, 0, 0, 0]);
        }
    }
}
