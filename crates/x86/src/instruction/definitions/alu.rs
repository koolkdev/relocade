use super::*;
use crate::{
    alu::{
        flags::{AnyFlagSource, FlagSource},
        ArithmeticOp, LogicOp, UnaryOp,
    },
    register::RegisterType,
};

#[derive(Clone, Copy)]
enum BinaryOperation {
    Add,
    AddWithCarry,
    Subtract,
    SubtractWithBorrow,
    And,
    Or,
    Xor,
}

/// The main ALU families share operand layouts. Bits 3–5 of the first
/// opcode also select their extension in groups 80, 81 and 83.
struct AluFamily {
    register_rm: [Form; 4],
    accumulator_immediate: [Form; 2],
    rm_immediate: [Form; 3],
}

const fn alu_family(first_opcode: u8, handlers: IntegerHandlers<Handler>) -> AluFamily {
    let extension = first_opcode >> 3;
    let byte = SizedHandlers::fixed(handlers.byte);
    AluFamily {
        register_rm: [
            register_rm(OpcodeMap::Primary, first_opcode, byte, RegisterSide::Right),
            register_rm(
                OpcodeMap::Primary,
                first_opcode + 1,
                handlers.sized,
                RegisterSide::Right,
            ),
            register_rm(
                OpcodeMap::Primary,
                first_opcode + 2,
                byte,
                RegisterSide::Left,
            ),
            register_rm(
                OpcodeMap::Primary,
                first_opcode + 3,
                handlers.sized,
                RegisterSide::Left,
            ),
        ],
        accumulator_immediate: [
            accumulator_immediate(first_opcode + 4, ImmediateWidth::Byte, byte),
            accumulator_immediate(
                first_opcode + 5,
                ImmediateWidth::OperandSize,
                handlers.sized,
            ),
        ],
        rm_immediate: [
            rm_immediate(
                OpcodeMap::Primary,
                0x80,
                extension,
                ImmediateWidth::Byte,
                byte,
            ),
            rm_immediate(
                OpcodeMap::Primary,
                0x81,
                extension,
                ImmediateWidth::OperandSize,
                handlers.sized,
            ),
            rm_immediate(
                OpcodeMap::Primary,
                0x83,
                extension,
                ImmediateWidth::SignedByte,
                handlers.sized,
            ),
        ],
    }
}

const ALU_FAMILIES: [AluFamily; 8] = [
    alu_family(0x00, binary_handlers!(update_binary, BinaryOperation::Add)),
    alu_family(
        0x10,
        binary_handlers!(update_binary, BinaryOperation::AddWithCarry),
    ),
    alu_family(
        0x18,
        binary_handlers!(update_binary, BinaryOperation::SubtractWithBorrow),
    ),
    alu_family(0x38, binary_handlers!(compare)),
    alu_family(
        0x28,
        binary_handlers!(update_binary, BinaryOperation::Subtract),
    ),
    alu_family(0x20, binary_handlers!(update_binary, BinaryOperation::And)),
    alu_family(0x08, binary_handlers!(update_binary, BinaryOperation::Or)),
    alu_family(0x30, binary_handlers!(update_binary, BinaryOperation::Xor)),
];

const TEST: IntegerHandlers<Handler> = binary_handlers!(test);
const TEST_MODRM_FORMS: [Form; 4] = [
    register_rm(
        OpcodeMap::Primary,
        0x84,
        SizedHandlers::fixed(TEST.byte),
        RegisterSide::Right,
    ),
    register_rm(OpcodeMap::Primary, 0x85, TEST.sized, RegisterSide::Right),
    rm_immediate(
        OpcodeMap::Primary,
        0xf6,
        0,
        ImmediateWidth::Byte,
        SizedHandlers::fixed(TEST.byte),
    ),
    rm_immediate(
        OpcodeMap::Primary,
        0xf7,
        0,
        ImmediateWidth::OperandSize,
        TEST.sized,
    ),
];

const TEST_ACCUMULATOR_FORMS: [Form; 2] = [
    accumulator_immediate(0xa8, ImmediateWidth::Byte, SizedHandlers::fixed(TEST.byte)),
    accumulator_immediate(0xa9, ImmediateWidth::OperandSize, TEST.sized),
];

const INCREMENT: IntegerHandlers<Handler> =
    unary_handlers!(update_unary, TypedLocation, UnaryOp::Increment);
const DECREMENT: IntegerHandlers<Handler> =
    unary_handlers!(update_unary, TypedLocation, UnaryOp::Decrement);
const NOT: IntegerHandlers<Handler> = unary_handlers!(update_unary, TypedLocation, UnaryOp::Not);
const NEGATE: IntegerHandlers<Handler> =
    unary_handlers!(update_unary, TypedLocation, UnaryOp::Negate);

const UNARY_FORMS: [Form; 10] = [
    opcode_register(0x40, INCREMENT.sized),
    opcode_register(0x48, DECREMENT.sized),
    rm(0xfe, 0, SizedHandlers::fixed(INCREMENT.byte)),
    rm(0xff, 0, INCREMENT.sized),
    rm(0xfe, 1, SizedHandlers::fixed(DECREMENT.byte)),
    rm(0xff, 1, DECREMENT.sized),
    rm(0xf6, 2, SizedHandlers::fixed(NOT.byte)),
    rm(0xf7, 2, NOT.sized),
    rm(0xf6, 3, SizedHandlers::fixed(NEGATE.byte)),
    rm(0xf7, 3, NEGATE.sized),
];

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    ALU_FAMILIES
        .iter()
        .flat_map(|family| family.register_rm.iter())
        .chain(
            ALU_FAMILIES
                .iter()
                .flat_map(|family| family.rm_immediate.iter()),
        )
        .chain(TEST_MODRM_FORMS.iter())
        .chain(
            ALU_FAMILIES
                .iter()
                .flat_map(|family| family.accumulator_immediate.iter()),
        )
        .chain(TEST_ACCUMULATOR_FORMS.iter())
        .chain(UNARY_FORMS.iter())
}

fn update_binary<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
    operation: BinaryOperation,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    destination.update(execution, |execution, left| {
        let right = source.read(execution)?;
        let outcome = match operation {
            BinaryOperation::Add => ArithmeticOp::Add.apply(left, right),
            BinaryOperation::Subtract => ArithmeticOp::Subtract.apply(left, right),
            BinaryOperation::AddWithCarry => {
                let carry = execution.condition(Condition::B)?;
                ArithmeticOp::Add.apply_with_carry(left, right, carry)
            }
            BinaryOperation::SubtractWithBorrow => {
                let carry = execution.condition(Condition::B)?;
                ArithmeticOp::Subtract.apply_with_carry(left, right, carry)
            }
            BinaryOperation::And => LogicOp::And.apply(left, right),
            BinaryOperation::Or => LogicOp::Or.apply(left, right),
            BinaryOperation::Xor => LogicOp::Xor.apply(left, right),
        };
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn compare<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    let left = left.read(execution)?;
    let right = right.read(execution)?;
    execution.set_flags(ArithmeticOp::Subtract.apply(left, right).flags)
}

fn test<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<AnyFlagSource>,
{
    let left = left.read(execution)?;
    let right = right.read(execution)?;
    execution.set_flags(LogicOp::And.apply(left, right).flags)
}

fn update_unary<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    operation: UnaryOp,
) -> Result<(), BuildError>
where
    FlagSource<T>: Into<AnyFlagSource>,
{
    destination.update(execution, |execution, input| {
        let outcome = operation.apply(input);
        execution.set_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}
