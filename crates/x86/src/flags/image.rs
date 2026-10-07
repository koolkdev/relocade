//! Architectural flag images, independent of the stored flag record.
//!
//! Protected transfers fix IF=1 and IOPL=0; real mode reads and writes both.
//! VM/RF/VIF/VIP are unrepresented and stay clear. Transferring TF, IF, NT, AC or ID does
//! not enable debug delivery, task switching, alignment checks or interrupts.

use wasm86_compiler::{AtLeast, BuildError, IntType, Type, Val, I1, I32, I8};

use super::{Flag, FlagChange, StatusFlag};
use crate::{execution::ExecutionBuilder, ExecutionProfile};

/// A flag roster and fixed bits shared by image reads and masked writes.
pub(crate) struct FlagImage<const N: usize> {
    flags: [Flag; N],
    fixed: u32,
}

pub(crate) const AH: FlagImage<5> = FlagImage {
    flags: [Flag::CF, Flag::PF, Flag::AF, Flag::ZF, Flag::SF],
    fixed: 0x02,
};

/// Common writable low flags. IF and IOPL depend on the execution profile.
const WORD: FlagImage<9> = FlagImage {
    flags: [
        Flag::CF,
        Flag::PF,
        Flag::AF,
        Flag::ZF,
        Flag::SF,
        Flag::OF,
        Flag::TF,
        Flag::DF,
        Flag::NT,
    ],
    fixed: 0x02,
};

/// The dword image additionally transfers AC and ID.
const DWORD: FlagImage<11> = FlagImage {
    flags: [
        Flag::CF,
        Flag::PF,
        Flag::AF,
        Flag::ZF,
        Flag::SF,
        Flag::OF,
        Flag::TF,
        Flag::DF,
        Flag::NT,
        Flag::AC,
        Flag::ID,
    ],
    fixed: 0x02,
};

/// Packs the represented FLAGS/EFLAGS without changing their backing state.
pub(crate) fn read_stack_image<T: IntType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<Val<T>, BuildError>
where
    I32: AtLeast<T>,
{
    let image = match T::TYPE {
        Type::I16 => WORD.pack(execution.read_flags(WORD.flags())?),
        Type::I32 => DWORD.pack(execution.read_flags(DWORD.flags())?),
        _ => unreachable!("stack flag images use word or dword operands"),
    };
    let image = match execution.profile() {
        ExecutionProfile::Protected(_) => image.or(0x0200),
        ExecutionProfile::Real16 => image
            .or(execution
                .read_flag(Flag::IF)?
                .unsigned()
                .extend::<I32>()
                .shl(bit(Flag::IF)))
            .or(execution.read_iopl()?.unsigned().extend::<I32>().shl(12)),
    };
    Ok(image.truncate::<T>())
}

/// Restores writable flags; word images leave AC/ID unchanged. RF remains
/// unrepresented even for IRETD; debug delivery is outside this execution model.
pub(crate) fn write_stack_image<T: IntType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    image: &Val<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let image = image.unsigned().extend::<I32>();
    let change = match T::TYPE {
        Type::I16 => WORD.change(&image),
        Type::I32 => DWORD.change(&image),
        _ => unreachable!("stack flag images use word or dword operands"),
    };
    execution.write_flags(change)?;
    match execution.profile() {
        ExecutionProfile::Protected(_) => Ok(()),
        ExecutionProfile::Real16 => {
            execution.write_flag(
                Flag::IF,
                image.unsigned().shr(bit(Flag::IF)).truncate::<I1>(),
            )?;
            execution.write_iopl(image.unsigned().shr(12).truncate::<I8>())
        }
    }
}

impl<const N: usize> FlagImage<N> {
    pub(crate) fn flags(&self) -> [Flag; N] {
        self.flags
    }

    pub(crate) fn pack(&self, values: [Val<I1>; N]) -> Val<I32> {
        let mut image: Val<I32> = self.fixed.into();
        for (flag, value) in self.flags.into_iter().zip(values) {
            image = image.or(value.unsigned().extend::<I32>().shl(bit(flag)));
        }
        image
    }

    /// Fixed and unrepresented bits do not alter any modeled flag.
    pub(crate) fn change(&self, image: &Val<I32>) -> FlagChange {
        FlagChange::partial(
            self.flags
                .map(|flag| (flag, image.unsigned().shr(bit(flag)).truncate::<I1>())),
        )
    }
}

fn bit(flag: Flag) -> u32 {
    match flag {
        Flag::Status(StatusFlag::CF) => 0,
        Flag::Status(StatusFlag::PF) => 2,
        Flag::Status(StatusFlag::AF) => 4,
        Flag::Status(StatusFlag::ZF) => 6,
        Flag::Status(StatusFlag::SF) => 7,
        Flag::Status(StatusFlag::OF) => 11,
        Flag::TF => 8,
        Flag::IF => 9,
        Flag::DF => 10,
        Flag::NT => 14,
        Flag::AC => 18,
        Flag::ID => 21,
    }
}
