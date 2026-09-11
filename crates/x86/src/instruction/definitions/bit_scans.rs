use super::*;
use crate::{
    alu::{
        flags::{AnyFlagSource, FlagSource},
        BitScanOp,
    },
    register::RegisterType,
};

instruction_families! {
    BSF {
        execute: bit_scan(BitScanOp::Forward);
        forms {
            0x0F 0xBC => word_or_dword(modrm_reg, rm);
        }
    }
    BSR {
        execute: bit_scan(BitScanOp::Reverse);
        forms {
            0x0F 0xBD => word_or_dword(modrm_reg, rm);
        }
    }
}

fn bit_scan<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
    operation: BitScanOp,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    let source = source.read(execution)?;
    destination.update(execution, |execution, previous| {
        let outcome = operation.apply(source, previous);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}
