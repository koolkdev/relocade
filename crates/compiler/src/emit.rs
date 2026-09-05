//! Instruction ordering and storage of shared expression results.
use wasm_encoder::{Encode, Function, Instruction, ValType};

use crate::{locals, Body, Type, ValueKind};

struct LocalEvent {
    // Position in the byte buffer, excluding local instructions inserted later.
    offset: usize,
    slot: usize,
    tee: bool,
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
}

pub(super) fn encode(body: &Body, parameter_count: u32) -> Function {
    // Values are appended after their operands, so reverse order visits consumers
    // before dependencies. Count each reachable addition's operands once: a shared
    // addition executes once, regardless of how many later additions use it.
    let mut uses = vec![0_usize; body.values.len()];
    uses[body.result] = 1;
    for id in (0..body.values.len()).rev() {
        if uses[id] != 0 {
            if let ValueKind::Add(a, b) = body.values[id].kind {
                uses[a] += 1;
                uses[b] += 1;
            }
        }
    }
    // Only shared additions need temporary locals; constants and parameters can
    // be emitted directly each time they are used.
    let mut slot_types = Vec::new();
    let slots: Vec<_> = body
        .values
        .iter()
        .enumerate()
        .map(|(id, value)| {
            if uses[id] > 1 && matches!(value.kind, ValueKind::Add(..)) {
                let slot = slot_types.len();
                slot_types.push(wasm_type(value.ty));
                Some(slot)
            } else {
                None
            }
        })
        .collect();
    let mut emitted = vec![false; body.values.len()];
    let mut bytes = Vec::new();
    let mut events = Vec::new();
    let mut pending = vec![Walk::Value(body.result)];
    while let Some(next) = pending.pop() {
        let id = match next {
            Walk::Value(id) => id,
            Walk::FinishAdd(id) => {
                match body.values[id].ty {
                    Type::I1 | Type::I8 | Type::I16 | Type::I32 => Instruction::I32Add,
                    Type::I64 => Instruction::I64Add,
                }
                .encode(&mut bytes);
                if let Some(slot) = slots[id] {
                    events.push(LocalEvent {
                        offset: bytes.len(),
                        slot,
                        tee: true,
                    });
                }
                emitted[id] = true;
                continue;
            }
        };
        if let Some(slot) = slots[id].filter(|_| emitted[id]) {
            events.push(LocalEvent {
                offset: bytes.len(),
                slot,
                tee: false,
            });
            continue;
        }
        match body.values[id].kind {
            ValueKind::Constant(bits) => match body.values[id].ty {
                Type::I1 | Type::I8 | Type::I16 | Type::I32 => {
                    Instruction::I32Const(bits as u32 as i32)
                }
                Type::I64 => Instruction::I64Const(bits as i64),
            }
            .encode(&mut bytes),
            ValueKind::Parameter(index) => Instruction::LocalGet(index).encode(&mut bytes),
            ValueKind::Add(a, b) => {
                pending.push(Walk::FinishAdd(id));
                pending.push(Walk::Value(b));
                pending.push(Walk::Value(a));
            }
        }
    }
    // Constants and incoming parameters are canonical. Addition preserves the
    // low bits without masking each step; only its returned result needs the
    // unused upper bits cleared.
    let result = &body.values[body.result];
    match result.kind {
        ValueKind::Constant(_) | ValueKind::Parameter(_) => {}
        ValueKind::Add(..) => {
            if matches!(result.ty, Type::I1 | Type::I8 | Type::I16) {
                Instruction::I32Const(result.ty.mask() as i32).encode(&mut bytes);
                Instruction::I32And.encode(&mut bytes);
            }
        }
    }
    Instruction::Return.encode(&mut bytes);
    Instruction::End.encode(&mut bytes);

    // Wasm local declarations precede instructions. Choose their types and indices
    // from the completed access order, then insert local instructions into the
    // buffered code.
    let allocated = locals::allocate(events.iter().map(|event| event.slot), &slot_types);
    let mut function = Function::new_with_locals_types(allocated.types);
    let mut previous = 0;
    for event in events {
        function.raw(bytes[previous..event.offset].iter().copied());
        previous = event.offset;
        let local = parameter_count
            .checked_add(allocated.indices[event.slot])
            .expect("function locals fit the Wasm index space");
        function.instruction(&if event.tee {
            Instruction::LocalTee(local)
        } else {
            Instruction::LocalGet(local)
        });
    }
    function.raw(bytes[previous..].iter().copied());
    function
}
