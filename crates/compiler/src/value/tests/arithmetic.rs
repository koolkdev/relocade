use crate::{
    BuildError, IntType, MemoryImport, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};

#[test]
fn equivalent_constant_offsets_share_values_at_each_logical_width() {
    fn check<T: IntType>() {
        let mut program = Program::new();
        let function = program.declare(Signature {
            parameters: vec![T::TYPE],
            results: vec![T::TYPE],
        });
        let body = program.define(function).unwrap();
        let input = body.parameter::<T>(0).unwrap();
        let minus_four = Val::<T>::from(0).sub(4);
        let highest_bit = Val::<T>::literal(1u64 << (T::TYPE.bits() - 1));
        for (actual, expected) in [
            (input.sub(4), input.add(minus_four)),
            (Val::<T>::from(7).add(input.sub(3)), input.add(4)),
            (input.add(2).add(3).sub(5), input.clone()),
            (
                input.add(Val::<T>::literal(T::TYPE.mask())).add(1),
                input.clone(),
            ),
            (input.sub(&highest_bit), input.add(highest_bit)),
        ] {
            assert!(actual.same_expression(&expected), "{:?}", T::TYPE);
        }
        body.return_(input).unwrap();
        program.compile().unwrap();
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn cancelling_offsets_retain_the_original_operand_scope() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    let mut body = program.define(function).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    let mut retained = None;
    body.if_(false, |mut child| {
        let offset = child.load::<I32>(memory, 0)?.and(0).add(4);
        let result = input.add(offset).sub(4);
        assert!(result.same_expression(&input));
        retained = Some(child.value(result)?);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        body.value(retained.unwrap()).err(),
        Some(BuildError::OutOfScope),
    );
    body.return_(input).unwrap();
    program.compile().unwrap();
}
