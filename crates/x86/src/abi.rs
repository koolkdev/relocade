//! Names and logical types shared by generated x86 modules and their embedding.

use relocade_compiler::{
    abi::{Function, Memory, Table},
    I1, I16, I32, I64,
};

pub const EXECUTION_CONTEXT: Memory = Memory::new("relocade", "executionContext", 1);
pub const GUEST: Memory = Memory::new("relocade", "guest", 1);
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

/// Status, error code, cache fields, and accessed-byte offset. The final offset
/// is u32::MAX when no accessed-bit update is needed.
pub type ResolveSegmentResults = (I32, I32, I32, I32, I16, I1, I1, I1, I1, I1, I1, I32);
pub type ResolveSegmentSignature = fn(I32, I16) -> ResolveSegmentResults;
pub type QuerySegmentDescriptorSignature = fn(I16) -> (I1, I1, I1, I1, I32, I32);

pub const RESOLVE_SEGMENT: Function<ResolveSegmentSignature> =
    Function::new("relocade", "resolveSegment");
pub const QUERY_SEGMENT_DESCRIPTOR: Function<QuerySegmentDescriptorSignature> =
    Function::new("relocade", "querySegmentDescriptor");

/// Nonfaulting update in dedicated descriptor storage, after successful load guards.
pub const MARK_SEGMENT_ACCESSED: Function<fn(I32)> =
    Function::new("relocade", "markSegmentAccessed");
