use super::*;

fn mixed_results() -> TestModule {
    let mut fixture = Fixture::new();
    let helper = fixture
        .program
        .function(
            signature(
                &[Type::I32],
                &[Type::I1, Type::I8, Type::I16, Type::I32, Type::I64],
            ),
            |body| {
                let input = body.parameter::<I32>(0)?;
                // These additions leave dirty upper carrier bits for narrow results.
                body.return_((
                    input.truncate::<I1>().add(1),
                    (
                        input.truncate::<I8>().add(1),
                        input.truncate::<I16>().add(1),
                    ),
                    input.add(1),
                    input.unsigned().extend::<I64>().add(1_u64 << 63),
                ))
            },
        )
        .unwrap();
    fixture.program.export("helper", helper).unwrap();
    fixture.function(
        &[Type::I32],
        &[
            Type::I64,
            Type::I32,
            Type::I1,
            Type::I8,
            Type::I16,
            Type::I32,
            Type::I1,
        ],
        |mut body| {
            let input = body.parameter::<I32>(0)?;
            let (bit, (byte, word), dword, wide) =
                body.call::<(I1, (I8, I16), I32, I64)>(helper, &[input.into()])?;
            body.return_((wide, &dword, &bit, byte, word, &dword, &bit))
        },
    )
}

#[test]
fn mixed_logical_results_normalize_at_function_boundaries_and_keep_their_order() {
    let module = mixed_results();
    assert_eq!(
        inspect(&module, "helper").results,
        [
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I64
        ]
    );
    let code = inspect(&module, "run");
    assert_eq!(code.calls, 1);
    assert_eq!(
        code.results,
        [
            ValType::I64,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ]
    );
    for (input, bit, byte, word, dword) in [
        (u32::MAX, 0, 0, 0, 0),
        (0x1234_ff00, 1, 1, 0xff01, 0x1234_ff01),
        (0x8000_ffff, 0, 0, 0, 0x8001_0000_u32),
    ] {
        let wide = i64::MIN + i64::from(input);
        let mut instance = module.instantiate();
        assert_eq!(
            instance
                .call_export::<(i32, i32, i32, i32, i64)>("helper", input as i32)
                .unwrap(),
            (bit, byte, word, dword as i32, wide)
        );
        assert_eq!(
            instance
                .call_values("run", &[Value::I32(input as i32)])
                .unwrap(),
            [
                Value::I64(wide),
                Value::I32(dword as i32),
                Value::I32(bit),
                Value::I32(byte),
                Value::I32(word),
                Value::I32(dword as i32),
                Value::I32(bit),
            ]
        );
    }
}

fn array_results<const N: usize>() -> TestModule {
    let mut fixture = Fixture::new();
    let result_types = vec![Type::I1; N];
    let helper = fixture
        .program
        .function(signature(&[Type::I32], &result_types), |body| {
            let input = body.parameter::<I32>(0)?;
            let values = (0..N)
                .map(|index| input.eq(index as u32))
                .collect::<Vec<_>>();
            body.return_(values)
        })
        .unwrap();
    fixture.function(&[Type::I32], &result_types, |mut body| {
        let input = body.parameter::<I32>(0)?;
        let values = body.call::<[I1; N]>(helper, &[input.into()])?;
        body.return_(values)
    })
}

#[test]
fn array_results_and_vector_returns_cover_empty_scalar_and_many_boolean_values() {
    fn check<const N: usize>() {
        let module = array_results::<N>();
        let code = inspect(&module, "run");
        assert_eq!(code.results, vec![ValType::I32; N]);
        assert_eq!(code.calls, usize::from(N != 0));
        for input in 0..=N {
            assert_eq!(
                module
                    .instantiate()
                    .call_values("run", &[Value::I32(input as i32)])
                    .unwrap(),
                (0..N)
                    .map(|index| Value::I32(i32::from(index == input)))
                    .collect::<Vec<_>>()
            );
        }
    }
    check::<0>();
    check::<1>();
    check::<6>();
    check::<9>();
}

const IMPORTED_RESULTS: [Value; 6] = [
    Value::I32(0x1234_5678),
    Value::I64(-2),
    Value::I32(1),
    Value::I32(255),
    Value::I64(i64::MIN),
    Value::I32(65535),
];

