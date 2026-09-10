use super::*;

const INITIAL: [u8; 14] = [7, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a];

fn reader(fixture: &mut Fixture, memory: Mem) -> Func {
    fixture
        .program
        .function(signature(&[], &[Type::I32, Type::I32]), |mut body| {
            let first = body.load::<I32>(memory, 0)?;
            let second = body.load::<I32>(memory, 4)?;
            body.return_((first, second))
        })
        .unwrap()
}

fn snapshots(store_offset: u32, fresh: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &INITIAL);
    let read = reader(&mut fixture, memory);
    let results = vec![Type::I32; if fresh { 4 } else { 2 }];
    fixture.function(&[], &results, |mut body| {
        let (first, second) = body.call::<(I32, I32)>(read, &[])?;
        body.store::<I32>(memory, store_offset, 9)?;
        if fresh {
            let (third, fourth) = body.call::<(I32, I32)>(read, &[])?;
            body.return_((first, second, third, fourth))
        } else {
            body.return_((first, second))
        }
    })
}

#[test]
fn readonly_result_groups_cross_only_nonaliasing_writes() {
    for (offset, aliases) in [(0, true), (2, true), (4, true), (8, false)] {
        let module = snapshots(offset, false);
        let code = inspect(&module, "run");
        assert_eq!(code.calls, 1);
        let call = code
            .events
            .iter()
            .position(|event| *event == Event::Call)
            .unwrap();
        let store = code
            .events
            .iter()
            .position(|event| *event == Event::Store(u64::from(offset)))
            .unwrap();
        assert_eq!(call < store, aliases);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<(i32, i32)>(()).unwrap(), (7, 11));
        let mut expected = INITIAL;
        expected[offset as usize..offset as usize + 4].copy_from_slice(&9_u32.to_le_bytes());
        assert_eq!(&instance.memory("state")[..INITIAL.len()], &expected);
    }
}

#[test]
fn separate_invocations_keep_distinct_before_and_after_snapshot_groups() {
    let module = snapshots(0, true);
    assert_eq!(inspect(&module, "run").calls, 2);
    let mut instance = module.instantiate();
    assert_eq!(
        instance.call::<(i32, i32, i32, i32)>(()).unwrap(),
        (7, 11, 9, 11)
    );
    assert_eq!(
        &instance.memory("state")[..INITIAL.len()],
        &[9, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn an_early_component_use_captures_later_components_of_the_same_invocation() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &INITIAL);
    let read = reader(&mut fixture, memory);
    let module = fixture.function(&[], &[Type::I32], |mut body| {
        let (first, second) = body.call::<(I32, I32)>(read, &[])?;
        body.store(memory, 8, first)?;
        body.store::<I32>(memory, 4, 99)?;
        body.return_(second)
    });
    assert_eq!(inspect(&module, "run").calls, 1);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 11);
    assert_eq!(
        &instance.memory("state")[..INITIAL.len()],
        &[7, 0, 0, 0, 99, 0, 0, 0, 7, 0, 0, 0, 0xa5, 0x5a]
    );
}

fn sibling_uses() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &INITIAL);
    let read = reader(&mut fixture, memory);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let (first, second) = body.call::<(I32, I32)>(read, &[])?;
        let result = body.if_value::<I32>(
            choose,
            |mut arm| {
                arm.store::<I32>(memory, 0, 99)?;
                arm.yield_(first)
            },
            |mut arm| {
                arm.store::<I32>(memory, 4, 88)?;
                arm.yield_(second)
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn components_demanded_in_sibling_arms_share_one_snapshot_before_either_write() {
    let module = sibling_uses();
    let code = inspect(&module, "run");
    assert_eq!(code.calls, 1);
    let call = code
        .events
        .iter()
        .position(|event| *event == Event::Call)
        .unwrap();
    let branch = code
        .events
        .iter()
        .position(|event| *event == Event::If)
        .unwrap();
    assert!(call < branch);
    for (choose, returned, offset, value) in [(1, 7, 0, 99_u32), (0, 11, 4, 88)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(choose).unwrap(), returned);
        let mut expected = INITIAL;
        expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(&instance.memory("state")[..INITIAL.len()], &expected);
    }
}

#[test]
fn a_boolean_result_and_a_later_value_share_the_snapshot_before_the_guard() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &INITIAL);
    let helper = fixture
        .program
        .function(signature(&[], &[Type::I1, Type::I32]), |mut body| {
            let condition = body.load::<I32>(memory, 0)?.eq(7);
            let result = body.load::<I32>(memory, 4)?;
            body.return_((condition, result))
        })
        .unwrap();
    let module = fixture.function(&[], &[Type::I32, Type::I1], |mut body| {
        let (condition, result) = body.call::<(I1, I32)>(helper, &[])?;
        body.if_(&condition, |mut arm| arm.store::<I32>(memory, 4, 99))?;
        body.return_((result, condition))
    });
    assert_eq!(inspect(&module, "run").calls, 1);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<(i32, i32)>(()).unwrap(), (11, 1));
    assert_eq!(
        &instance.memory("state")[..INITIAL.len()],
        &[7, 0, 0, 0, 99, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn a_pure_invocation_needed_in_only_one_arm_stays_on_that_control_path() {
    let mut fixture = Fixture::new();
    let helper = fixture
        .program
        .function(
            signature(&[Type::I1], &[Type::I32, Type::I64]),
            |mut body| {
                let fail = body.parameter::<I1>(0)?;
                body.if_(fail, |arm| arm.trap())?;
                body.return_((13, 17_u64))
            },
        )
        .unwrap();
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let (first, _unused) = body.call::<(I32, I64)>(helper, &[true.into()])?;
        let result = body.if_value::<I32>(choose, |arm| arm.yield_(first), |arm| arm.yield_(7))?;
        body.return_(result)
    });
    let code = inspect(&module, "run");
    assert_eq!(code.calls, 1);
    let call = code
        .events
        .iter()
        .position(|event| *event == Event::Call)
        .unwrap();
    let branch = code
        .events
        .iter()
        .position(|event| *event == Event::If)
        .unwrap();
    assert!(call > branch);
    assert_eq!(module.instantiate().call::<i32>(0).unwrap(), 7);
    assert!(module.instantiate().call::<i32>(1).is_err());
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn multi_result_snapshot_groups_execute_across_writes_and_branches_in_v8() {
    assert_eq!(
        snapshots(0, true)
            .run_v8(&Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &INITIAL)])),
        Observation::returned(&[7, 11, 9, 11].map(Value::I32)).with_memories(&[MemoryBytes::new(
            "state",
            &[9, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a]
        ),])
    );
    for (choose, returned, offset, value) in [(1, 7, 0, 99_u32), (0, 11, 4, 88)] {
        let mut expected = INITIAL;
        expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(
            sibling_uses().run_v8(
                &Input::call("run", &[Value::I32(choose)])
                    .with_memories(&[MemoryBytes::new("state", &INITIAL)])
            ),
            Observation::returned(&[Value::I32(returned)])
                .with_memories(&[MemoryBytes::new("state", &expected)])
        );
    }
}
