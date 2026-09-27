//! Instruction ordering and lowering of shared expression results.
use std::collections::HashMap;

use wasm_encoder::{Function, Instruction, ValType};

use crate::{
    body::{BlockTree, Body, Site, Target, ValueDefinition},
    effects::Effects,
    memory::Location,
    module::Types,
    place, Expression, Type,
};

mod branch;
mod calls;
mod control;
mod function;
mod integer;
mod loops;
mod memory;
mod switch;

use function::{FunctionEncoder, LocalOp};

pub(super) fn wasm_type(ty: Type) -> ValType {
    match ty {
        Type::I1 | Type::I8 | Type::I16 | Type::I32 => ValType::I32,
        Type::I64 => ValType::I64,
    }
}

enum Walk {
    Value(usize),
    Finish(usize),
    FinishLoad {
        result: usize,
        location: Location,
        signed: bool,
    },
    FinishCall(usize),
    FinishZero(usize, Option<Type>),
}

struct ControlLabel {
    target: Option<Target>,
    outputs: Vec<usize>,
}

struct Scheduler<'a> {
    body: &'a Body,
    blocks: BlockTree<'a>,
    memories: &'a [Option<u32>],
    functions: &'a [Option<u32>],
    effects: &'a [Effects],
    types: &'a mut Types,
    labels: Vec<ControlLabel>,
    slots: Vec<Option<usize>>,
    captures: HashMap<Site, Vec<usize>>,
    emitted: Vec<bool>,
    code: FunctionEncoder,
}

pub(super) fn encode(
    body: &Body,
    parameter_count: u32,
    memories: &[Option<u32>],
    functions: &[Option<u32>],
    effects: &[Effects],
    types: &mut Types,
) -> Function {
    let blocks = BlockTree::new(&body.block);
    let place::Placement {
        slots,
        captures,
        slot_types,
    } = place::plan(body, effects, &blocks);
    let mut scheduler = Scheduler {
        body,
        blocks,
        memories,
        functions,
        effects,
        types,
        labels: Vec::new(),
        slots,
        captures,
        emitted: vec![false; body.values.len()],
        code: FunctionEncoder::new(parameter_count, slot_types),
    };
    scheduler.block(&body.block, None);
    scheduler.code.finish()
}

