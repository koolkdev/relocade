//! Names and logical types shared by generated x86 modules and their embedding.

use relocade_compiler::{
    abi::{Function, Memory, Table},
    I1, I16, I32, I64,
};

pub const EXECUTION_CONTEXT: Memory = Memory {
    shared: true,
    maximum: Some(1),
    ..Memory::new("relocade", "executionContext", 1)
};
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
/// Real-mode hooks and compiled blocks, keyed by packed 16:16 CS:IP.
pub const EXECUTION_ENTRIES: Memory = Memory::new("relocade", "executionEntries", 1);
/// CPU-local call observations. The embedding drains these while execution is stopped.
pub const CALL_PROFILE: Memory = Memory::new("relocade", "callProfile", 1);
pub type ExecutionSignature = fn() -> crate::ExecutionExit;
pub type InterruptSignature = fn(vector: u8) -> crate::ExecutionExit;
pub type FarCallSignature = fn(segment: u16, offset: u16, stack_bytes: u16) -> crate::ExecutionExit;
pub const EXECUTION_FUNCTIONS: Table<ExecutionSignature> =
    Table::new("relocade", "executionFunctions", 2);

pub const DISPATCH: Function<ExecutionSignature> = Function::new("relocade", "dispatch");
/// Resolves a completed call once. Only Real16 records unresolved callees, using
/// packed 16:16 CS:IP keys; protected-mode calls resolve without profiling.
/// Published CPU state must remain compatible with the dispatcher's profile.
/// Check execution controls before resolving; stopped calls are not profiled.
/// Returns the callable slot and whether recording can continue without draining.
pub const RESOLVE_CALL: Function<fn() -> (u32, bool)> = Function::new("relocade", "resolveCall");
pub const INTERPRET: Function<ExecutionSignature> = Function::new("relocade", "interpret");
pub const RUN: Function<ExecutionSignature> = Function::new("relocade", "run");
pub const STEP: Function<ExecutionSignature> = Function::new("relocade", "step");
/// Real16 architectural entry; success returns InterruptEntered before ISR dispatch.
/// The embedding owns maskable-interrupt eligibility and acknowledgement.
pub const INTERRUPT: Function<InterruptSignature> = Function::new("relocade", "interrupt");
/// Real16 far-call entry; success returns FarCallEntered before guest dispatch.
/// Reserves stack_bytes above the saved CS:IP, checking the complete frame.
/// The host fills the reserved bytes before dispatch; flags remain unchanged.
pub const FAR_CALL: Function<FarCallSignature> = Function::new("relocade", "farCall");

/// One architectural port read (1, 2 or 4 bytes); the caller consumes the low bits.
pub type PortReadSignature = fn(I16, I32) -> I32;
/// One architectural port write (1, 2 or 4 bytes), with unused value bits zero.
pub type PortWriteSignature = fn(I16, I32, I32);
pub const PORT_READS: Table<PortReadSignature> = {
    let mut table = Table::new("relocade", "portReads", 65536);
    table.maximum = Some(65536);
    table
};
pub const PORT_WRITES: Table<PortWriteSignature> = {
    let mut table = Table::new("relocade", "portWrites", 65536);
    table.maximum = Some(65536);
    table
};
/// A physical-bus transfer of one through eight bytes, including partial widths.
pub type MmioReadSignature = fn(I32, I32) -> I64;
pub type MmioWriteSignature = fn(I32, I32, I64);
pub const MMIO_READS: Table<MmioReadSignature> = Table::new("relocade", "mmioReads", 1);
pub const MMIO_WRITES: Table<MmioWriteSignature> = Table::new("relocade", "mmioWrites", 1);

/// Status, error code, cache fields, and whether the load needs completion.
pub type ResolveSegmentResults = (I32, I32, I32, I32, I16, I1, I1, I1, I1, I1, I1, I1);
pub type ResolveSegmentSignature = fn(I32, I16) -> ResolveSegmentResults;
pub type QuerySegmentDescriptorSignature = fn(u16) -> crate::SegmentDescriptorInfo;

pub const RESOLVE_SEGMENT: Function<ResolveSegmentSignature> =
    Function::new("relocade", "resolveSegment");
pub const QUERY_SEGMENT_DESCRIPTOR: Function<QuerySegmentDescriptorSignature> =
    Function::new("relocade", "querySegmentDescriptor");

/// Completes a resolved selector's load after all instruction guards succeed.
pub const COMPLETE_SEGMENT_LOAD: Function<fn(I16)> =
    Function::new("relocade", "completeSegmentLoad");
