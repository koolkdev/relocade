//! Half-register transfers preserve the other half and use exact m64 accesses.

use super::*;
use crate::{address::MemoryAddress, memory::Intent, register::RegisterCode};
use relocade_compiler::I64;

#[derive(Clone, Copy)]
enum Half {
    Low,
    High,
}

impl Half {
    fn lane(self) -> u32 {
        match self {
            Self::Low => 0,
            Self::High => 1,
        }
    }
}

instruction_families! {
    MOVLPS_LOAD {
        execute: load_half(Half::Low);
        requires: [Sse];
        forms { NP 0x0f 0x12 => operands(xmm, mem); }
    }
    MOVLPS_STORE {
        execute: store_half(Half::Low);
        requires: [Sse];
        forms { NP 0x0f 0x13 => operands(mem, xmm); }
    }
    MOVLPD_LOAD {
        execute: load_half(Half::Low);
        requires: [Sse2];
        forms { P66 0x0f 0x12 => operands(xmm, mem); }
    }
    MOVLPD_STORE {
        execute: store_half(Half::Low);
        requires: [Sse2];
        forms { P66 0x0f 0x13 => operands(mem, xmm); }
    }
    MOVHPS_LOAD {
        execute: load_half(Half::High);
        requires: [Sse];
        forms { NP 0x0f 0x16 => operands(xmm, mem); }
    }
    MOVHPS_STORE {
        execute: store_half(Half::High);
        requires: [Sse];
        forms { NP 0x0f 0x17 => operands(mem, xmm); }
    }
    MOVHPD_LOAD {
        execute: load_half(Half::High);
        requires: [Sse2];
        forms { P66 0x0f 0x16 => operands(xmm, mem); }
    }
    MOVHPD_STORE {
        execute: store_half(Half::High);
        requires: [Sse2];
        forms { P66 0x0f 0x17 => operands(mem, xmm); }
    }
    MOVHLPS {
        execute: move_half(Half::Low, Half::High);
        requires: [Sse];
        forms { NP 0x0f 0x12 => operands(xmm, register(xmm_rm)); }
    }
    MOVLHPS {
        execute: move_half(Half::High, Half::Low);
        requires: [Sse];
        forms { NP 0x0f 0x16 => operands(xmm, register(xmm_rm)); }
    }
}

fn load_half(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MemoryAddress<Val<I32>>,
    half: Half,
) -> Result<(), BuildError> {
    let value = execution
        .memory_operand(source, 8, Intent::Read, &[])?
        .read::<I64>(execution, 0)?;
    let vector = execution.read_xmm(destination.clone())?;
    execution.write_xmm(destination, vector.replace_lane(half.lane(), value))
}

fn store_half(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
    source: RegisterCode,
    half: Half,
) -> Result<(), BuildError> {
    let value = execution.read_xmm(source)?.extract_lane::<I64>(half.lane());
    execution
        .memory_operand(destination, 8, Intent::Write, &[])?
        .write(execution, 0, value)
}

fn move_half(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: RegisterCode,
    destination_half: Half,
    source_half: Half,
) -> Result<(), BuildError> {
    let value = execution
        .read_xmm(source)?
        .extract_lane::<I64>(source_half.lane());
    let vector = execution.read_xmm(destination.clone())?;
    execution.write_xmm(
        destination,
        vector.replace_lane(destination_half.lane(), value),
    )
}
