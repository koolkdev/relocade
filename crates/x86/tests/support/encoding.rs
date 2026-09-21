//! Shared checks for the exact extent of an instruction encoding.

use wasm86_x86::{compile_block_from_bytes, BlockError, CompiledModule};
use wasmparser::Validator;

/// Every supplied byte is required, and a one-instruction block consumes no
/// successor bytes. Returns the complete block for additional boundary checks.
#[track_caller]
pub(crate) fn check_length(code: &[u8]) -> CompiledModule {
    assert!(!code.is_empty() && code.len() <= 15);
    for available in 0..code.len() {
        assert_eq!(
            compile_block_from_bytes(0x1000, &code[..available], 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: 0x1000,
                available,
            }),
            "{code:02x?}, available {available}",
        );
    }
    let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
    Validator::new().validate_all(&complete.bytes).unwrap();
    assert_eq!(
        compile_block_from_bytes(0x1000, &[code, &[0x0f]].concat(), 1)
            .unwrap()
            .bytes,
        complete.bytes,
        "{code:02x?}: a successor must not change a one-instruction block",
    );
    complete
}
