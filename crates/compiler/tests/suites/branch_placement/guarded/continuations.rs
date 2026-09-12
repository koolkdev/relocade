use super::*;

const INITIAL: [u8; 16] = [7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn snapshot_with_continuation(source: Source) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    let reader = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            let value = body.load::<I32>(state, 0)?;
            body.return_(value)
        })
        .unwrap();
    let receive = fixture.callback("receive", signature(&[], &[Type::I32]), &[Value::I32(7)]);
    fixture.function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let choose_load = body.parameter::<I1>(1)?;
        let previous = match source {
            Source::Load => body.load::<I32>(state, 0)?,
            Source::ReadOnlyCall => body.call::<I32>(reader, &[])?,
            Source::Callback => body.call::<I32>(receive, &[])?,
            Source::Join => body.if_value::<I32>(
                choose_load,
                |mut arm| {
                    let value = arm.load::<I32>(state, 0)?;
                    arm.yield_(value)
                },
                |arm| arm.yield_(3),
            )?,
        };
        let derived = previous.add(10);
        body.if_(enabled, |mut arm| {
            arm.store::<I32>(state, 0, 9)?;
            arm.store(state, 4, &derived)
        })?;
        body.store::<I32>(state, 0, 11)?;
        body.store(state, 8, &derived)?;
        let fresh = body.load::<I32>(state, 0)?;
        body.store(state, 12, &fresh)?;
        body.return_(fresh)
    })
}

fn expected_state(enabled: i32, value: u8) -> [u8; 16] {
    [
        11,
        0,
        0,
        0,
        if enabled != 0 { value } else { 0 },
        0,
        0,
        0,
        value,
        0,
        0,
        0,
        11,
        0,
        0,
        0,
    ]
}

#[test]
fn child_and_main_calculations_preserve_snapshots_while_a_fresh_load_sees_the_write() {
    for source in [
        Source::Load,
        Source::ReadOnlyCall,
        Source::Callback,
        Source::Join,
    ] {
        let module = snapshot_with_continuation(source);
        let events = inspect(module.bytes());
        assert_eq!(
            events.iter().filter(|&&event| event == Event::Add).count(),
            2
        );
        let calls = usize::from(matches!(source, Source::ReadOnlyCall | Source::Callback));
        assert_eq!(
            events.iter().filter(|&&event| event == Event::Call).count(),
            calls
        );
        assert_eq!(
            events.iter().filter(|&&event| event == Event::Load).count(),
            2 - calls
        );
        let producer = if calls == 1 { Event::Call } else { Event::Load };
        let read = events.iter().position(|&event| event == producer).unwrap();
        let write = events
            .iter()
            .position(|&event| event == Event::Store)
            .unwrap();
        assert!(read < write, "{events:?}");
        // Join's selector contributes another If before the consuming guard.
        let guard = events
            .iter()
            .rposition(|&event| event == Event::If)
            .unwrap();
        let guard_end = guard
            + events[guard..]
                .iter()
                .position(|&event| event == Event::End)
                .unwrap();
        assert!(!events[..guard].contains(&Event::Add));
        assert_eq!(
            events[guard..guard_end]
                .iter()
                .filter(|&&event| event == Event::Add)
                .count(),
            1
        );
        assert_eq!(
            events[guard_end..]
                .iter()
                .filter(|&&event| event == Event::Add)
                .count(),
            1
        );
        for enabled in [0, 1] {
            for choose_load in if matches!(source, Source::Join) {
                &[0, 1][..]
            } else {
                &[1][..]
            } {
                let mut instance = module.instantiate();
                assert_eq!(instance.call::<i32>((enabled, *choose_load)), Ok(11));
                let value = if *choose_load == 0 { 13 } else { 17 };
                assert_eq!(
                    &instance.memory("state")[..16],
                    expected_state(enabled, value)
                );
                let calls = if matches!(source, Source::Callback) {
                    vec![Call::new("receive", &[])
                        .with_memories(&[MemoryBytes::new("state", &INITIAL)])]
                } else {
                    vec![]
                };
                assert_eq!(instance.callbacks(), calls);
            }
        }
    }
}

