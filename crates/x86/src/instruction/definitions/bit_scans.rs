use super::*;
use crate::{
    alu::{
        flags::{AnyFlagSource, FlagSource},
        BitScanOp,
    },
    register::RegisterType,
};

pub(super) const FORMS: [Form; 2] = [
    register_rm(
        OpcodeMap::Extended,
        0xbc,
        binary_handlers!(bit_scan, BitScanOp::Forward).sized,
        RegisterSide::Left,
    ),
    register_rm(
        OpcodeMap::Extended,
        0xbd,
        binary_handlers!(bit_scan, BitScanOp::Reverse).sized,
        RegisterSide::Left,
    ),
];

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
