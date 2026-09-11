use super::*;
use crate::{
    alu::{DivideOp, DoubleWidth},
    instruction::{Location, Operand},
    register::{RegisterCode, RegisterType},
    state::exit,
};

const UNSIGNED: IntegerHandlers<Handler> = unary_handlers!(divide, Input, DivideOp::Unsigned);
const SIGNED: IntegerHandlers<Handler> = unary_handlers!(divide, Input, DivideOp::Signed);

pub(super) const FORMS: [Form; 4] = [
    rm(0xf6, 6, SizedHandlers::fixed(UNSIGNED.byte)),
    rm(0xf7, 6, UNSIGNED.sized),
    rm(0xf6, 7, SizedHandlers::fixed(SIGNED.byte)),
    rm(0xf7, 7, SIGNED.sized),
];

fn divide<T: RegisterType + DoubleWidth>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Input<T>,
    operation: DivideOp,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let divisor = source.read(execution)?;
    // Byte division uses all of original AX. Wider forms concatenate DX:AX or
    // EDX:EAX without interpreting the low half's sign independently.
    let dividend = if T::BYTES == 1 {
        TypedLocation::<I16>::accumulator()
            .read(execution)?
            .unsigned()
            .extend::<T::Double>()
    } else {
        let low = TypedLocation::<T>::accumulator().read(execution)?;
        let high = execution.read::<T>(Operand::<Val<I32>>::Location(Location::Register(
            RegisterCode::from_code(2),
        )))?;
        low.unsigned()
            .extend::<T::Double>()
            .or(high.unsigned().extend::<T::Double>().shl(T::BYTES * 8))
    };
    execution.fault_if(
        operation.input_fault(&dividend, &divisor),
        exit::divide_error(),
    )?;
    let result = operation.apply(dividend, divisor);
    if let Some(overflow) = result.overflow {
        execution.fault_if(overflow, exit::divide_error())?;
    }
    if T::BYTES == 1 {
        // Both byte results share AX, preserving the rest of EAX in one write.
        let ax = result.quotient.unsigned().extend::<T::Double>().or(result
            .remainder
            .unsigned()
            .extend::<T::Double>()
            .shl(8));
        TypedLocation::<I16>::accumulator().write(execution, ax.truncate::<I16>())?;
    } else {
        TypedLocation::<T>::accumulator().write(execution, result.quotient)?;
        execution.write::<T>(
            Location::<Val<I32>>::Register(RegisterCode::from_code(2)),
            result.remainder,
        )?;
    }
    // All status flags are undefined; preserve their incoming record as policy.
    Ok(())
}
