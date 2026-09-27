use super::{plan, Instruction, LocalOp};
use crate::{
    integer::BinaryOp, memory::Location, module::Types, Expression, FunctionKind, Mem,
    MemoryImport, Program, Signature, Type, I32,
};
use wasm_encoder::{BlockType, ValType};

#[test]
fn a_complete_schedule_keeps_the_guard_before_division_and_its_store() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "host".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I32],
            },
            |mut body| {
                let numerator = body.parameter::<I32>(0)?;
                let denominator = body.parameter::<I32>(1)?;
                body.if_(denominator.eq(0), |exit| exit.return_(99))?;
                let quotient = numerator.unsigned().div(denominator);
                body.store::<I32>(memory, 8, &quotient)?;
                body.return_(quotient)
            },
        )
        .unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        unreachable!("the test defined its function body")
    };
    let schedule = plan(body, &[], &mut Types::default());
    drop(program);

    // All control, value and effect ordering is inspectable without the source
    // body or an encoder. Both uses of the quotient share the scheduled local.
    assert_eq!(schedule.local_types, [ValType::I32]);
    assert!(matches!(
        schedule.instructions.as_slice(),
        [
            Instruction::Parameter(1),
            Instruction::Expression {
                expression: Expression::ZeroTest { nonzero: false, .. },
                ..
            },
            Instruction::If(BlockType::Empty),
            Instruction::Constant {
                ty: Type::I32,
                bits: 99
            },
            Instruction::Return,
            Instruction::End,
            Instruction::Constant {
                ty: Type::I32,
                bits: 0
            },
            Instruction::Parameter(0),
            Instruction::Parameter(1),
            Instruction::Expression {
                result_type: Type::I32,
                expression: Expression::Binary {
                    operator: BinaryOp::DivUnsigned,
                    left: Type::I32,
                    right: Type::I32
                }
            },
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Tee
            },
            Instruction::Store(Location {
                memory: Mem(0),
                base: (),
                offset: 8,
                bytes: 4
            }),
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Get
            },
            Instruction::Return,
        ]
    ));
}
