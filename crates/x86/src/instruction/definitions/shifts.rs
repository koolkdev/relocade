use super::*;
use crate::{
    flags::{FlagSource, LocalFlagSource, ShiftKind},
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

const FAMILIES: [[Form; 6]; 3] = [
    shift_forms(4, binary_handlers!(shift, source = I8, ShiftKind::Left)),
    shift_forms(
        5,
        binary_handlers!(shift, source = I8, ShiftKind::RightUnsigned),
    ),
    shift_forms(
        7,
        binary_handlers!(shift, source = I8, ShiftKind::RightSigned),
    ),
];

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    FAMILIES.iter().flat_map(|family| family.iter())
}

fn shift<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    count: Input<I8>,
    kind: ShiftKind,
) -> Result<(), BuildError>
where
    FlagSource<T>: Into<LocalFlagSource>,
{
    destination.update(execution, |execution, input| {
        let count = count.read(execution)?.and(31).unsigned().extend::<I32>();
        let flags = FlagSource::shift(kind, input, count.clone());
        let result = flags.result().clone();
        execution.set_flags_if(count.ne(0), flags)?;
        Ok(result)
    })
}
