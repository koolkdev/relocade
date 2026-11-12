//! Transfer adapters around concrete port operations. Device borrowing, if
//! needed, ends inside the first operation before a sequence visits another port.

use super::{PortRead, PortWrite};
use crate::IoContext;
use std::ops::RangeInclusive;

impl PortRead {
    /// Accepts exactly one byte. Two- and four-byte accesses return FF bytes
    /// without invoking the callback or borrowing its device.
    pub fn byte(read: impl Fn(IoContext<'_>) -> u8 + 'static) -> Self {
        Self::new(move |context, _, bytes| {
            if bytes == 1 {
                u32::from(read(context))
            } else {
                u32::MAX
            }
        })
    }

    /// Accepts exactly two bytes, invoking the callback once for the complete
    /// word. One- and four-byte accesses return FF bytes without invoking it.
    pub fn word(read: impl Fn(IoContext<'_>) -> u16 + 'static) -> Self {
        Self::new(move |context, _, bytes| {
            if bytes == 2 {
                u32::from(read(context))
            } else {
                u32::MAX
            }
        })
    }

    /// Accepts one, two or four bytes, invoking the byte callback exactly once.
    /// Upper requested bytes read as FF; no other port is accessed. A callback
    /// returning 5A produces 5A, FF5A or FFFFFF5A at the respective bus widths.
    /// Bits outside the requested width are discarded by the bus.
    pub fn low_byte(read: impl Fn(IoContext<'_>) -> u8 + 'static) -> Self {
        Self::new(move |context, _, _| u32::from(read(context)) | 0xffff_ff00)
    }

    /// Invokes the first byte callback directly, then reads successive addresses
    /// through their current mappings. Each callback finishes before resolving
    /// the next handler; a device borrow must end inside that callback.
    ///
    /// The initiating operation owns continuation: every trailing request has
    /// width one and uses the original `window`, even after mappings change.
    /// A trailing [`Self::low_byte`] does not stop the sequence; a trailing
    /// [`Self::word`] rejects its byte request without combining further bytes.
    /// Bytes beyond the window read as FF, without wrapping from FFFF to zero.
    /// Bind only inside `window`; a singleton window uses [`Self::low_byte`].
    pub fn byte_sequence(
        window: RangeInclusive<u16>,
        read: impl Fn(IoContext<'_>) -> u8 + 'static,
    ) -> Self {
        assert!(!window.is_empty(), "empty byte port window");
        if window.start() == window.end() {
            return Self::low_byte(read);
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
    /// Accepts exactly one byte. Two- and four-byte writes are ignored entirely,
    /// without invoking the callback or borrowing its device.
    pub fn byte(write: impl Fn(IoContext<'_>, u8) + 'static) -> Self {
        Self::new(move |context, _, bytes, value| {
            if bytes == 1 {
                write(context, value as u8);
            }
        })
    }

    /// Accepts exactly two bytes, invoking the callback once with the complete
    /// word. One- and four-byte writes are ignored without invoking it.
    pub fn word(write: impl Fn(IoContext<'_>, u16) + 'static) -> Self {
        Self::new(move |context, _, bytes, value| {
            if bytes == 2 {
                write(context, value as u16);
            }
        })
    }

    /// Accepts one, two or four bytes, invoking the byte callback exactly once
    /// with the low byte. Upper bytes are discarded; no other port is accessed.
    pub fn low_byte(write: impl Fn(IoContext<'_>, u8) + 'static) -> Self {
        Self::new(move |context, _, _, value| write(context, value as u8))
    }

    /// Invokes the first byte callback directly, then writes successive bytes
    /// through their current mappings. Each callback finishes before resolving
    /// the next handler; a device borrow must end inside that callback.
    ///
    /// The initiating operation owns continuation: every trailing request has
    /// width one and uses the original `window`, even after mappings change.
    /// A trailing [`Self::low_byte`] does not stop the sequence; a trailing
    /// [`Self::word`] rejects its byte request without combining further bytes.
    /// Bytes beyond the window are ignored, without wrapping from FFFF to zero.
    /// Bind only inside `window`; a singleton window uses [`Self::low_byte`].
    pub fn byte_sequence(
        window: RangeInclusive<u16>,
        write: impl Fn(IoContext<'_>, u8) + 'static,
    ) -> Self {
        assert!(!window.is_empty(), "empty byte port window");
        if window.start() == window.end() {
            return Self::low_byte(write);
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
