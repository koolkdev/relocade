//! Instruction ordering and storage of shared expression results.
use wasm_encoder::{Encode, Function, Instruction, MemArg, ValType};

use crate::{
    control::Site,
    effects::Effects,
    integer::{BinaryOp, CompareOp, ShiftOp},
    locals,
    memory::Location,
    module::Types,
    place, Body, Type, ValueKind,
};

mod control;
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
    FinishLoad(usize),
    FinishCall(usize),
    FinishZero(usize, Option<Type>),
}

struct ControlLabel {
    target: Option<Site>,
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
    };
    scheduler.region(&body.region, None);
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
        let ValueKind::ZeroTest {
            input,
            nonzero: true,
        } = self.body.values[condition].kind
        else {
            return condition;
        };
        // Wasm truth consumers accept any nonzero i32. ZeroTest already normalized
        // its input; a saved Boolean must retain its canonical numeric value.
        if self.placement.slots[condition].is_none()
            && wasm_type(self.body.values[input].ty) == ValType::I32
        {
            input
        } else {
            condition
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
                Walk::FinishLoad(id) => {
                    self.load(id);
                    self.completed(id, capture && id == root);
                    continue;
                }
                Walk::FinishCall(id) => {
                    let ValueKind::CallResult { site } = self.body.values[id].kind else {
                        unreachable!("call completion names a call result")
                    };
                    self.call(self.body.invocation(site).target);
                    self.completed(id, capture && id == root);
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
            match self.body.values[id].kind {
                ValueKind::Constant(bits) => match self.body.values[id].ty {
                    Type::I1 | Type::I8 | Type::I16 | Type::I32 => {
                        Instruction::I32Const(bits as u32 as i32)
                    }
                    Type::I64 => Instruction::I64Const(bits as i64),
                }
                .encode(&mut self.bytes),
                ValueKind::Parameter(index) => Instruction::LocalGet(index).encode(&mut self.bytes),
                ValueKind::JoinResult { .. } => {
                    unreachable!("a used join was saved after its branch operation")
                }
                ValueKind::Binary(_, a, b)
                | ValueKind::Compare(_, a, b)
                | ValueKind::Shift {
                    value: a, count: b, ..
                } => {
                    pending.push(Walk::Finish(id));
                    pending.push(Walk::Value(b));
                    pending.push(Walk::Value(a));
                }
                ValueKind::Select {
                    condition,
                    when_true,
                    when_false,
                } => {
                    pending.push(Walk::Finish(id));
                    pending.push(Walk::Value(self.condition_input(condition)));
                    pending.push(Walk::Value(when_false));
                    pending.push(Walk::Value(when_true));
                }
                ValueKind::Normalize(input)
                | ValueKind::Convert(input)
                | ValueKind::SignExtend(input)
                | ValueKind::Popcnt(input) => {
                    pending.push(Walk::Finish(id));
                    pending.push(Walk::Value(input));
                }
                ValueKind::ZeroTest { input, .. } => {
                    let mut input = place::representation(self.body, input);
                    let mut extension = None;
                    if let ValueKind::Normalize(raw) = self.body.values[input].kind {
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
                ValueKind::CallResult { site } => {
                    pending.push(Walk::FinishCall(id));
                    for &argument in self.body.invocation(site).arguments.iter().rev() {
                        pending.push(Walk::Value(argument));
                    }
                }
                ValueKind::Load { location, .. } => {
                    pending.push(Walk::FinishLoad(id));
                    pending.push(Walk::Value(location.base));
                }
            }
        }
    }

    fn operation(&mut self, id: usize) {
        let value = self.body.values[id];
        let wide = value.ty == Type::I64;
        let instruction = match value.kind {
            ValueKind::Binary(operator, _, _) => match (operator, wide) {
                (BinaryOp::Add, false) => Instruction::I32Add,
                (BinaryOp::Add, true) => Instruction::I64Add,
                (BinaryOp::Sub, false) => Instruction::I32Sub,
                (BinaryOp::Sub, true) => Instruction::I64Sub,
                (BinaryOp::And, false) => Instruction::I32And,
                (BinaryOp::And, true) => Instruction::I64And,
                (BinaryOp::Or, false) => Instruction::I32Or,
                (BinaryOp::Or, true) => Instruction::I64Or,
                (BinaryOp::Xor, false) => Instruction::I32Xor,
                (BinaryOp::Xor, true) => Instruction::I64Xor,
            },
            ValueKind::Shift { operator, .. } => match (operator, wide) {
                (ShiftOp::Left, false) => Instruction::I32Shl,
                (ShiftOp::Left, true) => Instruction::I64Shl,
                (ShiftOp::RightUnsigned, false) => Instruction::I32ShrU,
                (ShiftOp::RightUnsigned, true) => Instruction::I64ShrU,
                (ShiftOp::RightSigned, false) => Instruction::I32ShrS,
                (ShiftOp::RightSigned, true) => Instruction::I64ShrS,
            },
            ValueKind::Select { .. } => Instruction::Select,
            ValueKind::Popcnt(_) => {
                if wide {
                    Instruction::I64Popcnt
                } else {
                    Instruction::I32Popcnt
                }
            }
            ValueKind::SignExtend(input) => {
                match self.body.values[input].ty {
                    Type::I1 => {
                        Instruction::I32Const(31).encode(&mut self.bytes);
                        Instruction::I32Shl.encode(&mut self.bytes);
                        Instruction::I32Const(31).encode(&mut self.bytes);
                        Instruction::I32ShrS.encode(&mut self.bytes);
                    }
                    Type::I8 => Instruction::I32Extend8S.encode(&mut self.bytes),
                    Type::I16 => Instruction::I32Extend16S.encode(&mut self.bytes),
                    Type::I32 => {}
                    Type::I64 => unreachable!("a signed extension widens its input"),
                }
                if wide {
                    Instruction::I64ExtendI32S
                } else {
                    return;
                }
            }
            ValueKind::Compare(operator, a, _) => {
                // Comparisons produce I1; their opcode follows the operands' carrier.
                let wide = self.body.values[a].ty == Type::I64;
                match (operator, wide) {
                    (CompareOp::Eq, false) => Instruction::I32Eq,
                    (CompareOp::Eq, true) => Instruction::I64Eq,
                    (CompareOp::Ne, false) => Instruction::I32Ne,
                    (CompareOp::Ne, true) => Instruction::I64Ne,
                    (CompareOp::LtUnsigned, false) => Instruction::I32LtU,
                    (CompareOp::LtUnsigned, true) => Instruction::I64LtU,
                    (CompareOp::GeUnsigned, false) => Instruction::I32GeU,
                    (CompareOp::GeUnsigned, true) => Instruction::I64GeU,
                    (CompareOp::LtSigned, false) => Instruction::I32LtS,
                    (CompareOp::LtSigned, true) => Instruction::I64LtS,
                    (CompareOp::GeSigned, false) => Instruction::I32GeS,
                    (CompareOp::GeSigned, true) => Instruction::I64GeS,
                }
            }
            ValueKind::ZeroTest { input, nonzero } => {
                let test = if self.body.values[input].ty == Type::I64 {
                    Instruction::I64Eqz
                } else {
                    Instruction::I32Eqz
                };
                if nonzero {
                    test.encode(&mut self.bytes);
                    Instruction::I32Eqz
                } else {
                    test
                }
            }
            ValueKind::Normalize(_) => {
                Instruction::I32Const(value.ty.mask() as i32).encode(&mut self.bytes);
                Instruction::I32And
            }
            ValueKind::Convert(_) => {
                if wide {
                    Instruction::I64ExtendI32U
                } else {
                    Instruction::I32WrapI64
                }
            }
            ValueKind::Constant(_)
            | ValueKind::Parameter(_)
            | ValueKind::Load { .. }
            | ValueKind::CallResult { .. }
            | ValueKind::JoinResult { .. } => {
                unreachable!("constants, parameters and authored results emit separately")
            }
        };
        instruction.encode(&mut self.bytes);
    }

    fn memory_argument(&self, location: Location) -> MemArg {
        MemArg {
            offset: u64::from(location.offset),
            align: location.bytes.trailing_zeros(),
            memory_index: self.memories[location.memory.0].expect("an authored memory is imported"),
        }
    }

    fn call(&mut self, target: crate::Func) {
        Instruction::Call(self.functions[target.0].expect("a call target has a function index"))
            .encode(&mut self.bytes);
    }

    fn load(&mut self, id: usize) {
        let ValueKind::Load { location, .. } = self.body.values[id].kind else {
            unreachable!("load evaluation names a load value")
        };
        let argument = self.memory_argument(location);
        match location.bytes {
            1 => Instruction::I32Load8U(argument),
            2 => Instruction::I32Load16U(argument),
            4 => Instruction::I32Load(argument),
            8 => Instruction::I64Load(argument),
            _ => unreachable!("memory locations have a supported byte size"),
        }
        .encode(&mut self.bytes);
    }

    fn finish(self, parameter_count: u32) -> Function {
        // Wasm local declarations precede instructions. Choose their types and indices
        // from the completed access order, then insert local instructions into the
        // buffered code.
        let allocated = locals::allocate(
            self.events.iter().map(|event| event.slot),
            &self.placement.slot_types,
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
