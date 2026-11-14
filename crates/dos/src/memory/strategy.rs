//! DOS 5 allocation strategy values, including the requested memory region.

use crate::DosError;

#[derive(Clone, Copy, Default)]
pub(crate) struct AllocationStrategy {
    pub(super) fit: Fit,
    region: Region,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Fit {
    #[default]
    First = 0,
    Best = 1,
    Last = 2,
}

#[derive(Clone, Copy, Default)]
enum Region {
    #[default]
    Conventional = 0,
    UpperOnly = 0x40,
    UpperFirst = 0x80,
}

impl AllocationStrategy {
    pub(crate) fn from_code(code: u16) -> Result<Self, DosError> {
        let fit = match code & 0x3f {
            0 => Fit::First,
            1 => Fit::Best,
            2 => Fit::Last,
            _ => return Err(DosError::InvalidFunction),
        };
        let region = match code & !0x3f {
            0 => Region::Conventional,
            0x40 => Region::UpperOnly,
            0x80 => Region::UpperFirst,
            _ => return Err(DosError::InvalidFunction),
        };
        Ok(Self { fit, region })
    }

    pub(crate) fn code(self) -> u16 {
        self.fit as u16 | self.region as u16
    }
}
