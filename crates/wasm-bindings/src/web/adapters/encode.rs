use super::{Direction, Shape};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, GlobalType,
    ImportSection, Instruction as Op, MemArg, MemoryType, Module, TypeSection, ValType,
};

pub(super) fn module(shape: &Shape) -> Vec<u8> {
    let callback = shape.direction == Direction::Callback;
    let indirect = shape.area.is_some();
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function(
        shape.parameters.iter().copied(),
        shape.results.iter().copied(),
    );
    // Rust's C ABI places a hidden return pointer before the ordinary arguments.
    // A callback also receives its captured Rust closure pointer.
    let rust_parameters =
        std::iter::repeat_n(ValType::I32, usize::from(indirect) + usize::from(callback))
            .chain(shape.parameters.iter().copied())
            .collect::<Vec<_>>();
    types.ty().function(
        rust_parameters,
        shape
            .results
            .iter()
            .copied()
            .take(if indirect { 0 } else { 1 }),
    );
    types.ty().function([ValType::EXTERNREF], []);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "target",
        EntityType::Function(if callback { 1 } else { 0 }),
    );
    if callback {
        imports.import("env", "retain", EntityType::Function(2));
        global(&mut imports, "context", ValType::I32, false);
        global(&mut imports, "anchor", ValType::EXTERNREF, false);
        if indirect {
            global(&mut imports, "stack", ValType::I32, true);
        }
    }
    if indirect {
        imports.import(
            "env",
            "memory",
            MemoryType {
                minimum: 0,
                maximum: shape.shared.then_some(65536),
                memory64: false,
                shared: shape.shared,
                page_size_log2: None,
            },
        );
    }
    module.section(&imports);
    let needs_body = callback || indirect;
    if needs_body {
        let mut functions = FunctionSection::new();
        functions.function(if callback { 0 } else { 1 });
        module.section(&functions);
    }
    let mut exports = ExportSection::new();
    // Reexport a scalar target directly. Instantiation checks its physical type,
    // while the call itself needs no extra adapter function or memory access.
    exports.export(
        "run",
        ExportKind::Func,
        if callback { 2 } else { u32::from(indirect) },
    );
    module.section(&exports);
    if needs_body {
        let body = if callback {
            callback_body(shape)
        } else {
            export_body(shape)
        };
        let mut code = CodeSection::new();
        code.function(&body);
        module.section(&code);
    }
    module.finish()
}

fn global(imports: &mut ImportSection, name: &str, val_type: ValType, mutable: bool) {
    imports.import(
        "env",
        name,
        GlobalType {
            val_type,
            mutable,
            shared: false,
        },
    );
}

fn export_body(shape: &Shape) -> Function {
    let area = shape.area.as_ref().expect("aggregate return");
    let mut body = Function::new(shape.results.iter().map(|&ty| (1, ty)));
    let first_result = shape.parameters.len() as u32 + 1;
    for index in 0..shape.parameters.len() as u32 {
        body.instruction(&Op::LocalGet(index + 1));
    }
    body.instruction(&Op::Call(0));
    for index in (0..shape.results.len() as u32).rev() {
        body.instruction(&Op::LocalSet(first_result + index));
    }
    for (index, (&ty, &offset)) in shape.results.iter().zip(&area.offsets).enumerate() {
        body.instruction(&Op::LocalGet(0));
        body.instruction(&Op::LocalGet(first_result + index as u32));
        body.instruction(&store(ty, offset));
    }
    body.instruction(&Op::End);
    body
}

fn callback_body(shape: &Shape) -> Function {
    let indirect = shape.area.is_some();
    let mut locals = Vec::new();
    if indirect {
        locals.push((2, ValType::I32));
    }
    locals.extend(shape.results.iter().map(|&ty| (1, ty)));
    let mut body = Function::new(locals);
    let saved_stack = shape.parameters.len() as u32;
    let return_pointer = saved_stack + 1;
    let first_result = saved_stack + if indirect { 2 } else { 0 };
    // Keep the callback's owner on the operand stack across all Rust execution.
    body.instruction(&Op::GlobalGet(1));
    if let Some(area) = &shape.area {
        // Rust's stack is 16-byte aligned. Its callee grows below this return
        // area; nested callbacks therefore get independent ordinary call frames.
        body.instruction(&Op::GlobalGet(2));
        body.instruction(&Op::LocalTee(saved_stack));
        body.instruction(&Op::I32Const(area.size.next_multiple_of(16) as i32));
        body.instruction(&Op::I32Sub);
        body.instruction(&Op::LocalTee(return_pointer));
        body.instruction(&Op::GlobalSet(2));
        body.instruction(&Op::LocalGet(return_pointer));
    }
    body.instruction(&Op::GlobalGet(0));
    for index in 0..shape.parameters.len() as u32 {
        body.instruction(&Op::LocalGet(index));
    }
    body.instruction(&Op::Call(0));
    if let Some(area) = &shape.area {
        for (index, (&ty, &offset)) in shape.results.iter().zip(&area.offsets).enumerate() {
            body.instruction(&Op::LocalGet(return_pointer));
            body.instruction(&load(ty, offset));
            body.instruction(&Op::LocalSet(first_result + index as u32));
        }
        body.instruction(&Op::LocalGet(saved_stack));
        body.instruction(&Op::GlobalSet(2));
    } else if !shape.results.is_empty() {
        body.instruction(&Op::LocalSet(first_result));
    }
    body.instruction(&Op::Call(1));
    for index in 0..shape.results.len() as u32 {
        body.instruction(&Op::LocalGet(first_result + index));
    }
    body.instruction(&Op::End);
    body
}

fn address(ty: ValType, offset: u64) -> MemArg {
    MemArg {
        offset,
        align: if matches!(ty, ValType::I32 | ValType::F32) {
            2
        } else {
            3
        },
        memory_index: 0,
    }
}
fn load(ty: ValType, offset: u64) -> Op<'static> {
    match ty {
        ValType::I32 => Op::I32Load(address(ty, offset)),
        ValType::I64 => Op::I64Load(address(ty, offset)),
        ValType::F32 => Op::F32Load(address(ty, offset)),
        ValType::F64 => Op::F64Load(address(ty, offset)),
        _ => unreachable!("Rust scalar carrier"),
    }
}
fn store(ty: ValType, offset: u64) -> Op<'static> {
    match ty {
        ValType::I32 => Op::I32Store(address(ty, offset)),
        ValType::I64 => Op::I64Store(address(ty, offset)),
        ValType::F32 => Op::F32Store(address(ty, offset)),
        ValType::F64 => Op::F64Store(address(ty, offset)),
        _ => unreachable!("Rust scalar carrier"),
    }
}