fn addressed_snapshot_with_block_exit() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?.add(4);
        let previous = body.load_at::<I32>(state, &address, 0)?;
        let derived = previous.xor(1);
        body.block::<()>(|mut block, success| {
            block.if_(skip, |arm| arm.branch(&success, ()))?;
            block.store_at::<I32>(state, &address, 0, 13)?;
            block.store(state, 0, &derived)?;
            let fresh = block.load_at::<I32>(state, &address, 0)?;
            block.return_(fresh)
        })?;
        body.store_at::<I32>(state, &address, 0, 15)?;
        body.store(state, 0, &derived)?;
        let fresh = body.load_at::<I32>(state, &address, 0)?;
        body.return_(fresh)
    })
}

#[test]
fn a_parent_snapshot_has_its_address_even_when_a_block_suffix_is_skipped() {
    let module = addressed_snapshot_with_block_exit();
    let events = inspect(module.bytes());
    let address = events
        .iter()
        .position(|&event| event == Event::Add)
        .unwrap();
    let read = events
        .iter()
        .position(|&event| event == Event::Load)
        .unwrap();
    let branch = events.iter().position(|&event| event == Event::If).unwrap();
    assert!(address < read && read < branch, "{events:?}");
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Load).count(),
        3
    );
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Xor).count(),
        2
    );
    for (skip, input, result, expected) in [
        (0, 0, 13, [4, 0, 0, 0, 13, 0, 0, 0, 6, 0, 0, 0]),
        (1, 0, 15, [4, 0, 0, 0, 15, 0, 0, 0, 6, 0, 0, 0]),
        (0, 4, 13, [7, 0, 0, 0, 5, 0, 0, 0, 13, 0, 0, 0]),
        (1, 4, 15, [7, 0, 0, 0, 5, 0, 0, 0, 15, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((skip, input)), Ok(result));
        assert_eq!(&instance.memory("state")[..12], expected);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn child_and_main_calculations_preserve_snapshots_and_fresh_reads_in_v8() {
    for source in [
        Source::Load,
        Source::ReadOnlyCall,
        Source::Callback,
        Source::Join,
    ] {
        let module = snapshot_with_continuation(source);
        for enabled in [0, 1] {
            for choose_load in if matches!(source, Source::Join) {
                &[0, 1][..]
            } else {
                &[1][..]
            } {
                let value = if *choose_load == 0 { 13 } else { 17 };
                let mut input =
                    Input::call("run", &[Value::I32(enabled), Value::I32(*choose_load)])
                        .with_memories(&[MemoryBytes::new("state", &INITIAL)]);
                let mut expected = Observation::returned(&[Value::I32(11)])
                    .with_memories(&[MemoryBytes::new("state", &expected_state(enabled, value))]);
                if matches!(source, Source::Callback) {
                    input = input.with_callbacks(&[Callback::new("receive", &[Value::I32(7)])]);
                    expected = expected.with_callbacks(&[Call::new("receive", &[])
                        .with_memories(&[MemoryBytes::new("state", &INITIAL)])]);
                }
                assert_eq!(module.run_v8(&input), expected);
            }
        }
    }
    let module = addressed_snapshot_with_block_exit();
    for (skip, result, memory) in [
        (0, 13, [4, 0, 0, 0, 13, 0, 0, 0, 6, 0, 0, 0]),
        (1, 15, [4, 0, 0, 0, 15, 0, 0, 0, 6, 0, 0, 0]),
    ] {
        let input = Input::call("run", &[Value::I32(skip), Value::I32(0)]).with_memories(&[
            MemoryBytes::new("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
        ]);
        assert_eq!(
            module.run_v8(&input),
            Observation::returned(&[Value::I32(result)])
                .with_memories(&[MemoryBytes::new("state", &memory)])
        );
    }
}
