//! Transfer policy is resolved while preparing a concrete port callback.

use super::{PortRead, PortWrite};
use crate::IoContext;
use std::{marker::PhantomData, ops::RangeInclusive};

/// The logical value of a port register. Only byte and word registers are supported.
/// This sealed trait supplies constant-width conversions to the Wasm carrier.
pub trait PortValue: sealed::Sealed + Copy + Into<u32> + 'static {
    #[doc(hidden)]
    const BYTES: u8;
    #[doc(hidden)]
    fn from_u32(value: u32) -> Self;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
}

impl PortValue for u8 {
    const BYTES: u8 = 1;
    #[inline]
    fn from_u32(value: u32) -> Self {
        value as Self
    }
}

impl PortValue for u16 {
    const BYTES: u8 = 2;
    #[inline]
    fn from_u32(value: u32) -> Self {
        value as Self
    }
}

/// How a transfer addresses byte or word registers. Choose a policy when claiming
/// a port or window; its endpoints and prepared handlers retain that policy.
/// Policy selection adds no lookup or dispatch to ordinary accesses.
#[derive(Clone, Copy)]
pub struct PortPolicy<T> {
    sequential: bool,
    value: PhantomData<T>,
}

impl PortPolicy<u8> {
    /// Accept byte transfers only; other widths return open bus or ignore writes.
    pub const BYTE: Self = Self {
        sequential: false,
        value: PhantomData,
    };

    /// Visit successive byte registers, stopping at the owning window's end.
    /// Missing and out-of-window bytes read as FF and ignore writes. On a single
    /// port this calls the register once and supplies FF/ignored trailing bytes.
    pub const BYTE_SEQUENCE: Self = Self {
        sequential: true,
        value: PhantomData,
    };
}

impl PortPolicy<u16> {
    /// Accept one complete word per register; other widths do not call the device.
    pub const WORD: Self = Self {
        sequential: false,
        value: PhantomData,
    };
}

impl<T: PortValue> PortPolicy<T> {
    pub(super) fn read(
        self,
        window: RangeInclusive<u16>,
        read: impl Fn(IoContext<'_>) -> T + 'static,
    ) -> PortRead {
        if self.sequential {
            PortRead::byte_sequence(window, move |context| read(context).into() as u8)
        } else {
            PortRead::fixed_width(read)
        }
    }

    pub(super) fn write(
        self,
        window: RangeInclusive<u16>,
        write: impl Fn(IoContext<'_>, T) + 'static,
    ) -> PortWrite {
        if self.sequential {
            PortWrite::byte_sequence(window, move |context, value| {
                write(context, T::from_u32(value.into()));
            })
        } else {
            PortWrite::fixed_width(write)
        }
    }
}
