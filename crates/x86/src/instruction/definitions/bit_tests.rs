use super::*;
use crate::{
    alu::{BitTestOp, OperandUpdate},
    instruction::Operand,
    register::RegisterType,
};

instruction_families! {
    BT {
        execute: bit_test(BitTestOp::Test);
        forms {
            0x0F 0xA3 => word_or_dword(rm, modrm_reg);
            0x0F 0xBA /4 => word_or_dword(rm, imm8);
        }
    }
    BTS {
        execute: bit_test(BitTestOp::Set);
        forms {
            0x0F 0xAB => word_or_dword(rm, modrm_reg) lockable;
            0x0F 0xBA /5 => word_or_dword(rm, imm8) lockable;
        }
    }
    BTR {
        execute: bit_test(BitTestOp::Reset);
        forms {
            0x0F 0xB3 => word_or_dword(rm, modrm_reg) lockable;
            0x0F 0xBA /6 => word_or_dword(rm, imm8) lockable;
        }
    }
    BTC {
        execute: bit_test(BitTestOp::Complement);
        forms {
            0x0F 0xBB => word_or_dword(rm, modrm_reg) lockable;
            0x0F 0xBA /7 => word_or_dword(rm, imm8) lockable;
        }
    }
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
        execution.write_flags(operation.apply(input, offset).flags)
    } else {
        let target = destination.prepare_write(execution, &[])?;
        let mask = BitTestOp::mask::<T>(&offset);
        let update = match operation {
            BitTestOp::Set => OperandUpdate::Or(mask),
            BitTestOp::Reset => OperandUpdate::And(mask.xor(-1)),
            BitTestOp::Complement => OperandUpdate::Xor(mask),
            BitTestOp::Test => unreachable!("read-only bit tests do not modify their operand"),
        };
        let locked = execution.is_locked();
        target.modify(execution, update, locked, |execution, input| {
            execution.write_flags(operation.apply(input, offset).flags)
        })
    }
}
