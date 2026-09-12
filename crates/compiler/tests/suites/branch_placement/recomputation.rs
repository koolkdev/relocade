use super::*;

fn guard_and_continuation() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let masked = body.parameter::<I32>(1)?.and(0xff);
        body.if_(enabled, |mut arm| arm.store(state, 0, &masked))?;
        body.store(state, 4, &masked)?;
        body.return_(masked)
    })
}

#[test]
fn a_guard_and_its_continuation_each_compute_a_shared_mask() {
    let module = guard_and_continuation();
    let events = inspect(module.bytes());
    let branch = events.iter().position(|&event| event == Event::If).unwrap();
    let end = events
        .iter()
        .position(|&event| event == Event::End)
        .unwrap();
    assert!(!events[..branch].contains(&Event::And));
    assert_eq!(
        events[branch..end]
            .iter()
            .filter(|&&event| event == Event::And)
            .count(),
        1
    );
    assert_eq!(
        events[end..]
            .iter()
            .filter(|&&event| event == Event::And)
            .count(),
        1
    );
    for (enabled, input, result, expected) in [
        (0, 0x1234, 52, [0xa5, 0xa5, 0xa5, 0xa5, 52, 0, 0, 0]),
        (1, 0x1234, 52, [52, 0, 0, 0, 52, 0, 0, 0]),
        (0, -1, 255, [0xa5, 0xa5, 0xa5, 0xa5, 255, 0, 0, 0]),
        (1, -1, 255, [255, 0, 0, 0, 255, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((enabled, input)), Ok(result));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

fn block_exit_and_continuation() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let sum = body.parameter::<I32>(1)?.add(7);
        body.block::<()>(|mut block, success| {
            block.if_(skip, |arm| arm.branch(&success, ()))?;
            block.store(state, 0, &sum)?;
            block.return_(&sum)
        })?;
        body.store(state, 4, &sum)?;
        body.return_(sum)
    })
}

#[test]
fn a_block_suffix_and_its_continuation_each_compute_a_shared_sum() {
    let module = block_exit_and_continuation();
    let events = inspect(module.bytes());
    let guard_end = events
        .iter()
        .position(|&event| event == Event::End)
        .unwrap();
    let block_end = events
        .iter()
        .position(|&event| event == Event::BlockEnd)
        .unwrap();
    assert!(!events[..guard_end].contains(&Event::Add));
    assert_eq!(
        events[guard_end..block_end]
            .iter()
            .filter(|&&event| event == Event::Add)
            .count(),
        1
    );
    assert_eq!(
        events[block_end..]
            .iter()
            .filter(|&&event| event == Event::Add)
            .count(),
        1
    );
    // The outward branch skips the suffix capture; the parent use must still
    // initialize its own value. The suffix returns from the whole function.
    for (skip, input, result, expected) in [
        (0, 9, 16, [16, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]),
        (1, 9, 16, [0xa5, 0xa5, 0xa5, 0xa5, 16, 0, 0, 0]),
        (0, -7, 0, [0, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]),
        (1, -7, 0, [0xa5, 0xa5, 0xa5, 0xa5, 0, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((skip, input)), Ok(result));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

#[test]
fn three_demand_regions_retain_one_shared_calculation() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            let shared = body.parameter::<I32>(2)?.xor(0x55);
            body.if_(first, |mut arm| arm.store(state, 0, &shared))?;
            body.if_(second, |mut arm| arm.store(state, 4, &shared))?;
            body.return_(shared)
        },
    );
    let events = inspect(module.bytes());
    let branch = events.iter().position(|&event| event == Event::If).unwrap();
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Xor).count(),
        1
    );
    assert!(events[..branch].contains(&Event::Xor));
    for (first, second, expected) in [
        (0, 0, [0; 8]),
        (1, 0, [82, 0, 0, 0, 0, 0, 0, 0]),
        (0, 1, [0, 0, 0, 0, 82, 0, 0, 0]),
        (1, 1, [82, 0, 0, 0, 82, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((first, second, 7)), Ok(82));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

#[test]
fn repeating_a_cheap_result_preserves_its_shared_expensive_operand() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let enabled = body.parameter::<I1>(0)?;
            let left = body.parameter::<I32>(1)?;
            let right = body.parameter::<I32>(2)?;
            let derived = left.mul(right).xor(1);
            body.if_(enabled, |mut arm| arm.store(state, 0, &derived))?;
            body.store(state, 4, &derived)?;
            body.return_(derived)
        },
    );
    let events = inspect(module.bytes());
    let branch = events.iter().position(|&event| event == Event::If).unwrap();
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Mul).count(),
        1
    );
    assert!(events[..branch].contains(&Event::Mul));
    assert!(!events[..branch].contains(&Event::Xor));
    assert_eq!(xor_counts_on_paths(&events, &mut 0), [2, 1]);
    for (enabled, expected) in [
        (0, [0, 0, 0, 0, 34, 0, 0, 0]),
        (1, [34, 0, 0, 0, 34, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((enabled, 5, 7)), Ok(34));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

fn repeated_input_diamonds(depth: usize) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let mut shared = body.parameter::<I32>(1)?;
        for marker in 1..=depth {
            let fork = shared.xor(marker as u32);
            shared = fork.add(&fork);
        }
        body.if_(enabled, |mut arm| arm.store(state, 0, &shared))?;
        body.store(state, 4, &shared)?;
        body.return_(shared)
    })
}

#[test]
fn repeated_input_diamonds_have_two_copies_per_node_and_linear_growth() {
    for depth in [1, 4, 16] {
        let events = inspect(repeated_input_diamonds(depth).bytes());
        let branch = events.iter().position(|&event| event == Event::If).unwrap();
        let end = events
            .iter()
            .position(|&event| event == Event::End)
            .unwrap();
        assert!(!events[..branch]
            .iter()
            .any(|event| matches!(event, Event::Xor | Event::Add)));
        for region in [&events[branch..end], &events[end..]] {
            let arithmetic: Vec<_> = region
                .iter()
                .copied()
                .filter(|event| matches!(event, Event::Xor | Event::Add))
                .collect();
            // Each marker identifies one XOR node. Its repeated inputs merge in
            // one ADD, so cloning the expression tree would violate this bound.
            assert_eq!(arithmetic, [Event::Xor, Event::Add].repeat(depth));
            let markers: Vec<_> = region
                .windows(2)
                .filter_map(|pair| match pair {
                    [Event::Constant(marker), Event::Xor] => Some(*marker),
                    _ => None,
                })
                .collect();
            assert_eq!(markers, (1..=depth as i32).collect::<Vec<_>>());
        }
    }
    let module = repeated_input_diamonds(3);
    for (enabled, expected) in [
        (0, [0, 0, 0, 0, 62, 0, 0, 0]),
        (1, [62, 0, 0, 0, 62, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        // Input 7 gives 12, then 28, then 62 at the three merges.
        assert_eq!(instance.call::<i32>((enabled, 7)), Ok(62));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}
