//! Instruction ordering and storage of shared expression results.
use wasm_encoder::{Encode, Function, Instruction, MemArg, ValType};

use crate::{locals, memory::Location, place, Body, Operation, Terminal, Type, ValueKind};

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
    FinishAdd(usize),
    FinishNormalize(usize),
}

struct Scheduler<'a> {
    body: &'a Body,
    memories: &'a [Option<u32>],
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
) -> Function {
    let mut scheduler = Scheduler {
        body,
        memories,
        placement: place::plan(body),
        emitted: vec![false; body.values.len()],
        bytes: Vec::new(),
        events: Vec::new(),
    };
    for operation in &body.operations {
        match *operation {
            Operation::Load(id) if scheduler.placement.captures[id] => {
                // An overlapping write forces this read back to its authored
                // position. Its fixed address has no expression dependencies.
                scheduler.load(id);
                let slot = scheduler.placement.slots[id].expect("a captured read has storage");
                scheduler.local(slot, LocalOp::Set);
                scheduler.emitted[id] = true;
            }
            Operation::Load(_) => {}
            Operation::Store { location, value } => {
                Instruction::I32Const(0).encode(&mut scheduler.bytes);
                scheduler.value(value);
                let argument = scheduler.memory_argument(location);
                match location.bytes {
                    1 => Instruction::I32Store8(argument),
                    2 => Instruction::I32Store16(argument),
                    4 => Instruction::I32Store(argument),
                    8 => Instruction::I64Store(argument),
                    _ => unreachable!("memory locations have a supported byte size"),
                }
                .encode(&mut scheduler.bytes);
            }
        }
    }
    for &value in body.terminal.inputs() {
        scheduler.value(value);
    }
    match body.terminal {
        Terminal::Return(_) => Instruction::Return,
        Terminal::TailCall { target, .. } => Instruction::ReturnCall(
            functions[target.0].expect("a tail-call target has a function index"),
        ),
    }
    .encode(&mut scheduler.bytes);
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

    fn completed(&mut self, id: usize) {
        if let Some(slot) = self.placement.slots[id] {
            self.local(slot, LocalOp::Tee);
        }
        self.emitted[id] = true;
    }

    fn value(&mut self, root: usize) {
        let mut pending = vec![Walk::Value(root)];
        while let Some(next) = pending.pop() {
            let id = match next {
                Walk::Value(id) => id,
                Walk::FinishNormalize(id) => {
                    Instruction::I32Const(self.body.values[id].ty.mask() as i32)
                        .encode(&mut self.bytes);
                    Instruction::I32And.encode(&mut self.bytes);
                    self.completed(id);
                    continue;
                }
                Walk::FinishAdd(id) => {
                    match self.body.values[id].ty {
                        Type::I1 | Type::I8 | Type::I16 | Type::I32 => Instruction::I32Add,
                        Type::I64 => Instruction::I64Add,
                    }
                    .encode(&mut self.bytes);
                    self.completed(id);
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
                ValueKind::Add(a, b) => {
                    pending.push(Walk::FinishAdd(id));
                    pending.push(Walk::Value(b));
                    pending.push(Walk::Value(a));
                }
                ValueKind::Normalize(input) => {
                    pending.push(Walk::FinishNormalize(id));
                    pending.push(Walk::Value(input));
                }
                ValueKind::Load { .. } => {
                    self.load(id);
                    self.completed(id);
                }
            }
        }
    }

    fn memory_argument(&self, location: Location) -> MemArg {
        MemArg {
            offset: u64::from(location.offset),
            align: location.bytes.trailing_zeros(),
            memory_index: self.memories[location.memory.0].expect("an authored memory is imported"),
        }
    }

    fn load(&mut self, id: usize) {
        let ValueKind::Load { location, .. } = self.body.values[id].kind else {
            unreachable!("load evaluation names a load value")
        };
        let argument = self.memory_argument(location);
        Instruction::I32Const(0).encode(&mut self.bytes);
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
                .checked_add(allocated.indices[event.slot])
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
