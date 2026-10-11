//! Half-register transfers preserve the other half and use exact m64 accesses.

use super::*;
use crate::{
    address::MemoryAddress,
    instruction::{VectorAlignment, XmmLocation},
    memory::Intent,
};
use wasm86_compiler::{I64, V128};

#[derive(Clone, Copy)]
enum Half {
    Low,
    High,
}

impl Half {
    fn lane(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::High => 1,
        }
    }
}

instruction_families! {
    MOVLPS_LOAD {
        execute: load_half(Half::Low);
        forms { NP 0x0f 0x12 => operands(xmm, mem); }
    }
    MOVLPS_STORE {
        execute: store_half(Half::Low);
        forms { NP 0x0f 0x13 => operands(mem, xmm); }
    }
    MOVLPD_LOAD {
        execute: load_half(Half::Low);
        forms { P66 0x0f 0x12 => operands(xmm, mem); }
    }
    MOVLPD_STORE {
        execute: store_half(Half::Low);
        forms { P66 0x0f 0x13 => operands(mem, xmm); }
    }
    MOVHPS_LOAD {
        execute: load_half(Half::High);
        forms { NP 0x0f 0x16 => operands(xmm, mem); }
    }
    MOVHPS_STORE {
        execute: store_half(Half::High);
        forms { NP 0x0f 0x17 => operands(mem, xmm); }
    }
    MOVHPD_LOAD {
        execute: load_half(Half::High);
        forms { P66 0x0f 0x16 => operands(xmm, mem); }
    }
    MOVHPD_STORE {
        execute: store_half(Half::High);
        forms { P66 0x0f 0x17 => operands(mem, xmm); }
    }
    MOVHLPS {
        execute: move_half(Half::Low, Half::High);
        forms { NP 0x0f 0x12 @ register => operands(xmm, xmm_rm); }
    }
    MOVLHPS {
        execute: move_half(Half::High, Half::Low);
        forms { NP 0x0f 0x16 @ register => operands(xmm, xmm_rm); }
    }
}

fn load_half(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: MemoryAddress<Val<I32>>,
    half: Half,
) -> Result<(), BuildError> {
    let value = execution
        .memory_operand(source, 8, Intent::Read, &[])?
        .read::<I64>(execution, 0)?;
    let XmmLocation::Register(register) = destination else {
        unreachable!("half loads have an XMM destination")
    };
    let vector = execution.read_xmm(register.clone())?;
    execution.write_xmm(register, vector.replace_lane(half.lane(), value))
}

fn store_half(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
    source: XmmLocation,
    half: Half,
) -> Result<(), BuildError> {
    let value = source
        .read::<V128>(execution, VectorAlignment::Unaligned)?
        .extract_lane::<I64>(half.lane());
    execution
        .memory_operand(destination, 8, Intent::Write, &[])?
        .write(execution, 0, value)
}

fn move_half(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
    destination_half: Half,
    source_half: Half,
) -> Result<(), BuildError> {
    let value = source
        .read::<V128>(execution, VectorAlignment::Unaligned)?
        .extract_lane::<I64>(source_half.lane());
    let XmmLocation::Register(register) = destination else {
        unreachable!("half register moves have an XMM destination")
    };
    let vector = execution.read_xmm(register.clone())?;
    execution.write_xmm(
        register,
        vector.replace_lane(destination_half.lane(), value),
    )
}
