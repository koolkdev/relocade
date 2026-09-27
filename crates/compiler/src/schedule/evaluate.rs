//! Schedule shared value dependencies in Wasm operand-stack order.
use super::{Instruction, LocalOp, Scheduler};
use crate::{
    body::{Site, ValueDefinition},
    memory::Location,
    place, Expression, Type,
};

mod calls;
mod selection;
#[cfg(test)]
mod tests;

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

impl Scheduler<'_> {
    pub(super) fn values(&mut self, inputs: impl IntoIterator<Item = usize>) {
        for input in inputs {
            self.evaluate(input, false);
        }
    }

    pub(super) fn captures(&mut self, site: Site) {
        if let Some(captures) = self.captures.get(&site) {
            for index in 0..captures.len() {
                let id = self.captures[&site][index];
                if !self.available[id] {
                    self.evaluate(id, true);
                }
            }
        }
    }

    /// Consume the complete result tuple of an already scheduled operation.
    /// The last result is on top; unused components still need to be dropped.
    pub(super) fn save_results(&mut self, outputs: &[usize]) {
        for &output in outputs.iter().rev() {
            self.completed(output, true);
            if self.slots[output].is_none() {
                self.instructions.push(Instruction::Drop);
            }
        }
    }

    fn completed(&mut self, id: usize, capture: bool) {
        if let Some(slot) = self.slots[id] {
            self.local(slot, if capture { LocalOp::Set } else { LocalOp::Tee });
        }
        self.available[id] = true;
    }

    fn evaluate(&mut self, root: usize, capture: bool) {
        let mut pending = vec![Walk::Value(root)];
        while let Some(next) = pending.pop() {
            let id = match next {
                Walk::Value(id) => place::representation(self.body, id),
                Walk::FinishExpression {
                    result,
                    sign_extend_from,
                } => {
                    if let Some(input) = sign_extend_from {
                        self.instructions.push(Instruction::Expression {
                            result_type: Type::I32,
                            expression: Expression::SignExtend { input },
                        });
                    }
                    let value = self.body.values[result];
                    let ValueDefinition::Expression(expression) = value.definition else {
                        unreachable!("expression completion names an expression result")
                    };
                    self.instructions.push(Instruction::Expression {
                        result_type: value.ty,
                        expression: expression.map(|&input| self.body.values[input].ty),
                    });
                    self.completed(result, capture && result == root);
                    continue;
                }
                Walk::FinishLoad {
                    result,
                    location,
                    signed,
                } => {
                    self.instructions.push(Instruction::Load {
                        location: location.map(|_| ()),
                        result_type: self.body.values[result].ty,
                        signed,
                    });
                    self.completed(result, capture && result == root);
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
                    );
                    continue;
                }
            };
            if let Some(slot) = self.slots[id].filter(|_| self.available[id]) {
                self.local(slot, LocalOp::Get);
                continue;
            }
            match self.body.values[id].definition {
                ValueDefinition::Constant(bits) => self.instructions.push(Instruction::Constant {
                    ty: self.body.values[id].ty,
                    bits,
                }),
                ValueDefinition::Parameter(index) => {
                    self.instructions.push(Instruction::Parameter(index))
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