impl Scheduler<'_> {
    fn completed(&mut self, id: usize, capture: bool) {
        if let Some(slot) = self.slots[id] {
            self.code
                .local(slot, if capture { LocalOp::Set } else { LocalOp::Tee });
        }
        self.emitted[id] = true;
    }

    fn value(&mut self, root: usize) {
        self.evaluate(root, false);
    }

    fn condition_input(&self, condition: usize) -> usize {
        let condition = place::representation(self.body, condition);
        let ValueDefinition::Expression(Expression::ZeroTest {
            input,
            nonzero: true,
        }) = self.body.values[condition].definition
        else {
            return condition;
        };
        // Wasm truth consumers accept any nonzero i32. ZeroTest's input is zero
        // exactly when its logical value is zero; saved Booleans remain zero or one.
        if self.slots[condition].is_none() && wasm_type(self.body.values[input].ty) == ValType::I32
        {
            input
        } else {
            condition
        }
    }

    fn emit_condition(&mut self, condition: usize, inverted: bool) {
        let condition = self.condition_input(condition);
        if inverted {
            if let ValueDefinition::Expression(Expression::ZeroTest {
                input,
                nonzero: false,
            }) = self.body.values[condition].definition
            {
                // Inverting an unshared i32 zero-test can use its operand as the
                // Wasm truth value. Saved predicates must keep their original
                // evaluation, and i64 tests must still produce an i32 condition.
                if self.slots[condition].is_none()
                    && wasm_type(self.body.values[input].ty) == ValType::I32
                {
                    self.value(input);
                    return;
                }
            }
        }
        self.value(condition);
        if inverted {
            self.code.instruction(Instruction::I32Eqz);
        }
    }

    fn evaluate(&mut self, root: usize, capture: bool) {
        let mut pending = vec![Walk::Value(root)];
        while let Some(next) = pending.pop() {
            let id = match next {
                Walk::Value(id) => place::representation(self.body, id),
                Walk::Finish(id) => {
                    self.operation(id);
                    self.completed(id, capture && id == root);
                    continue;
                }
                Walk::FinishLoad {
                    result,
                    location,
                    signed,
                } => {
                    self.load(location, self.body.values[result].ty, signed);
                    self.completed(result, capture && result == root);
                    continue;
                }
                Walk::FinishCall(id) => {
                    let ValueDefinition::OperationResult { site, .. } =
                        self.body.values[id].definition
                    else {
                        unreachable!("call completion names a call result")
                    };
                    self.finish_call(site, Some((id, capture && id == root)));
                    continue;
                }
                Walk::FinishZero(id, extension) => {
                    if let Some(ty) = extension {
                        let instruction = match ty {
                            Type::I8 => Instruction::I32Extend8S,
                            Type::I16 => Instruction::I32Extend16S,
                            _ => {
                                unreachable!("only byte and word masks have a sign-extension cover")
                            }
                        };
                        self.code.instruction(instruction);
                    }
                    self.operation(id);
                    self.completed(id, capture && id == root);
                    continue;
                }
            };
            if let Some(slot) = self.slots[id].filter(|_| self.emitted[id]) {
                self.code.local(slot, LocalOp::Get);
                continue;
            }
            match self.body.values[id].definition {
                ValueDefinition::Constant(bits) => {
                    self.code.instruction(match self.body.values[id].ty {
                        Type::I1 | Type::I8 | Type::I16 | Type::I32 => {
                            Instruction::I32Const(bits as u32 as i32)
                        }
                        Type::I64 => Instruction::I64Const(bits as i64),
                    })
                }
                ValueDefinition::Parameter(index) => {
                    self.code.instruction(Instruction::LocalGet(index))
                }
                ValueDefinition::JoinResult { .. } => {
                    unreachable!("a used join was saved after its branch operation")
                }
                ValueDefinition::LoopInput { .. } => {
                    unreachable!("loop inputs are saved at the header")
                }
                ValueDefinition::Expression(expression) => match expression {
                    Expression::Binary { left, right, .. }
                    | Expression::Compare { left, right, .. }
                    | Expression::Shift {
                        value: left,
                        count: right,
                        ..
                    }
                    | Expression::Rotate {
                        value: left,
                        count: right,
                        ..
                    } => {
                        pending.push(Walk::Finish(id));
                        pending.push(Walk::Value(right));
                        pending.push(Walk::Value(left));
                    }
                    Expression::Select {
                        condition,
                        when_true,
                        when_false,
                    } => {
                        pending.push(Walk::Finish(id));
                        pending.push(Walk::Value(self.condition_input(condition)));
                        pending.push(Walk::Value(when_false));
                        pending.push(Walk::Value(when_true));
                    }
                    Expression::Normalize { input }
                    | Expression::Convert { input }
                    | Expression::BitCount { input, .. } => {
                        pending.push(Walk::Finish(id));
                        pending.push(Walk::Value(input));
                    }
                    Expression::SignExtend { input } => {
                        if let Some(location) = self.signed_load_location(input) {
                            pending.push(Walk::FinishLoad {
                                result: id,
                                location,
                                signed: true,
                            });
                            pending.push(Walk::Value(location.base));
                        } else {
                            pending.push(Walk::Finish(id));
                            pending.push(Walk::Value(input));
                        }
                    }
                    Expression::ZeroTest { input, .. } => {
                        let mut input = place::representation(self.body, input);
                        let mut extension = None;
                        if let ValueDefinition::Expression(Expression::Normalize { input: raw }) =
                            self.body.values[input].definition
                        {
                            let ty = self.body.values[input].ty;
                            if matches!(ty, Type::I8 | Type::I16) && self.slots[input].is_none() {
                                // For a zero test alone, sign extension tests the same low
                                // bits with one instruction. Shared masks remain unsigned.
                                extension = Some(ty);
                                input = raw;
                            }
                        }
                        pending.push(Walk::FinishZero(id, extension));
                        pending.push(Walk::Value(input));
                    }
                },
                ValueDefinition::OperationResult { site, .. } => {
                    pending.push(Walk::FinishCall(id));
                    for &argument in self.blocks.call(site).0.arguments.iter().rev() {
                        pending.push(Walk::Value(argument));
                    }
                }
                ValueDefinition::Load { site } => {
                    let location = self.blocks.load_location(site);
                    pending.push(Walk::FinishLoad {
                        result: id,
                        location,
                        signed: false,
                    });
                    pending.push(Walk::Value(location.base));
                }
            }
        }
    }

    fn call(&mut self, target: crate::Func) {
        self.code.instruction(Instruction::Call(
            self.functions[target.0].expect("a call target has a function index"),
        ));
    }
}
