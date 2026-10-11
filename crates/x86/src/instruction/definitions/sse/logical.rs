//! Bitwise operations use all 128 bits without floating-point interpretation.

use super::*;
use crate::instruction::{VectorAlignment, XmmLocation};
use wasm86_compiler::V128;

#[derive(Clone, Copy)]
enum LogicalOperation {
    And,
    AndNot,
    Or,
    Xor,
}

instruction_families! {
    ANDPS {
        execute: logical(LogicalOperation::And);
        forms { NP 0x0f 0x54 => operands(xmm, xmm_rm); }
    }
    ANDPD {
        execute: logical(LogicalOperation::And);
        forms { P66 0x0f 0x54 => operands(xmm, xmm_rm); }
    }
    ANDNPS {
        execute: logical(LogicalOperation::AndNot);
        forms { NP 0x0f 0x55 => operands(xmm, xmm_rm); }
    }
    ANDNPD {
        execute: logical(LogicalOperation::AndNot);
        forms { P66 0x0f 0x55 => operands(xmm, xmm_rm); }
    }
    ORPS {
        execute: logical(LogicalOperation::Or);
        forms { NP 0x0f 0x56 => operands(xmm, xmm_rm); }
    }
    ORPD {
        execute: logical(LogicalOperation::Or);
        forms { P66 0x0f 0x56 => operands(xmm, xmm_rm); }
    }
    XORPS {
        execute: logical(LogicalOperation::Xor);
        forms { NP 0x0f 0x57 => operands(xmm, xmm_rm); }
    }
    XORPD {
        execute: logical(LogicalOperation::Xor);
        forms { P66 0x0f 0x57 => operands(xmm, xmm_rm); }
    }
    PAND {
        execute: logical(LogicalOperation::And);
        forms { P66 0x0f 0xdb => operands(xmm, xmm_rm); }
    }
    PANDN {
        execute: logical(LogicalOperation::AndNot);
        forms { P66 0x0f 0xdf => operands(xmm, xmm_rm); }
    }
    POR {
        execute: logical(LogicalOperation::Or);
        forms { P66 0x0f 0xeb => operands(xmm, xmm_rm); }
    }
    PXOR {
        execute: logical(LogicalOperation::Xor);
        forms { P66 0x0f 0xef => operands(xmm, xmm_rm); }
    }
}

fn logical(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: XmmLocation,
    source: XmmLocation,
    operation: LogicalOperation,
) -> Result<(), BuildError> {
    let XmmLocation::Register(register) = destination else {
        unreachable!("vector logical instructions have an XMM destination")
    };
    let right = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    let left = execution.read_xmm(register.clone())?;
    let result = match operation {
        LogicalOperation::And => left.and(right),
        LogicalOperation::AndNot => left.xor(u128::MAX).and(right),
        LogicalOperation::Or => left.or(right),
        LogicalOperation::Xor => left.xor(right),
    };
    execution.write_xmm(register, result)
}
