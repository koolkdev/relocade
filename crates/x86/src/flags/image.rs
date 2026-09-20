//! Architectural flag images, independent of the stored flag record.
//!
//! Stack transfers use a fixed CPL3/IOPL0 model with IF set and VM/RF/VIF/VIP clear.
//! Writes ignore fixed and unrepresented bits. Transferring TF, NT, AC or ID does
//! not enable debug delivery, task switching, alignment checks or interrupts.

use wasm86_compiler::{Val, I1, I32};

use super::{Flag, FlagChange, StatusFlag};

/// A flag roster and fixed bits shared by image reads and masked writes.
pub(crate) struct FlagImage<const N: usize> {
    flags: [Flag; N],
    fixed: u32,
}

pub(crate) const AH: FlagImage<5> = FlagImage {
    flags: [Flag::CF, Flag::PF, Flag::AF, Flag::ZF, Flag::SF],
    fixed: 0x02,
};

/// Stack images expose bit 1 and IF as set in the fixed CPL3/IOPL0 model.
pub(crate) const WORD: FlagImage<9> = FlagImage {
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
    fixed: 0x0202,
};

/// The dword image additionally transfers AC and ID.
pub(crate) const DWORD: FlagImage<11> = FlagImage {
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
    fixed: 0x0202,
};

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
        Flag::DF => 10,
        Flag::NT => 14,
        Flag::AC => 18,
        Flag::ID => 21,
    }
}
