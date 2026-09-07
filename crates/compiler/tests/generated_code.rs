use wasm86_compiler::{
    FunctionBuilder, IntType, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, ValType};

#[derive(Debug, Default, Eq, PartialEq)]
struct CodeShape {
    additions: usize,
    locals: u32,
    local_writes: usize,
    masks: usize,
    mask_before_addition: bool,
}

fn shape<T: IntType>(
    parameters: &[Type],
    build: impl FnOnce(&FunctionBuilder<'_>) -> Val<T>,
) -> CodeShape {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: parameters.to_vec(),
        result: T::TYPE,
    });
    let body = program.define(function).unwrap();
    let result = build(&body);
    body.return_(&result).unwrap();
    program.export("test", function).unwrap();
    let bytes = program.compile().unwrap();
    let mut shape = CodeShape::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            for local in body.get_locals_reader().unwrap() {
                shape.locals += local.unwrap().0;
            }
            let mut operators = body.get_operators_reader().unwrap();
            while !operators.eof() {
                match operators.read().unwrap() {
                    Operator::I32Add | Operator::I64Add => {
                        shape.additions += 1;
                        shape.mask_before_addition |= shape.masks != 0;
                    }
                    Operator::I32And => shape.masks += 1,
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                        shape.local_writes += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    assert_eq!(bodies, 1, "the exported function has one encoded body");
    shape
}

#[test]
fn shared_additions_are_evaluated_once() {
    fn check<T: IntType>() {
        let ty = T::TYPE;
        let actual = shape(&[ty], |body| {
            let parameter = body.parameter::<T>(0).unwrap();
            let a = parameter.add(1);
            let _dead = a.add(9);
            let b = a.add(2);
            let b_again = a.add(2);
            b.add(&b_again)
        });
        assert_eq!(
            actual,
            CodeShape {
                additions: 3,
                locals: 1,
                local_writes: 1,
                ..CodeShape::default()
            },
            "{ty:?}"
        );
    }
    check::<I32>();
    check::<I64>();
}

#[test]
fn repeated_parameters_and_constants_stay_inline() {
    fn check<T: IntType>() {
        let ty = T::TYPE;
        let actual = shape(&[ty, ty], |body| {
            let first = body.parameter::<T>(0).unwrap();
            let second = body.parameter::<T>(1).unwrap();
            let one = body.value::<T>(1).unwrap();
            let a = first.add(&one);
            let b = second.add(&one);
            a.add(&b).add(&first)
        });
        assert_eq!(
            actual,
            CodeShape {
                additions: 4,
                locals: 0,
                local_writes: 0,
                ..CodeShape::default()
            },
            "{ty:?}"
        );
    }
    check::<I32>();
    check::<I64>();
}

#[test]
fn constant_sums_emit_no_add_instructions() {
    fn check<T: IntType>() {
        let ty = T::TYPE;
        let actual = shape(&[], |body| body.value::<T>(17).unwrap().add(25));
        assert_eq!(actual, CodeShape::default(), "{ty:?}");
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn narrow_additions_are_masked_once_at_the_return() {
    fn check<T: IntType>() {
        let actual = shape(&[T::TYPE], |body| {
            let a = body.parameter::<T>(0).unwrap().add(1);
            let _dead = a.add(9);
            let b = a.add(1);
            let b_again = a.add(1);
            b.add(&b_again)
        });
        assert_eq!(
            actual,
            CodeShape {
                additions: 3,
                locals: 1,
                local_writes: 1,
                masks: 1,
                mask_before_addition: false,
            },
            "{:?}",
            T::TYPE
        );
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
}

#[test]
fn narrow_parameters_need_no_mask_when_forwarded() {
    fn check<T: IntType>() {
        let actual = shape(&[T::TYPE], |body| body.parameter::<T>(0).unwrap().add(0));
        assert_eq!(actual, CodeShape::default(), "{:?}", T::TYPE);
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
}

#[test]
fn logical_signatures_share_their_wasm_integer_representation() {
    fn identity<T: IntType>(program: &mut Program, name: &str) {
        let function = program.declare(Signature {
            parameters: vec![T::TYPE],
            result: T::TYPE,
        });
        let body = program.define(function).unwrap();
        let result = body.parameter::<T>(0).unwrap();
        body.return_(&result).unwrap();
        program.export(name, function).unwrap();
    }
    let mut program = Program::new();
    identity::<I1>(&mut program, "i1");
    identity::<I8>(&mut program, "i8");
    identity::<I16>(&mut program, "i16");
    identity::<I32>(&mut program, "i32");
    identity::<I64>(&mut program, "i64");
    let bytes = program.compile().unwrap();
    let mut types = Vec::new();
    let mut indices = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                types.extend(section.into_iter_err_on_gc_types().map(Result::unwrap));
            }
            Payload::FunctionSection(section) => {
                indices.extend(section.into_iter().map(Result::unwrap));
            }
            _ => {}
        }
    }
    assert_eq!(types.len(), 2);
    assert_eq!(indices.len(), 5);
    assert!(indices[..4].iter().all(|index| *index == indices[0]));
    assert_ne!(indices[0], indices[4]);
    for (index, ty) in [(indices[0], ValType::I32), (indices[4], ValType::I64)] {
        assert_eq!(types[index as usize].params(), &[ty]);
        assert_eq!(types[index as usize].results(), &[ty]);
    }
}
