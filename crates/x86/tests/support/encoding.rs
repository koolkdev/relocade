//! Shared checks for the exact extent of an instruction encoding.

use wasm86_x86::{compile_block_from_bytes, BlockError, CompiledModule};
use wasmparser::Validator;

/// Rejects an invalid form before either decoder requests unavailable trailing bytes.
#[track_caller]
pub(crate) fn check_unsupported_at_page_end(code: &[u8], opcode: u8) {
    use super::{
        machine::{Exit, Image},
        step::TestModule,
    };
    let address = 0x2000 - code.len() as u32;
    assert_eq!(
        compile_block_from_bytes(address, code, 1).err(),
        Some(BlockError::UnsupportedInstruction { address, opcode }),
    );
    let mut image = Image::new(&[]);
    image.cpu.eip = address;
    image.data(address + 0x2000, code);
    image.check_unchanged_exit(
        TestModule::interpreter(),
        "invalid form is rejected before fetching trailing bytes",
        Exit::Other(0x0008_0000_0000_0000 | (u64::from(opcode) << 32) | u64::from(address)),
    );
}

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
