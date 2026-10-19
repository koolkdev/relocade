//! Compound counts expose shared primitives while retaining path and width contracts.
use crate::fixture::Fixture;
use crate::wasm::TestModule;
use wasm86_compiler::{AtLeast, IntType, Type, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload};

fn counts(module: &TestModule) -> (usize, usize) {
    let mut counts = (0, 0);
    for payload in Parser::new(0).parse_all(module.bytes()) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                match operator.unwrap() {
                    Operator::I32Clz => counts.0 += 1,
                    Operator::I32Ctz => counts.1 += 1,
                    _ => {}
                }
            }
        }
    }
    counts
}

fn shared_counts<T: IntType>(width: u32) -> TestModule
where
    I32: AtLeast<T>,
{
    Fixture::new().function(&[T::TYPE], &[Type::I32; 4], |body| {
        let input = body.parameter::<T>(0)?;
        let full = input.unsigned().extend::<I32>();
        body.return_((
            input.clz().unsigned().extend::<I32>(),
            full.clz(),
            input.ctz().unsigned().extend::<I32>(),
            full.or(1_u32 << width).ctz(),
        ))
    })
}

#[test]
fn narrow_and_full_width_counts_share_native_primitives() {
    for (module, width) in [
        (shared_counts::<I1>(1), 1),
        (shared_counts::<I8>(8), 8),
        (shared_counts::<I16>(16), 16),
    ] {
        assert_eq!(counts(&module), (1, 1));
        let mut instance = module.instantiate();
        for input in [0_u32, 1, 1 << (width - 1), (1 << width) - 1] {
            let leading = input.leading_zeros();
            let trailing = input.trailing_zeros().min(width);
            assert_eq!(
                instance
                    .call::<(i32, i32, i32, i32)>((input as i32,))
                    .unwrap(),
                (
                    (leading - (32 - width)) as i32,
                    leading as i32,
                    trailing as i32,
                    trailing as i32
                )
            );
        }
    }
}

fn joined_count(constant_arm: bool, trailing: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0]);
    fixture.function(&[Type::I1, Type::I8], &[Type::I64], |mut body| {
        let mode = body.parameter::<I1>(0)?;
        let input = body.parameter::<I8>(1)?;
        let input = if constant_arm {
            mode.select(0, &input)
        } else {
            mode.select(&input, input.add(1))
        };
        let count = if trailing { input.ctz() } else { input.clz() };
        body.if_else(
            &mode,
            |mut arm| arm.store(memory, 0, &count),
            |mut arm| arm.store(memory, 0, &count),
        )?;
        body.return_(count.unsigned().extend::<I64>())
    })
}

#[test]
fn lowered_counts_reuse_branch_executions_and_constant_predecessors() {
    for constant_arm in [false, true] {
        for trailing in [false, true] {
            let module = joined_count(constant_arm, trailing);
            let expected_count = if constant_arm { 1 } else { 2 };
            let (leading, trailing_count) = counts(&module);
            let actual = if trailing {
                assert_eq!(leading, 0);
                trailing_count
            } else {
                assert_eq!(trailing_count, 0);
                leading
            };
            assert!(
                (1..=expected_count).contains(&actual),
                "no extra count after the join"
            );
            let mut instance = module.instantiate();
            for mode in [0, 1] {
                for input in [0_u8, 1, 0x80, 0xff] {
                    let input_value = if constant_arm && mode != 0 {
                        0
                    } else if !constant_arm && mode == 0 {
                        input.wrapping_add(1)
                    } else {
                        input
                    };
                    let expected = if trailing {
                        input_value.trailing_zeros()
                    } else {
                        input_value.leading_zeros()
                    };
                    assert_eq!(
                        instance.call::<i64>((mode, i32::from(input))).unwrap(),
                        i64::from(expected)
                    );
                    assert_eq!(instance.memory("state")[0], expected as u8);
                }
            }
        }
    }
}

#[test]
fn repeated_guards_reuse_the_lowered_count() {
    let module = Fixture::new().function(&[Type::I1, Type::I8], &[Type::I8], |mut body| {
        let mode = body.parameter::<I1>(0)?;
        let count = body.parameter::<I8>(1)?.clz();
        let first = body.if_value::<I8>(&mode, |arm| arm.yield_(&count), |arm| arm.yield_(0))?;
        let second = body.if_value::<I8>(
            &mode,
            |arm| arm.yield_(first.add(&count)),
            |arm| arm.yield_(0),
        )?;
        body.return_(second)
    });
    assert_eq!(counts(&module), (1, 0));
    let mut instance = module.instantiate();
    for (mode, input, expected) in [(0, 0, 0), (1, 0, 16), (1, 1, 14), (1, 128, 0)] {
        assert_eq!(instance.call::<i32>((mode, input)).unwrap(), expected);
    }
}

#[test]
fn lowering_does_not_restore_a_count_discarded_by_path_facts() {
    let module = Fixture::new().function(&[Type::I8], &[Type::I8], |mut body| {
        let input = body.parameter::<I8>(0)?;
        body.if_(input.ne(0), |arm| arm.return_(99))?;
        body.return_(input.clz().add(input.ctz()))
    });
    assert_eq!(counts(&module), (0, 0));
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>((0,)).unwrap(), 16);
    assert_eq!(instance.call::<i32>((1,)).unwrap(), 99);
}

#[test]
fn a_guarded_count_does_not_narrow_an_unguarded_primitive() {
    let module = Fixture::new().function(&[Type::I32, Type::I8], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let byte = body.parameter::<I8>(1)?;
        let guard = input.and(!0xfd_u32).eq(0);
        let guarded = input
            .and(0xfd)
            .truncate::<I8>()
            .clz()
            .unsigned()
            .extend::<I32>();
        let other = guard.select(0, byte).clz().unsigned().extend::<I32>();
        let unguarded = input
            .clz()
            .sub(24)
            .add(other)
            .truncate::<I8>()
            .unsigned()
            .extend::<I32>();
        body.if_(&guard, |arm| arm.return_(guarded))?;
        body.return_(unguarded)
    });
    let mut instance = module.instantiate();
    for (input, byte, expected) in [(256, 128, 255), (512, 128, 254), (0, 128, 8), (128, 128, 0)] {
        assert_eq!(instance.call::<i32>((input, byte)).unwrap(), expected);
    }
}

#[test]
fn common_count_primitives_can_remove_a_control_flow_edge() {
    let module = Fixture::new().function(&[Type::I8], &[Type::I32], |mut body| {
        let byte = body.parameter::<I8>(0)?;
        let leading = byte.clz().unsigned().extend::<I32>().add(24);
        let native = byte.unsigned().extend::<I32>().clz();
        body.if_(leading.ne(native), |arm| arm.return_(99))?;
        body.return_(7)
    });
    assert_eq!(counts(&module), (0, 0));
    let mut instance = module.instantiate();
    for input in [0, 1, 128, 255] {
        assert_eq!(instance.call::<i32>((input,)).unwrap(), 7);
    }
}
