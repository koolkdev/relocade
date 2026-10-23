use super::MemoryTypes;
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction as Op, MemArg, MemoryType, Module, TypeSection, ValType,
};

pub(super) fn encode(memory_types: MemoryTypes) -> Vec<u8> {
    let mut types = TypeSection::new();
    const COPY: u32 = 0;
    const READ_I32: u32 = 1;
    const WRITE_I32: u32 = 2;
    const READ_I64: u32 = 3;
    const WRITE_I64: u32 = 4;
    types.ty().function([ValType::I32; 3], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 2], []);
    types.ty().function([ValType::I32], [ValType::I64]);
    types.ty().function([ValType::I32, ValType::I64], []);

    // Imported handles keep both memories current after growth.
    let mut imports = ImportSection::new();
    for (name, shared) in [
        ("rust", memory_types.rust_shared),
        ("memory", memory_types.imported_shared),
    ] {
        imports.import(
            "env",
            name,
            EntityType::Memory(MemoryType {
                minimum: 0,
                maximum: shared.then_some(65536),
                memory64: false,
                shared,
                page_size_log2: None,
            }),
        );
    }

    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut code = CodeSection::new();
    let mut transfer = |name: &str, signature, parameters, operation| {
        exports.export(name, ExportKind::Func, functions.len());
        functions.function(signature);
        let mut body = Function::new([]);
        for parameter in 0..parameters {
            body.instruction(&Op::LocalGet(parameter));
        }
        body.instruction(&operation);
        body.instruction(&Op::End);
        code.function(&body);
    };
    transfer(
        "read",
        COPY,
        3,
        Op::MemoryCopy {
            src_mem: 1,
            dst_mem: 0,
        },
    );
    transfer(
        "write",
        COPY,
        3,
        Op::MemoryCopy {
            src_mem: 0,
            dst_mem: 1,
        },
    );

    // Each scalar entry takes an offset and directly loads or stores its width.
    // Values cross the function boundary without a Rust buffer or a length argument.
    let address = |align| MemArg {
        offset: 0,
        align,
        memory_index: 1,
    };
    transfer("read_u8", READ_I32, 1, Op::I32Load8U(address(0)));
    transfer("write_u8", WRITE_I32, 2, Op::I32Store8(address(0)));
    transfer("read_u16", READ_I32, 1, Op::I32Load16U(address(1)));
    transfer("write_u16", WRITE_I32, 2, Op::I32Store16(address(1)));
    transfer("read_u32", READ_I32, 1, Op::I32Load(address(2)));
    transfer("write_u32", WRITE_I32, 2, Op::I32Store(address(2)));
    transfer("read_u64", READ_I64, 1, Op::I64Load(address(3)));
    transfer("write_u64", WRITE_I64, 2, Op::I64Store(address(3)));

    let mut module = Module::new();
    module.section(&types);
    module.section(&imports);
    module.section(&functions);
    module.section(&exports);
    module.section(&code);
    module.finish()
}
