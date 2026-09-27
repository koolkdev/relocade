//! Instruction ordering and storage of shared expression results.
use wasm_encoder::{Encode, Function, Instruction, ValType};

use crate::{
    body::{Body, Target, ValueDefinition},
    effects::Effects,
    locals,
    memory::Location,
    module::Types,
    place, Expression, Type,
};

mod branch;
mod calls;
mod control;
mod integer;
mod loops;
mod memory;
mod switch;

struct LocalEvent {
    // Position in the byte buffer, excluding local instructions inserted later.
    offset: usize,
    slot: usize,
    operation: LocalOp,
}

enum LocalOp {
    Get,
    Set,
    Tee,
}

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
    memories: &'a [Option<u32>],
    functions: &'a [Option<u32>],
    effects: &'a [Effects],
    types: &'a mut Types,
    labels: Vec<ControlLabel>,
    placement: place::Placement,
    emitted: Vec<bool>,
    bytes: Vec<u8>,
    events: Vec<LocalEvent>,
    loop_ranges: Vec<(usize, usize)>,
}

pub(super) fn encode(
    body: &Body,
    parameter_count: u32,
    memories: &[Option<u32>],
    functions: &[Option<u32>],
    effects: &[Effects],
    types: &mut Types,
) -> Function {
    let mut scheduler = Scheduler {
        body,
        memories,
        functions,
        effects,
        types,
        labels: Vec::new(),
        placement: place::plan(body, effects),
        emitted: vec![false; body.values.len()],
        bytes: Vec::new(),
        events: Vec::new(),
        loop_ranges: Vec::new(),
    };
    scheduler.block(&body.block, None);
    Instruction::End.encode(&mut scheduler.bytes);
    scheduler.finish(parameter_count)
}

impl Scheduler<'_> {
    fn local(&mut self, slot: usize, operation: LocalOp) {
        self.events.push(LocalEvent {
            offset: self.bytes.len(),
            slot,
            operation,
        });
    }

    fn completed(&mut self, id: usize, capture: bool) {
        if let Some(slot) = self.placement.slots[id] {
            self.local(slot, if capture { LocalOp::Set } else { LocalOp::Tee });
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
        if self.placement.slots[condition].is_none()
            && wasm_type(self.body.values[input].ty) == ValType::I32
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
                if self.placement.slots[condition].is_none()
                    && wasm_type(self.body.values[input].ty) == ValType::I32
                {
                    self.value(input);
                    return;
                }
            }
        }
        self.value(condition);
        if inverted {
            Instruction::I32Eqz.encode(&mut self.bytes);
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
                        match ty {
                            Type::I8 => Instruction::I32Extend8S,
                            Type::I16 => Instruction::I32Extend16S,
                            _ => {
                                unreachable!("only byte and word masks have a sign-extension cover")
                            }
                        }
                        .encode(&mut self.bytes);
                    }
                    self.operation(id);
                    self.completed(id, capture && id == root);
                    continue;
                }
            };
            if let Some(slot) = self.placement.slots[id].filter(|_| self.emitted[id]) {
                self.local(slot, LocalOp::Get);
                continue;
            }
            match self.body.values[id].definition {
                ValueDefinition::Constant(bits) => match self.body.values[id].ty {
                    Type::I1 | Type::I8 | Type::I16 | Type::I32 => {
                        Instruction::I32Const(bits as u32 as i32)
                    }
                    Type::I64 => Instruction::I64Const(bits as i64),
                }
                .encode(&mut self.bytes),
                ValueDefinition::Parameter(index) => {
                    Instruction::LocalGet(index).encode(&mut self.bytes)
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
                            if matches!(ty, Type::I8 | Type::I16)
                                && self.placement.slots[input].is_none()
                            {
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
                    for &argument in self.body.call(site).0.arguments.iter().rev() {
                        pending.push(Walk::Value(argument));
                    }
                }
                ValueDefinition::Load { location, .. } => {
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
        Instruction::Call(self.functions[target.0].expect("a call target has a function index"))
            .encode(&mut self.bytes);
    }

    fn finish(self, parameter_count: u32) -> Function {
        // Wasm local declarations precede instructions. Choose their types and indices
        // from the completed access order, then insert local instructions into the
        // buffered code.
        let allocated = locals::allocate(
            self.events.iter().map(|event| event.slot),
            &self.placement.slot_types,
            &self.loop_ranges,
        );
        let mut function = Function::new_with_locals_types(allocated.types);
        let mut previous = 0;
        for event in self.events {
            function.raw(self.bytes[previous..event.offset].iter().copied());
            previous = event.offset;
            let local = parameter_count
                .checked_add(
                    allocated.indices[event.slot].expect("an emitted local access has an index"),
                )
                .expect("function locals fit the Wasm index space");
            function.instruction(&match event.operation {
                LocalOp::Get => Instruction::LocalGet(local),
                LocalOp::Set => Instruction::LocalSet(local),
                LocalOp::Tee => Instruction::LocalTee(local),
            });
        }
        function.raw(self.bytes[previous..].iter().copied());
        function
    }
}
