//! Schedule value dependencies into an explicit Wasm operand-stack order.
use std::collections::HashMap;

use super::function::LocalOp;
use crate::{
    body::{BlockTree, Body, Site, ValueDefinition},
    memory::Location,
    place, Expression, Func, Mem, Type,
};

mod calls;
mod selection;
#[cfg(test)]
mod tests;

/// Operands come from preceding steps or an enclosing operation's result tuple.
/// Expression inputs retain their logical types for opcode selection, not value
/// IDs. Loads likewise consume an address already on the stack.
pub(super) enum Evaluation {
    Constant {
        ty: Type,
        bits: u64,
    },
    Parameter(u32),
    Local {
        slot: usize,
        operation: LocalOp,
    },
    Expression {
        result_type: Type,
        expression: Expression<Type>,
    },
    Load {
        memory: Mem,
        offset: u32,
        bytes: u8,
        result_type: Type,
        signed: bool,
    },
    Call(Func),
    Drop,
}

/// Values already produced on entry to a control arm or loop.
pub(super) struct Availability(Vec<bool>);

/// Plans are consumed in request order. Planning a definition makes its saved
/// value available to later requests, before any Wasm bytes are written.
pub(super) struct ValuePlanner<'a> {
    body: &'a Body,
    blocks: BlockTree<'a>,
    slots: Vec<Option<usize>>,
    captures: HashMap<Site, Vec<usize>>,
    available: Vec<bool>,
}

struct RequestedResult {
    value: usize,
    capture: bool,
}

enum Walk {
    Value(usize),
    FinishExpression {
        result: usize,
        sign_extend_from: Option<Type>,
    },
    FinishLoad {
        result: usize,
        location: Location,
        signed: bool,
    },
    FinishCall(usize),
}

impl<'a> ValuePlanner<'a> {
    pub(super) fn new(
        body: &'a Body,
        blocks: BlockTree<'a>,
        slots: Vec<Option<usize>>,
        captures: HashMap<Site, Vec<usize>>,
    ) -> Self {
        Self {
            body,
            blocks,
            slots,
            captures,
            available: vec![false; body.values.len()],
        }
    }

    pub(super) fn has_local(&self, value: usize) -> bool {
        self.slots[value].is_some()
    }

    pub(super) fn checkpoint(&self) -> Availability {
        Availability(self.available.clone())
    }

    pub(super) fn restore(&mut self, checkpoint: &Availability) {
        self.available.clone_from(&checkpoint.0);
    }

    pub(super) fn values(&mut self, inputs: impl IntoIterator<Item = usize>) -> Vec<Evaluation> {
        let mut plan = Vec::new();
        for input in inputs {
            self.evaluate(input, false, &mut plan);
        }
        plan
    }

    pub(super) fn captures(&mut self, site: Site) -> Vec<Evaluation> {
        let mut plan = Vec::new();
        if let Some(captures) = self.captures.get(&site) {
            for index in 0..captures.len() {
                let id = self.captures[&site][index];
                if !self.available[id] {
                    self.evaluate(id, true, &mut plan);
                }
            }
        }
        plan
    }

    /// Consume the complete result tuple of an already scheduled operation.
    /// The last result is on top; unused components still need to be dropped.
    pub(super) fn save_results(&mut self, outputs: &[usize]) -> Vec<Evaluation> {
        let mut plan = Vec::new();
        self.consume_results(outputs, &mut plan);
        plan
    }

    fn consume_results(&mut self, outputs: &[usize], plan: &mut Vec<Evaluation>) {
        for &output in outputs.iter().rev() {
            self.completed(output, true, plan);
            if !self.has_local(output) {
                plan.push(Evaluation::Drop);
            }
        }
    }

    fn completed(&mut self, id: usize, capture: bool, plan: &mut Vec<Evaluation>) {
        if let Some(slot) = self.slots[id] {
            plan.push(Evaluation::Local {
                slot,
                operation: if capture { LocalOp::Set } else { LocalOp::Tee },
            });
        }
        self.available[id] = true;
    }

