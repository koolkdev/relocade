//! Transfer policies around concrete register operations. Device borrowing, if
//! needed, ends inside the first operation before a sequence visits another port.

use super::{PortRead, PortValue, PortWrite};
use crate::IoContext;
use std::ops::RangeInclusive;

impl PortRead {
    /// Rejects unsupported widths before invoking the concrete register callback.
    pub(super) fn fixed_width<T: PortValue>(read: impl Fn(IoContext<'_>) -> T + 'static) -> Self {
        Self::new(move |context, _, requested| {
            if requested == T::BYTES {
                read(context).into()
            } else {
                u32::MAX
            }
        })
    }

    /// Reads the first byte directly, then visits subsequent ports through the
    /// same context in little-endian order. Bytes beyond `window` read as FF;
    /// a transfer never wraps from FFFF to port zero. Bind only inside `window`.
    pub(super) fn byte_sequence(
        window: RangeInclusive<u16>,
        read: impl Fn(IoContext<'_>) -> u8 + 'static,
    ) -> Self {
        assert!(!window.is_empty(), "empty byte port window");
        if window.start() == window.end() {
            return Self::new(move |context, _, _| u32::from(read(context)) | 0xffff_ff00);
        }
        let last = u32::from(*window.end());
        Self::new(move |mut context, port, bytes| {
            let mut value = u32::from(read(context.reborrow()));
            for index in 1..bytes {
                let address = u32::from(port) + u32::from(index);
                let byte = if address <= last {
                    context.read_port(address as u16, 1)
                } else {
                    0xff
                };
                value |= byte << (index * 8);
            }
            value
        })
    }
}

impl PortWrite {
    pub(super) fn fixed_width<T: PortValue>(write: impl Fn(IoContext<'_>, T) + 'static) -> Self {
        Self::new(move |context, _, requested, value| {
            if requested == T::BYTES {
                write(context, T::from_u32(value));
            }
        })
    }

    /// Writes bytes in address order within `window`. Each operation completes
    /// before looking up the next, so an index or mode write can change the next
    /// byte's handler. Bytes past the window are ignored, without address wrap.
    /// Bind only inside `window`.
    pub(super) fn byte_sequence(
        window: RangeInclusive<u16>,
        write: impl Fn(IoContext<'_>, u8) + 'static,
    ) -> Self {
        assert!(!window.is_empty(), "empty byte port window");
        if window.start() == window.end() {
            return Self::new(move |context, _, _, value| write(context, value as u8));
        }
        let last = u32::from(*window.end());
        Self::new(move |mut context, port, bytes, value| {
            write(context.reborrow(), value as u8);
            for index in 1..bytes {
                let address = u32::from(port) + u32::from(index);
                if address <= last {
                    context.write_port(address as u16, 1, value >> (index * 8));
                }
            }
        })
    }
}
