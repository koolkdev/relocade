//! Memory ordering and cache control have no SIMD register operands.

use super::*;
use crate::address::{EffectiveAddress, MemoryAddress};

instruction_families! {
    LFENCE {
        execute: fence;
        forms { NP 0x0f 0xae @ 0xe8 + rm => no_operands(); }
    }
    MFENCE {
        execute: fence;
        forms { NP 0x0f 0xae @ 0xf0 + rm => no_operands(); }
    }
    SFENCE {
        execute: fence;
        forms { NP 0x0f 0xae @ 0xf8 + rm => no_operands(); }
    }
    PREFETCHNTA {
        execute: prefetch;
        forms { 0x0f 0x18 / 0 => operands(address); }
    }
    PREFETCHT0 {
        execute: prefetch;
        forms { 0x0f 0x18 / 1 => operands(address); }
    }
    PREFETCHT1 {
        execute: prefetch;
        forms { 0x0f 0x18 / 2 => operands(address); }
    }
    PREFETCHT2 {
        execute: prefetch;
        forms { 0x0f 0x18 / 3 => operands(address); }
    }
    CLFLUSH {
        execute: flush;
        forms { NP 0x0f 0xae / 7 => operands(mem); }
    }
}

fn fence(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.memory_fence();
    Ok(())
}

fn prefetch(
    _execution: &mut ExecutionBuilder<'_, '_>,
    _address: EffectiveAddress<Val<I32>>,
) -> Result<(), BuildError> {
    // A cache hint may be ignored. Decoding still consumes the entire address,
    // but no segment/page checks or data reads belong to an ignored prefetch.
    Ok(())
}

fn flush(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    execution.flush_cache_line(address)
}
