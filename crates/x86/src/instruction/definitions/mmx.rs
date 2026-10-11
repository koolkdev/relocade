//! MMX instructions use raw physical x87 payloads and shared exception admission.

use super::*;
use crate::instruction::MmxLocation;
use wasm86_compiler::{I16, I64, V128};

instruction_families! {
    PCMPEQB {
        execute: packed_binary(|left, right| left.eq_lanes::<I8>(right));
        forms { NP 0x0f 0x74 => operands(mm, mm_rm); }
    }
    PCMPEQW {
        execute: packed_binary(|left, right| left.eq_lanes::<I16>(right));
        forms { NP 0x0f 0x75 => operands(mm, mm_rm); }
    }
    PCMPEQD {
        execute: packed_binary(|left, right| left.eq_lanes::<I32>(right));
        forms { NP 0x0f 0x76 => operands(mm, mm_rm); }
    }
    PCMPGTB {
        execute: packed_binary(|left, right| left.signed_gt_lanes::<I8>(right));
        forms { NP 0x0f 0x64 => operands(mm, mm_rm); }
    }
    PCMPGTW {
        execute: packed_binary(|left, right| left.signed_gt_lanes::<I16>(right));
        forms { NP 0x0f 0x65 => operands(mm, mm_rm); }
    }
    PCMPGTD {
        execute: packed_binary(|left, right| left.signed_gt_lanes::<I32>(right));
        forms { NP 0x0f 0x66 => operands(mm, mm_rm); }
    }

    PADDB {
        execute: packed_binary(|left, right| left.add_lanes::<I8>(right));
        forms { NP 0x0f 0xfc => operands(mm, mm_rm); }
    }
    PADDW {
        execute: packed_binary(|left, right| left.add_lanes::<I16>(right));
        forms { NP 0x0f 0xfd => operands(mm, mm_rm); }
    }
    PADDD {
        execute: packed_binary(|left, right| left.add_lanes::<I32>(right));
        forms { NP 0x0f 0xfe => operands(mm, mm_rm); }
    }
    PSUBB {
        execute: packed_binary(|left, right| left.sub_lanes::<I8>(right));
        forms { NP 0x0f 0xf8 => operands(mm, mm_rm); }
    }
    PSUBW {
        execute: packed_binary(|left, right| left.sub_lanes::<I16>(right));
        forms { NP 0x0f 0xf9 => operands(mm, mm_rm); }
    }
    PSUBD {
        execute: packed_binary(|left, right| left.sub_lanes::<I32>(right));
        forms { NP 0x0f 0xfa => operands(mm, mm_rm); }
    }

    PAND {
        execute: binary(|left, right| left.and(right));
        forms { NP 0x0f 0xdb => operands(mm, mm_rm); }
    }
    PANDN {
        execute: binary(|left, right| left.xor(u64::MAX).and(right));
        forms { NP 0x0f 0xdf => operands(mm, mm_rm); }
    }
    POR {
        execute: binary(|left, right| left.or(right));
        forms { NP 0x0f 0xeb => operands(mm, mm_rm); }
    }
    PXOR {
        execute: binary(|left, right| left.xor(right));
        forms { NP 0x0f 0xef => operands(mm, mm_rm); }
    }

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

fn binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MmxLocation,
    source: MmxLocation,
    operation: impl FnOnce(Val<I64>, Val<I64>) -> Val<I64>,
) -> Result<(), BuildError> {
    let MmxLocation::Register(register) = destination else {
        unreachable!("binary MMX operations have a register destination")
    };
    execution.execute_mmx(|execution| {
        let right = source.read::<I64>(execution)?;
        let left = execution.read_mmx(register.clone())?;
        execution.write_mmx(register, operation(left, right))
    })
}

fn packed_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MmxLocation,
    source: MmxLocation,
    operation: impl FnOnce(Val<V128>, Val<V128>) -> Val<V128>,
) -> Result<(), BuildError> {
    binary(execution, destination, source, |left, right| {
        let left = Val::<V128>::from(0_u128).replace_lane(0, left);
        let right = Val::<V128>::from(0_u128).replace_lane(0, right);
        operation(left, right).extract_lane::<I64>(0)
    })
}
