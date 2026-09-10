use super::*;
use crate::{
    execution::PairValues,
    flags::{ArithmeticKind, FlagSource, LocalFlagSource},
    register::RegisterType,
};

const XCHG: IntegerHandlers<Handler> = binary_handlers!(xchg, right = TypedLocation);
const XADD: IntegerHandlers<Handler> = binary_handlers!(xadd, right = TypedLocation);
const CMPXCHG: IntegerHandlers<Handler> = binary_handlers!(cmpxchg);

const fn accumulator_register() -> Form {
    let mut form = primary_form(
        0x90,
        Encoding::OpcodeRegister,
        XCHG.sized,
        OperandBindingShape::Binary {
            left: LocationBinding::Accumulator,
            right: OperandBinding::Location(LocationBinding::Register),
        },
    );
    form.mask = 0xf8;
    form
}

pub(super) const FORMS: [Form; 7] = [
    register_rm(
        OpcodeMap::Primary,
        0x86,
        SizedHandlers::fixed(XCHG.byte),
        RegisterSide::Right,
    ),
    register_rm(OpcodeMap::Primary, 0x87, XCHG.sized, RegisterSide::Right),
    accumulator_register(),
    register_rm(
        OpcodeMap::Extended,
        0xc0,
        SizedHandlers::fixed(XADD.byte),
        RegisterSide::Right,
    ),
    register_rm(OpcodeMap::Extended, 0xc1, XADD.sized, RegisterSide::Right),
    register_rm(
        OpcodeMap::Extended,
        0xb0,
        SizedHandlers::fixed(CMPXCHG.byte),
        RegisterSide::Right,
    ),
    register_rm(
        OpcodeMap::Extended,
        0xb1,
        CMPXCHG.sized,
        RegisterSide::Right,
    ),
];

fn xchg<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: TypedLocation<T>,
) -> Result<(), BuildError> {
    left.update_pair(execution, right, |_, old| {
        Ok(PairValues {
            left: old.right,
            right: old.left,
        })
    })
}

fn xadd<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: TypedLocation<T>,
) -> Result<(), BuildError>
where
    FlagSource<T>: Into<LocalFlagSource>,
{
    destination.update_pair(execution, source, |execution, old| {
        let flags = FlagSource::arithmetic(ArithmeticKind::Add, old.left.clone(), old.right);
        let sum = flags.result().clone();
        execution.set_flags(flags)?;
        Ok(PairValues {
            left: sum,
            right: old.left,
        })
    })
}

fn cmpxchg<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<LocalFlagSource>,
{
    destination.update_pair(execution, TypedLocation::accumulator(), |execution, old| {
        let replacement = source.read(execution)?;
        let equal = old.right.eq(&old.left);
        execution.set_flags(FlagSource::arithmetic(
            ArithmeticKind::Sub,
            old.right.clone(),
            old.left.clone(),
        ))?;
        Ok(PairValues {
            left: equal.select(replacement, &old.left),
            right: equal.select(old.right, old.left),
        })
    })
}
