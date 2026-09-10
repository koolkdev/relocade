use crate::alu::flags::{AnyFlagSource, Condition, FlagChange, FlagSource, StatusFlag};
use crate::alu::ArithmeticOp;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use wasm86_compiler::{AtLeast, MemoryInt, Program, Signature, Type, I1, I16, I32, I64, I8};

use super::super::fixture::{assert_result, initial_cpu};

#[test]
fn partial_carry_changes_unsigned_conditions_and_retains_subtraction_results() {
    const QUERIES: [(&str, Option<Condition>); 7] = [
        ("result", None),
        ("below", Some(Condition::B)),
        ("above_equal", Some(Condition::AE)),
        ("below_equal", Some(Condition::BE)),
        ("above", Some(Condition::A)),
        ("equal", Some(Condition::E)),
        ("signed_less", Some(Condition::L)),
    ];

    fn module<T: MemoryInt>() -> CompiledModule
    where
        I32: AtLeast<T>,
        I64: AtLeast<T>,
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        for (name, condition) in QUERIES {
            let function = program
                .function(
                    Signature {
                        parameters: vec![T::TYPE, T::TYPE, Type::I1],
                        results: vec![Type::I64],
                    },
                    |mut body| {
                        let mut state = State::new(&cpu);
                        let left = body.parameter::<T>(0)?;
                        let right = body.parameter::<T>(1)?;
                        let carry = body.parameter::<I1>(2)?;
                        let subtraction = ArithmeticOp::Subtract.apply(left, right);
                        let arithmetic_result = subtraction.result.unsigned().extend::<I32>();
                        state.set_flags(&mut body, subtraction.flags)?;
                        state
                            .set_flags(&mut body, FlagChange::partial([(StatusFlag::CF, carry)]))?;
                        let result = match condition {
                            Some(condition) => state
                                .condition(&mut body, condition)?
                                .unsigned()
                                .extend::<I32>(),
                            None => arithmetic_result,
                        };
                        body.return_(result.signed().extend::<I64>())
                    },
                )
                .unwrap();
            program.export(name, function).unwrap();
        }
        CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "result".into(),
        }
    }

    for (module, minimum, maximum, positive_maximum) in [
        (module::<I8>(), 128, 255, 127),
        (module::<I16>(), 32768, 65535, 32767),
        (module::<I32>(), i32::MIN, -1, i32::MAX),
    ] {
        let mut module = TestModule::new(&module);
        let initial = initial_cpu();
        for (left, right, carry, expected) in [
            (2, 1, 1, [1, 1, 0, 1, 0, 0, 0]),
            (0, 1, 0, [maximum, 0, 1, 0, 1, 0, 1]),
            (1, 1, 0, [0, 0, 1, 1, 0, 1, 0]),
            (minimum, 1, 0, [positive_maximum, 0, 1, 0, 1, 0, 1]),
        ] {
            for ((entry, _), expected) in QUERIES.into_iter().zip(expected) {
                module.entry = entry.into();
                assert_result(
                    &module,
                    &initial,
                    &[left, right, carry],
                    &initial,
                    i64::from(expected),
                );
            }
        }
    }
}

#[test]
fn changing_another_flag_retains_an_earlier_partial_change() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I8],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let source = FlagSource::Logic {
                    result: body.parameter::<I8>(0)?.add(1),
                };
                state.set_flags(&mut body, source)?;
                state.set_flags(
                    &mut body,
                    FlagChange::partial([(StatusFlag::ZF, true.into())]),
                )?;
                state.set_flags(
                    &mut body,
                    FlagChange::partial([(StatusFlag::CF, false.into())]),
                )?;
                let below_equal = state.condition(&mut body, Condition::BE)?;
                body.return_(below_equal.unsigned().extend::<I64>())
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let initial = initial_cpu();
    for input in [0, 14, 255, 254] {
        assert_result(&module, &initial, &[input], &initial, 1);
    }
}
