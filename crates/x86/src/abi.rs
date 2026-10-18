//! Names and logical types shared by generated x86 modules and their embedding.

use wasm86_compiler::{
    abi::{Function, Memory, Table},
    I1, I16, I32, I64,
};

pub const EXECUTION_CONTEXT: Memory = Memory::new("wasm86", "executionContext", 1);
pub const GUEST: Memory = Memory::new("wasm86", "guest", 1);
pub const PHYSICAL_MAP: Memory = Memory::new("wasm86", "physicalMap", 1);
pub const PAGE_TABLE: Memory = Memory::new(
    "wasm86",
    "machine",
    crate::memory::page_table::BYTE_LEN / 65536,
);
pub const EXECUTION_ENTRIES: Memory = Memory::new("wasm86", "executionEntries", 1);
pub const EXECUTION_FUNCTIONS: Table<fn() -> I64> = Table::new("wasm86", "executionFunctions", 2);

pub const DISPATCH: Function<fn() -> I64> = Function::new("wasm86", "dispatch");
pub const INTERPRET: Function<fn() -> I64> = Function::new("wasm86", "interpret");
pub const RUN: Function<fn() -> I64> = Function::new("wasm86", "run");
pub const STEP: Function<fn() -> I64> = Function::new("wasm86", "step");

pub const READ_PORT: Function<fn(I16, I32) -> I32> = Function::new("wasm86", "readPort");
pub const WRITE_PORT: Function<fn(I16, I32, I32)> = Function::new("wasm86", "writePort");
pub const READ_MMIO: Function<fn(I32, I32) -> I64> = Function::new("wasm86", "readMmio");
pub const WRITE_MMIO: Function<fn(I32, I32, I64)> = Function::new("wasm86", "writeMmio");

pub type ResolveSegmentSignature = fn(I32, I16) -> ([I32; 4], I16, [I1; 6]);
pub type QuerySegmentDescriptorSignature = fn(I16) -> (I1, I1, I1, I32, I32);

pub const RESOLVE_SEGMENT: Function<ResolveSegmentSignature> =
    Function::new("wasm86", "resolveSegment");
pub const QUERY_SEGMENT_DESCRIPTOR: Function<QuerySegmentDescriptorSignature> =
    Function::new("wasm86", "querySegmentDescriptor");
