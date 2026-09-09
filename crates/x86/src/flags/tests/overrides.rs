use super::{
    assert_return, step, ArithmeticKind, CompiledModule, Condition, FlagSource, StatusFlag,
};
use wasm86_compiler::{AtLeast, MemoryInt, Program, Signature, Type, I1, I16, I32, I8};

#[test]
fn replacing_carry_changes_unsigned_conditions_and_retains_result_flags() {
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
    {
        let mut program = Program::new();
        for (name, condition) in QUERIES {
            let function = program
                .function(
                    Signature {
                        parameters: vec![T::TYPE, T::TYPE, Type::I1],
                        result: Some(Type::I32),
                    },
                    |body| {
                        let left = body.parameter::<T>(0)?;
                        let right = body.parameter::<T>(1)?;
                        let carry = body.parameter::<I1>(2)?;
                        let source = FlagSource::arithmetic(ArithmeticKind::Sub, left, right)
                            .with_flag(StatusFlag::CF, carry);
                        let result = match condition {
                            Some(condition) => {
                                source.condition(condition).unsigned().extend::<I32>()
                            }
                            None => source.result().unsigned().extend::<I32>(),
                        };
                        body.return_(result)
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
        let mut module = step::TestModule::new(&module);
        for (left, right, carry, expected) in [
            (2, 1, 1, [1, 1, 0, 1, 0, 0, 0]),
            (0, 1, 0, [maximum, 0, 1, 0, 1, 0, 1]),
            (1, 1, 0, [0, 0, 1, 1, 0, 1, 0]),
            (minimum, 1, 0, [positive_maximum, 0, 1, 0, 1, 0, 1]),
        ] {
            for ((entry, _), expected) in QUERIES.into_iter().zip(expected) {
                module.entry = entry.into();
                assert_return(&module, &[left, right, carry], expected);
            }
        }
    }
}

#[test]
fn replacing_another_flag_retains_an_earlier_override() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I8],
                result: Some(Type::I1),
            },
            |body| {
                let source = FlagSource::Logic {
                    result: body.parameter::<I8>(0)?.add(1),
                }
                .with_flag(StatusFlag::ZF, true.into())
                .with_flag(StatusFlag::CF, false.into());
                body.return_(source.condition(Condition::BE))
            },
        )
        .unwrap();
    program.export("retained_zero", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "retained_zero".into(),
    });
    for input in [0, 14, 255, 254] {
        assert_return(&module, &[input], 1);
    }
}