fn imported_results() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0xa5, 0x5a]);
    let target = fixture.callback(
        "receive",
        signature(
            &[Type::I8],
            &[
                Type::I32,
                Type::I64,
                Type::I1,
                Type::I8,
                Type::I64,
                Type::I16,
            ],
        ),
        &IMPORTED_RESULTS,
    );
    fixture.function(
        &[Type::I8],
        &[Type::I1, Type::I64, Type::I1, Type::I64],
        |mut body| {
            let input = body.parameter::<I8>(0)?.add(1);
            body.store(memory, 0, &input)?;
            let (_first, _second, bit, _fourth, wide, _last) =
                body.call::<(I32, I64, I1, I8, I64, I16)>(target, &[input.into()])?;
            body.store::<I8>(memory, 0, 7)?;
            body.return_((&bit, &wide, &bit, &wide))
        },
    )
}

#[test]
fn imported_results_share_one_call_when_middle_components_are_reused() {
    let module = imported_results();
    let code = inspect(&module, "run");
    assert_eq!(code.calls, 1);
    assert_eq!(code.drops, 4);
    let mut instance = module.instantiate();
    assert_eq!(
        instance.call::<(i32, i64, i32, i64)>(255).unwrap(),
        (1, i64::MIN, 1, i64::MIN)
    );
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(0)])
            .with_memories(&[MemoryBytes::new("state", &[0, 0x5a])])]
    );
    assert_eq!(&instance.memory("state")[..2], &[7, 0x5a]);
}

fn forward_tail_results() -> TestModule {
    let mut fixture = Fixture::new();
    let shape = signature(&[Type::I32], &[Type::I1, Type::I8, Type::I64]);
    let run = fixture.program.declare(shape.clone());
    let helper = fixture.program.declare(shape);
    let body = fixture.program.define(run).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    body.tail_call(helper, &[input.into()]).unwrap();
    let body = fixture.program.define(helper).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    body.return_((input.eq(255), input.truncate::<I8>().add(1), u64::MAX))
        .unwrap();
    fixture.finish(run)
}

#[test]
fn forward_declared_tail_targets_return_the_complete_logical_result_list() {
    let module = forward_tail_results();
    let code = inspect(&module, "run");
    assert_eq!(code.calls, 0);
    assert_eq!(code.tails, 1);
    assert_eq!(code.results, [ValType::I32, ValType::I32, ValType::I64]);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<(i32, i32, i64)>(255).unwrap(), (1, 0, -1));
    assert_eq!(instance.call::<(i32, i32, i64)>(7).unwrap(), (0, 8, -1));
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn typed_multi_results_execute_in_v8_without_packing_their_carriers() {
    assert_eq!(
        mixed_results().run_v8(&Input::call("run", &[Value::I32(-1)])),
        Observation::returned(&[
            Value::I64(i64::MIN + i64::from(u32::MAX)),
            Value::I32(0),
            Value::I32(0),
            Value::I32(0),
            Value::I32(0),
            Value::I32(0),
            Value::I32(0),
        ])
    );
    assert_eq!(
        array_results::<6>().run_v8(&Input::call("run", &[Value::I32(3)])),
        Observation::returned(&[0, 0, 0, 1, 0, 0].map(Value::I32))
    );
    assert_eq!(
        imported_results().run_v8(
            &Input::call("run", &[Value::I32(255)])
                .with_memories(&[MemoryBytes::new("state", &[0xa5, 0x5a])])
                .with_callbacks(&[Callback::new("receive", &IMPORTED_RESULTS)])
        ),
        Observation::returned(&[
            Value::I32(1),
            Value::I64(i64::MIN),
            Value::I32(1),
            Value::I64(i64::MIN),
        ])
        .with_callbacks(&[Call::new("receive", &[Value::I32(0)])
            .with_memories(&[MemoryBytes::new("state", &[0, 0x5a])])])
        .with_memories(&[MemoryBytes::new("state", &[7, 0x5a])])
    );
    assert_eq!(
        forward_tail_results().run_v8(&Input::call("run", &[Value::I32(255)])),
        Observation::returned(&[Value::I32(1), Value::I32(0), Value::I64(-1)])
    );
}
