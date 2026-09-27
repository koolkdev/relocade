use std::collections::HashMap;

use super::{Instruction, LocalOp, Scheduler};
use crate::{
    body::{Block, BlockTree, Body, Invocation, Operation, Site, Value, ValueDefinition},
    integer::BinaryOp,
    memory::Location,
    module::Types,
    Expression, Func, Mem, Type,
};

fn body(values: &[(Type, ValueDefinition)], operations: Vec<Operation>) -> Body {
    Body {
        values: values
            .iter()
            .map(|&(ty, definition)| Value { ty, definition })
            .collect(),
        block: Block {
            id: 0,
            operations,
            terminal: None,
        },
    }
}

fn scheduler<'a>(
    body: &'a Body,
    types: &'a mut Types,
    slots: Vec<Option<usize>>,
    captures: HashMap<Site, Vec<usize>>,
) -> Scheduler<'a> {
    let local_types = body
        .values
        .iter()
        .zip(&slots)
        .filter_map(|(value, slot)| slot.map(|_| crate::emit::wasm_type(value.ty)))
        .collect();
    Scheduler {
        body,
        blocks: BlockTree::new(&body.block),
        effects: &[],
        types,
        labels: Vec::new(),
        slots,
        captures,
        available: vec![false; body.values.len()],
        instructions: Vec::new(),
        local_types,
    }
}

#[test]
fn select_operands_share_results_before_any_lowering() {
    use Instruction::*;
    let body = body(
        &[
            (Type::I32, ValueDefinition::Parameter(0)),
            (Type::I32, ValueDefinition::Constant(7)),
            (
                Type::I32,
                ValueDefinition::Expression(crate::Expression::Binary {
                    operator: BinaryOp::Add,
                    left: 0,
                    right: 1,
                }),
            ),
            (Type::I32, ValueDefinition::Parameter(1)),
            (
                Type::I1,
                ValueDefinition::Expression(crate::Expression::ZeroTest {
                    input: 3,
                    nonzero: true,
                }),
            ),
            (
                Type::I32,
                ValueDefinition::Expression(crate::Expression::Binary {
                    operator: BinaryOp::Sub,
                    left: 2,
                    right: 1,
                }),
            ),
            (
                Type::I32,
                ValueDefinition::Expression(crate::Expression::Select {
                    condition: 4,
                    when_true: 2,
                    when_false: 5,
                }),
            ),
        ],
        vec![],
    );
    let mut types = Types::default();
    let mut planner = scheduler(
        &body,
        &mut types,
        vec![None, None, Some(0), None, None, None, None],
        HashMap::new(),
    );
    let before = planner.available.clone();
    planner.values([6]);
    let plan = std::mem::take(&mut planner.instructions);
    // True and false operands precede the condition. The second operand reuses
    // the first one's saved result even though no plan step has been lowered.
    assert!(matches!(
        plan.as_slice(),
        [
            Parameter(0),
            Constant {
                ty: Type::I32,
                bits: 7
            },
            Expression {
                expression: crate::Expression::Binary {
                    operator: BinaryOp::Add,
                    ..
                },
                ..
            },
            Local {
                slot: 0,
                operation: LocalOp::Tee
            },
            Local {
                slot: 0,
                operation: LocalOp::Get
            },
            Constant { bits: 7, .. },
            Expression {
                expression: crate::Expression::Binary {
                    operator: BinaryOp::Sub,
                    ..
                },
                ..
            },
            Parameter(1),
            Expression {
                expression: crate::Expression::Select { .. },
                ..
            },
        ]
    ));
    planner.instructions.clear();
    planner.values([2]);
    assert!(matches!(
        planner.instructions.as_slice(),
        [Local {
            slot: 0,
            operation: LocalOp::Get
        }]
    ));
    planner.available = before;
    planner.instructions.clear();
    planner.values([2]);
    assert!(matches!(
        planner.instructions.as_slice(),
        [
            Parameter(0),
            Constant { bits: 7, .. },
            Expression { .. },
            Local {
                operation: LocalOp::Tee,
                ..
            }
        ]
    ));
}

