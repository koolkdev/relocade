use wasm86_compiler::{Val, I32, I64, I8};

const GENERAL_PROTECTION: u64 = 2 << 48;
const PAGE_FAULT: u64 = 4 << 48;
const UNSUPPORTED_INSTRUCTION: u64 = 8 << 48;

pub(crate) fn page_fault(address: &Val<I32>, error: &Val<I32>) -> Val<I64> {
    address
        .unsigned()
        .extend::<I64>()
        .or(error.unsigned().extend::<I64>().shl(32))
        .or(PAGE_FAULT)
}

pub(crate) fn unsupported(address: &Val<I32>, opcode: &Val<I8>) -> Val<I64> {
    address
        .unsigned()
        .extend::<I64>()
        .or(opcode.unsigned().extend::<I64>().shl(32))
        .or(UNSUPPORTED_INSTRUCTION)
}

/// Encodes general protection with error code zero and no address payload.
pub(crate) fn general_protection() -> u64 {
    GENERAL_PROTECTION
}
