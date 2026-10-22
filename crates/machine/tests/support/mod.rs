#![allow(dead_code)]
use relocade_machine as machine;
mod configuration;
#[allow(unused_imports)]
pub use configuration::*;

pub fn initializing_module(offset: u32, bytes: &[u8]) -> Vec<u8> {
    use wasm_encoder::{ConstExpr, DataSection, ImportSection, MemoryType, Module};
    let mut module = Module::new();
    let mut imports = ImportSection::new();
    imports.import(
        "relocade",
        "guest",
        MemoryType {
            minimum: 1,
            maximum: Some(65536),
            memory64: false,
            shared: true,
            page_size_log2: None,
        },
    );
    module.section(&imports);
    let mut data = DataSection::new();
    data.active(
        0,
        &ConstExpr::i32_const(offset as i32),
        bytes.iter().copied(),
    );
    module.section(&data);
    module.finish()
}
