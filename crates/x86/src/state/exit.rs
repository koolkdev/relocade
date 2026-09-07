use wasm86_compiler::{Val, I32, I64, I8};

const PAGE_FAULT: u64 = 4 << 48;
const UNSUPPORTED_INSTRUCTION: u64 = 8 << 48;
const INSTRUCTION_FETCH_ERROR: u64 = 0x10 << 32;

pub(crate) fn page_fault(address: &Val<I32>) -> Val<I64> {
    address
        .unsigned()
        .extend::<I64>()
        .or(PAGE_FAULT | INSTRUCTION_FETCH_ERROR)
}

pub(crate) fn unsupported(address: &Val<I32>, opcode: &Val<I8>) -> Val<I64> {
    address
        .unsigned()
        .extend::<I64>()
        .or(opcode.unsigned().extend::<I64>().shl(32))
        .or(UNSUPPORTED_INSTRUCTION)
}
