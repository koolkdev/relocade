use super::*;
use crate::{alu::BitTestOp, instruction::Operand, register::RegisterType};

const fn bit_test_forms(
    opcode: u8,
    extension: u8,
    register_handlers: SizedHandlers<Handler>,
    immediate_handlers: SizedHandlers<Handler>,
) -> [Form; 2] {
    [
        register_rm(
            OpcodeMap::Extended,
            opcode,
            register_handlers,
            RegisterSide::Right,
        ),
        rm_immediate(
            OpcodeMap::Extended,
            0xba,
            extension,
            ImmediateWidth::Byte,
            immediate_handlers,
        ),
    ]
}

const FAMILIES: [[Form; 2]; 4] = [
    bit_test_forms(
        0xa3,
        4,
        binary_handlers!(bit_test, BitTestOp::Test).sized,
        binary_handlers!(bit_test, source = I8, sized, BitTestOp::Test),
    ),
    bit_test_forms(
        0xab,
        5,
        binary_handlers!(bit_test, BitTestOp::Set).sized,
        binary_handlers!(bit_test, source = I8, sized, BitTestOp::Set),
    ),
    bit_test_forms(
        0xb3,
        6,
        binary_handlers!(bit_test, BitTestOp::Reset).sized,
        binary_handlers!(bit_test, source = I8, sized, BitTestOp::Reset),
    ),
    bit_test_forms(
        0xbb,
        7,
        binary_handlers!(bit_test, BitTestOp::Complement).sized,
        binary_handlers!(bit_test, source = I8, sized, BitTestOp::Complement),
    ),
];

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    FAMILIES.iter().flat_map(|family| family.iter())
}

fn bit_test<T: RegisterType, O: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    offset: Input<O>,
    operation: BitTestOp,
) -> Result<(), BuildError>
where
    I32: AtLeast<T> + AtLeast<O>,
{
    let operand = offset.into_operand();
    let immediate = matches!(&operand, Operand::Immediate(_));
    let offset = execution.read::<O>(operand)?;
    let (destination, offset) = if immediate {
        (destination, offset.unsigned().extend::<I32>())
    } else {
        let offset = offset.signed().extend::<I32>();
        // The bit string consists of operand-sized units. A negative offset
        // selects a preceding unit; the original base need not be aligned.
        let unit = offset.signed().shr((T::BYTES * 8).trailing_zeros());
        let byte_offset = unit.shl(T::BYTES.trailing_zeros());
        (destination.offset_memory(byte_offset), offset)
    };
    if operation == BitTestOp::Test {
        let input = destination.read(execution)?;
        execution.set_flags(operation.apply(input, offset).flags)
    } else {
        destination.update(execution, |execution, input| {
            let outcome = operation.apply(input, offset);
            execution.set_flags(outcome.flags)?;
            Ok(outcome.result)
        })
    }
}
