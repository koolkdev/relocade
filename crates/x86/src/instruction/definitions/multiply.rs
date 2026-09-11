use super::*;
use crate::{
    alu::{
        flags::{AnyFlagSource, FlagSource},
        MultiplyOp, MultiplyType,
    },
    instruction::Location,
    register::{RegisterCode, RegisterType},
};

const UNSIGNED: IntegerHandlers<Handler> =
    unary_handlers!(implicit_multiply, Input, MultiplyOp::Unsigned);
const SIGNED: IntegerHandlers<Handler> =
    unary_handlers!(implicit_multiply, Input, MultiplyOp::Signed);

pub(super) const FORMS: [Form; 7] = [
    rm(0xf6, 4, SizedHandlers::fixed(UNSIGNED.byte)),
    rm(0xf7, 4, UNSIGNED.sized),
    rm(0xf6, 5, SizedHandlers::fixed(SIGNED.byte)),
    rm(0xf7, 5, SIGNED.sized),
    register_rm(
        OpcodeMap::Extended,
        0xaf,
        binary_handlers!(multiply_destination).sized,
        RegisterSide::Left,
    ),
    immediate_form(0x69, ImmediateWidth::OperandSize),
    immediate_form(0x6b, ImmediateWidth::SignedByte),
];

const fn immediate_form(opcode: u8, immediate: ImmediateWidth) -> Form {
    primary_form(
        opcode,
        Encoding::ModRm {
            immediate: Some(immediate),
        },
        ternary_handlers!(multiply_sources, sized),
        OperandBindingShape::Ternary {
            destination: LocationBinding::Register,
            first_source: OperandBinding::Location(LocationBinding::Rm),
            second_source: OperandBinding::Immediate,
        },
    )
}

fn implicit_multiply<T: RegisterType + MultiplyType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Input<T>,
    operation: MultiplyOp,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    let source = source.read(execution)?;
    let accumulator = TypedLocation::<T>::accumulator().read(execution)?;
    let outcome = operation.apply(accumulator, source);
    if T::BYTES == 1 {
        TypedLocation::<I16>::accumulator().write(execution, outcome.result.truncate::<I16>())?;
    } else {
        TypedLocation::<T>::accumulator().write(execution, outcome.result.truncate::<T>())?;
        // Register code two selects DX or EDX at the operand width.
        execution.write::<T>(
            Location::<Val<I32>>::Register(RegisterCode::from_code(2)),
            outcome.result.unsigned().shr(T::BYTES * 8).truncate::<T>(),
        )?;
    }
    execution.set_flags(outcome.flags)
}

fn multiply_destination<T: RegisterType + MultiplyType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    let source = source.read(execution)?;
    destination.update(execution, |execution, previous| {
        let outcome = MultiplyOp::Signed.apply(previous, source);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result.truncate::<T>())
    })
}

fn multiply_sources<T: RegisterType + MultiplyType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    first_source: Input<T>,
    second_source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    let left = first_source.read(execution)?;
    let right = second_source.read(execution)?;
    let outcome = MultiplyOp::Signed.apply(left, right);
    destination.write(execution, outcome.result.truncate::<T>())?;
    execution.set_flags(outcome.flags)
}
