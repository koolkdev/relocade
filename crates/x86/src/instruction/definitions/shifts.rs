use super::*;
use crate::{
    alu::{
        flags::{AnyFlagSource, FlagSource},
        RotateDirection, ShiftOp,
    },
    register::RegisterType,
};

const fn implicit_count(
    opcode: u8,
    extension: u8,
    handlers: SizedHandlers<Handler>,
    count: OperandBinding,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::Rm,
        handlers,
        OperandBindingShape::Binary {
            left: LocationBinding::Rm,
            right: count,
        },
    );
    form.extension = Some(extension);
    form
}

const fn shift_forms(extension: u8, handlers: IntegerHandlers<Handler>) -> [Form; 6] {
    let byte = SizedHandlers::fixed(handlers.byte);
    let one = OperandBinding::Constant(1);
    let cl = OperandBinding::Location(LocationBinding::CountRegister);
    [
        implicit_count(0xd0, extension, byte, one),
        implicit_count(0xd1, extension, handlers.sized, one),
        implicit_count(0xd2, extension, byte, cl),
        implicit_count(0xd3, extension, handlers.sized, cl),
        rm_immediate(0xc0, extension, ImmediateWidth::Byte, byte),
        rm_immediate(0xc1, extension, ImmediateWidth::Byte, handlers.sized),
    ]
}

const FAMILIES: [[Form; 6]; 7] = [
    shift_forms(
        0,
        binary_handlers!(rotate, source = I8, RotateDirection::Left),
    ),
    shift_forms(
        1,
        binary_handlers!(rotate, source = I8, RotateDirection::Right),
    ),
    shift_forms(
        2,
        binary_handlers!(rotate_through_carry, source = I8, RotateDirection::Left),
    ),
    shift_forms(
        3,
        binary_handlers!(rotate_through_carry, source = I8, RotateDirection::Right),
    ),
    shift_forms(4, binary_handlers!(shift, source = I8, ShiftOp::Left)),
    shift_forms(
        5,
        binary_handlers!(shift, source = I8, ShiftOp::RightLogical),
    ),
    shift_forms(
        7,
        binary_handlers!(shift, source = I8, ShiftOp::RightArithmetic),
    ),
];

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    FAMILIES.iter().flat_map(|family| family.iter())
}

fn rotate<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    direction: RotateDirection,
) -> Result<(), BuildError> {
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let outcome = direction.rotate(input, count.clone());
        execution.set_flags_if(count.ne(0), outcome.flags)?;
        Ok(outcome.result)
    })
}

fn rotate_through_carry<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    direction: RotateDirection,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let carry = execution.condition(Condition::B)?;
        let outcome = direction.rotate_through_carry(input, count.clone(), carry);
        execution.set_flags_if(count.ne(0), outcome.flags)?;
        Ok(outcome.result)
    })
}

fn shift<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    operation: ShiftOp,
) -> Result<(), BuildError>
where
    FlagSource<T>: Into<AnyFlagSource>,
{
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let outcome = operation.apply(input, count.clone());
        execution.set_flags_if(count.ne(0), outcome.flags)?;
        Ok(outcome.result)
    })
}