    fn evaluate(&mut self, root: usize, capture: bool, plan: &mut Vec<Evaluation>) {
        let mut pending = vec![Walk::Value(root)];
        while let Some(next) = pending.pop() {
            let id = match next {
                Walk::Value(id) => place::representation(self.body, id),
                Walk::FinishExpression {
                    result,
                    sign_extend_from,
                } => {
                    if let Some(input) = sign_extend_from {
                        plan.push(Evaluation::Expression {
                            result_type: Type::I32,
                            expression: Expression::SignExtend { input },
                        });
                    }
                    let value = self.body.values[result];
                    let ValueDefinition::Expression(expression) = value.definition else {
                        unreachable!("expression completion names an expression result")
                    };
                    plan.push(Evaluation::Expression {
                        result_type: value.ty,
                        expression: expression.map(|&input| self.body.values[input].ty),
                    });
                    self.completed(result, capture && result == root, plan);
                    continue;
                }
                Walk::FinishLoad {
                    result,
                    location,
                    signed,
                } => {
                    plan.push(Evaluation::Load {
                        memory: location.memory,
                        offset: location.offset,
                        bytes: location.bytes,
                        result_type: self.body.values[result].ty,
                        signed,
                    });
                    self.completed(result, capture && result == root, plan);
                    continue;
                }
                Walk::FinishCall(id) => {
                    let ValueDefinition::OperationResult { site, .. } =
                        self.body.values[id].definition
                    else {
                        unreachable!("call completion names a call result")
                    };
                    self.finish_call(
                        site,
                        Some(RequestedResult {
                            value: id,
                            capture: capture && id == root,
                        }),
                        plan,
                    );
                    continue;
                }
            };
            if let Some(slot) = self.slots[id].filter(|_| self.available[id]) {
                plan.push(Evaluation::Local {
                    slot,
                    operation: LocalOp::Get,
                });
                continue;
            }
            match self.body.values[id].definition {
                ValueDefinition::Constant(bits) => plan.push(Evaluation::Constant {
                    ty: self.body.values[id].ty,
                    bits,
                }),
                ValueDefinition::Parameter(index) => plan.push(Evaluation::Parameter(index)),
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
                        pending.push(Walk::FinishExpression {
                            result: id,
                            sign_extend_from: None,
                        });
                        pending.push(Walk::Value(right));
                        pending.push(Walk::Value(left));
                    }
                    Expression::Select {
                        condition,
                        when_true,
                        when_false,
                    } => {
                        pending.push(Walk::FinishExpression {
                            result: id,
                            sign_extend_from: None,
                        });
                        pending.push(Walk::Value(self.condition_input(condition)));
                        pending.push(Walk::Value(when_false));
                        pending.push(Walk::Value(when_true));
                    }
                    Expression::Normalize { input }
                    | Expression::Convert { input }
                    | Expression::BitCount { input, .. } => {
                        pending.push(Walk::FinishExpression {
                            result: id,
                            sign_extend_from: None,
                        });
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
                            pending.push(Walk::FinishExpression {
                                result: id,
                                sign_extend_from: None,
                            });
                            pending.push(Walk::Value(input));
                        }
                    }
                    Expression::ZeroTest { input, .. } => {
                        let mut input = place::representation(self.body, input);
                        let mut sign_extend_from = None;
                        if let ValueDefinition::Expression(Expression::Normalize { input: raw }) =
                            self.body.values[input].definition
                        {
                            let ty = self.body.values[input].ty;
                            if matches!(ty, Type::I8 | Type::I16) && self.slots[input].is_none() {
                                // For a zero test alone, sign extension tests the same low
                                // bits with one instruction. Shared masks remain unsigned.
                                sign_extend_from = Some(ty);
                                input = raw;
                            }
                        }
                        pending.push(Walk::FinishExpression {
                            result: id,
                            sign_extend_from,
                        });
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
}