#[test]
fn captures_save_without_leaving_an_operand_or_repeating_the_read() {
    let site = Site { block: 0, index: 0 };
    let location = Location {
        memory: Mem(0),
        base: 0,
        offset: 8,
        bytes: 4,
    };
    let body = body(
        &[
            (Type::I32, ValueDefinition::Parameter(0)),
            (Type::I32, ValueDefinition::Load { site }),
        ],
        vec![Operation::Load { location }],
    );
    let mut types = Types::default();
    let mut planner = scheduler(
        &body,
        &mut types,
        vec![None, Some(0)],
        HashMap::from([(site, vec![1])]),
    );
    planner.instructions.clear();
    planner.captures(site);
    assert!(matches!(
        planner.instructions.as_slice(),
        [
            Instruction::Parameter(0),
            Instruction::Load { .. },
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Set
            }
        ]
    ));
    planner.instructions.clear();
    planner.captures(site);
    assert!(planner.instructions.is_empty());
    planner.instructions.clear();
    planner.values([1]);
    assert!(matches!(
        planner.instructions.as_slice(),
        [Instruction::Local {
            slot: 0,
            operation: LocalOp::Get
        }]
    ));
}

#[test]
fn signed_load_cover_keeps_the_access_and_saves_the_widened_result() {
    let site = Site { block: 0, index: 0 };
    let location = Location {
        memory: Mem(2),
        base: 0,
        offset: 19,
        bytes: 1,
    };
    let body = body(
        &[
            (Type::I32, ValueDefinition::Parameter(0)),
            (Type::I8, ValueDefinition::Load { site }),
            (
                Type::I32,
                ValueDefinition::Expression(Expression::SignExtend { input: 1 }),
            ),
            (
                Type::I64,
                ValueDefinition::Expression(Expression::SignExtend { input: 2 }),
            ),
        ],
        vec![Operation::Load { location }],
    );
    let mut types = Types::default();
    let mut planner = scheduler(
        &body,
        &mut types,
        vec![None, None, None, Some(0)],
        HashMap::new(),
    );
    planner.instructions.clear();
    planner.values([3, 3]);
    assert!(matches!(
        planner.instructions.as_slice(),
        [
            Instruction::Parameter(0),
            Instruction::Load {
                location: Location {
                    memory: Mem(2),
                    base: (),
                    offset: 19,
                    bytes: 1
                },
                result_type: Type::I64,
                signed: true
            },
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Tee
            },
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Get
            },
        ]
    ));
}

#[test]
fn one_call_schedules_arguments_and_consumes_the_entire_result_tuple() {
    let site = Site { block: 0, index: 0 };
    let body = body(
        &[
            (Type::I32, ValueDefinition::Parameter(0)),
            (Type::I32, ValueDefinition::Parameter(1)),
            (
                Type::I32,
                ValueDefinition::OperationResult { site, component: 0 },
            ),
            (
                Type::I64,
                ValueDefinition::OperationResult { site, component: 1 },
            ),
            (
                Type::I32,
                ValueDefinition::OperationResult { site, component: 2 },
            ),
        ],
        vec![Operation::Call {
            invocation: Invocation {
                target: Func(2),
                arguments: vec![0, 1],
            },
            outputs: vec![2, 3, 4],
        }],
    );
    let mut types = Types::default();
    let mut planner = scheduler(
        &body,
        &mut types,
        vec![None, None, Some(0), None, Some(1)],
        HashMap::new(),
    );
    planner.instructions.clear();
    planner.values([4, 2]);
    assert!(matches!(
        planner.instructions.as_slice(),
        [
            Instruction::Parameter(0),
            Instruction::Parameter(1),
            Instruction::Call(Func(2)),
            Instruction::Local {
                slot: 1,
                operation: LocalOp::Set
            },
            Instruction::Drop,
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Set
            },
            Instruction::Local {
                slot: 1,
                operation: LocalOp::Get
            },
            Instruction::Local {
                slot: 0,
                operation: LocalOp::Get
            },
        ]
    ));
    planner.instructions.clear();
    planner.call(site);
    assert!(planner.instructions.is_empty());
}

#[test]
fn zero_test_cover_keeps_logical_input_types_for_opcode_selection() {
    let body = body(
        &[
            (Type::I32, ValueDefinition::Parameter(0)),
            (
                Type::I8,
                ValueDefinition::Expression(Expression::Normalize { input: 0 }),
            ),
            (
                Type::I1,
                ValueDefinition::Expression(Expression::ZeroTest {
                    input: 1,
                    nonzero: false,
                }),
            ),
        ],
        vec![],
    );
    let mut types = Types::default();
    let mut planner = scheduler(&body, &mut types, vec![None; 3], HashMap::new());
    planner.instructions.clear();
    planner.values([2]);
    assert!(matches!(
        planner.instructions.as_slice(),
        [
            Instruction::Parameter(0),
            Instruction::Expression {
                result_type: Type::I32,
                expression: Expression::SignExtend { input: Type::I8 }
            },
            Instruction::Expression {
                result_type: Type::I1,
                expression: Expression::ZeroTest {
                    input: Type::I8,
                    nonzero: false
                }
            },
        ]
    ));
}
