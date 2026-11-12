use super::*;

#[test]
fn assumed_bits_fold_without_emitting_their_read_or_check() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[5, 7, 0, 0]);
    let module = fixture.function(&[], &[Type::I32], move |mut body| {
        let state = body.load::<I32>(memory, 0)?;
        body.assume(state.and(0xff).eq(5))?;
        assert_eq!(body.known_bits(state.and(0xff))?, Some(5));
        assert_eq!(body.constant_bits(state.and(0xff))?, None);
        body.return_(state.and(0xff).add(1))
    });
    assert_eq!(module.instantiate().call::<i32>(()), Ok(6));
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32Load { .. } | Operator::If { .. }
        )),
        0
    );
}

#[test]
fn assumptions_preserve_unknown_bits_and_new_memory_values() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[5, 7, 0, 0]);
    let module = fixture.function(&[], &[Type::I32, Type::I32], move |mut body| {
        let before = body.load::<I32>(memory, 0)?;
        body.assume(before.and(0xff).eq(5))?;
        body.store::<I32>(memory, 0, 9)?;
        let after = body.load::<I32>(memory, 0)?;
        assert_eq!(body.known_bits(before.and(0xff))?, Some(5));
        assert_eq!(body.known_bits(&before)?, None);
        assert_eq!(body.known_bits(&after)?, None);
        body.return_((before, after))
    });
    assert_eq!(module.instantiate().call::<(i32, i32)>(()), Ok((0x705, 9)));
}

#[test]
fn assumptions_do_not_reach_a_bypassing_path_or_earlier_effects() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    let module = fixture.function(&[Type::I1, Type::I32], &[Type::I32], move |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let value = body.parameter::<I32>(1)?;
        body.store(memory, 0, &value)?;
        body.block::<()>(|mut path, exit| {
            path.branch_if(skip, &exit, ())?;
            path.assume(value.eq(5))?;
            assert_eq!(path.known_bits(&value)?, Some(5));
            path.store(memory, 0, value.add(1))
        })?;
        assert_eq!(body.known_bits(&value)?, None);
        let result = body.load::<I32>(memory, 0)?;
        body.return_(result)
    });
    assert_eq!(module.instantiate().call::<i32>((1, 7)), Ok(7));
    assert_eq!(module.instantiate().call::<i32>((0, 5)), Ok(6));
}

#[test]
fn an_assumption_about_a_loop_value_does_not_reach_the_next_iteration() {
    let module = Fixture::new().function(&[], &[Type::I32], move |mut body| {
        body.loop_::<(I32, I1), ()>((0, false), |mut iteration, labels, (count, flag)| {
            iteration.if_(count.eq(1), |done| done.return_(flag.select::<I32>(27, 99)))?;
            iteration.assume(flag.eq(false))?;
            iteration.branch(&labels.again, (count.add(1), flag.eq(false)))
        })?;
        body.return_(99)
    });
    assert_eq!(module.instantiate().call::<i32>(()), Ok(27));
}
