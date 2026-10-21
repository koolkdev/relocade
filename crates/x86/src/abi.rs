//! Names and logical types shared by generated x86 modules and their embedding.

use relocade_compiler::{
    abi::{Function, Memory, Table},
    I1, I16, I32, I64,
};

pub const EXECUTION_CONTEXT: Memory = Memory::new("relocade", "executionContext", 1);
/// Guest backing is shared with host devices. The maximum is the Wasm32 address
/// space; each machine still chooses its initial allocation.
pub const GUEST: Memory = Memory {
    maximum: Some(65536),
    shared: true,
    ..Memory::new("relocade", "guest", 1)
};
pub const PHYSICAL_MAP: Memory = Memory::new("relocade", "physicalMap", 1);
pub const PAGE_TABLE: Memory = Memory::new(
    "relocade",
    "machine",
    crate::memory::page_table::BYTE_LEN / 65536,
);
pub const EXECUTION_ENTRIES: Memory = Memory::new("relocade", "executionEntries", 1);
pub const EXECUTION_FUNCTIONS: Table<fn() -> I64> = Table::new("relocade", "executionFunctions", 2);

pub const DISPATCH: Function<fn() -> I64> = Function::new("relocade", "dispatch");
pub const INTERPRET: Function<fn() -> I64> = Function::new("relocade", "interpret");
pub const RUN: Function<fn() -> I64> = Function::new("relocade", "run");
pub const STEP: Function<fn() -> I64> = Function::new("relocade", "step");

pub const READ_PORT: Function<fn(I16, I32) -> I32> = Function::new("relocade", "readPort");
pub const WRITE_PORT: Function<fn(I16, I32, I32)> = Function::new("relocade", "writePort");
pub const READ_MMIO: Function<fn(I32, I32) -> I64> = Function::new("relocade", "readMmio");
pub const WRITE_MMIO: Function<fn(I32, I32, I64)> = Function::new("relocade", "writeMmio");

/// Status, error code, cache fields, and whether the load needs completion.
pub type ResolveSegmentResults = (I32, I32, I32, I32, I16, I1, I1, I1, I1, I1, I1, I1);
pub type ResolveSegmentSignature = fn(I32, I16) -> ResolveSegmentResults;
pub type QuerySegmentDescriptorSignature = fn(I16) -> (I1, I1, I1, I1, I32, I32);

pub const RESOLVE_SEGMENT: Function<ResolveSegmentSignature> =
    Function::new("relocade", "resolveSegment");
pub const QUERY_SEGMENT_DESCRIPTOR: Function<QuerySegmentDescriptorSignature> =
    Function::new("relocade", "querySegmentDescriptor");

/// Completes a resolved selector's load after all instruction guards succeed.
pub const COMPLETE_SEGMENT_LOAD: Function<fn(I16)> =
    Function::new("relocade", "completeSegmentLoad");
