//! MMX instructions use raw physical x87 payloads and shared exception admission.

use super::*;
use crate::instruction::MmxLocation;
use wasm86_compiler::I64;

instruction_families! {
    MOVD_LOAD {
        execute: load_doubleword;
        forms { NP 0x0f 0x6e => dword(mm, rm); }
    }
    MOVD_STORE {
        execute: store_doubleword;
        forms { NP 0x0f 0x7e => dword(rm, mm); }
    }
    MOVQ {
        execute: move_quadword;
        forms {
            NP 0x0f 0x6f => operands(mm, mm_rm);
            NP 0x0f 0x7f => operands(mm_rm, mm);
        }
    }
    EMMS {
        execute: empty;
        forms { NP 0x0f 0x77 => no_operands(); }
    }
}

fn load_doubleword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MmxLocation,
    source: TypedLocation<I32>,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = source.read(execution)?.unsigned().extend::<I64>();
        destination.write(execution, value)
    })
}

fn store_doubleword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I32>,
    source: MmxLocation,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = source.read::<I32>(execution)?;
        destination.write(execution, value)
    })
}

fn move_quadword(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MmxLocation,
    source: MmxLocation,
) -> Result<(), BuildError> {
    execution.execute_mmx(|execution| {
        let value = source.read::<I64>(execution)?;
        destination.write(execution, value)
    })
}

fn empty(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.empty_mmx()
}
