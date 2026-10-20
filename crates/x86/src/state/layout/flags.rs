//! Native access to the stored flag record and its represented EFLAGS bits.

use super::codec::{read, read_u32, write};
use super::{FlagBytes, FlagsState, StatusSourceState};
use std::mem::{offset_of, size_of};

impl FlagsState {
    pub const BYTE_LEN: usize = size_of::<Self>();

    /// Reads raw backing bytes, including inactive fields and snapshot padding.
    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            status_source: StatusSourceState {
                kind: bytes[offset_of!(FlagsState, status_source.kind)],
                reserved: read(&bytes, offset_of!(FlagsState, status_source.reserved)),
                left: read_u32(&bytes, offset_of!(FlagsState, status_source.left)),
                right: read_u32(&bytes, offset_of!(FlagsState, status_source.right)),
            },
            bytes: FlagBytes {
                cf: bytes[offset_of!(FlagsState, bytes.cf)],
                pf: bytes[offset_of!(FlagsState, bytes.pf)],
                af: bytes[offset_of!(FlagsState, bytes.af)],
                zf: bytes[offset_of!(FlagsState, bytes.zf)],
                sf: bytes[offset_of!(FlagsState, bytes.sf)],
                of: bytes[offset_of!(FlagsState, bytes.of)],
                tf: bytes[offset_of!(FlagsState, bytes.tf)],
                df: bytes[offset_of!(FlagsState, bytes.df)],
                nt: bytes[offset_of!(FlagsState, bytes.nt)],
                ac: bytes[offset_of!(FlagsState, bytes.ac)],
                id: bytes[offset_of!(FlagsState, bytes.id)],
                if_: bytes[offset_of!(FlagsState, bytes.if_)],
                iopl: bytes[offset_of!(FlagsState, bytes.iopl)],
                reserved: read(&bytes, offset_of!(FlagsState, bytes.reserved)),
            },
        }
    }

    /// Preserves every stored byte without interpreting the flag source.
    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        bytes[offset_of!(FlagsState, status_source.kind)] = self.status_source.kind;
        write(
            &mut bytes,
            offset_of!(FlagsState, status_source.reserved),
            &self.status_source.reserved,
        );
        write(
            &mut bytes,
            offset_of!(FlagsState, status_source.left),
            &self.status_source.left.to_le_bytes(),
        );
        write(
            &mut bytes,
            offset_of!(FlagsState, status_source.right),
            &self.status_source.right.to_le_bytes(),
        );
        bytes[offset_of!(FlagsState, bytes.cf)] = self.bytes.cf;
        bytes[offset_of!(FlagsState, bytes.pf)] = self.bytes.pf;
        bytes[offset_of!(FlagsState, bytes.af)] = self.bytes.af;
        bytes[offset_of!(FlagsState, bytes.zf)] = self.bytes.zf;
        bytes[offset_of!(FlagsState, bytes.sf)] = self.bytes.sf;
        bytes[offset_of!(FlagsState, bytes.of)] = self.bytes.of;
        bytes[offset_of!(FlagsState, bytes.tf)] = self.bytes.tf;
        bytes[offset_of!(FlagsState, bytes.df)] = self.bytes.df;
        bytes[offset_of!(FlagsState, bytes.nt)] = self.bytes.nt;
        bytes[offset_of!(FlagsState, bytes.ac)] = self.bytes.ac;
        bytes[offset_of!(FlagsState, bytes.id)] = self.bytes.id;
        bytes[offset_of!(FlagsState, bytes.if_)] = self.bytes.if_;
        bytes[offset_of!(FlagsState, bytes.iopl)] = self.bytes.iopl;
        write(
            &mut bytes,
            offset_of!(FlagsState, bytes.reserved),
            &self.bytes.reserved,
        );
        bytes
    }

    /// Reads represented EFLAGS bits without changing backing state. This is a
    /// host observation; it does not apply PUSHF's profile-specific IF/IOPL rules.
    /// An unknown status-source kind is an invalid execution-state invariant.
    pub fn eflags(&self) -> u32 {
        let mut flags = self.bytes;
        if self.status_source.kind != 0 {
            let [cf, pf, af, zf, sf, of] = self.status_source.resolve();
            flags.cf = cf;
            flags.pf = pf;
            flags.af = af;
            flags.zf = zf;
            flags.sf = sf;
            flags.of = of;
        }
        let mut image = 2 | (u32::from(flags.iopl & 3) << 12);
        for (value, bit) in flag_bits(&mut flags) {
            image |= u32::from(*value & 1) << bit;
        }
        image
    }

    /// Replaces represented flags and clears the lazy source. Fixed and
    /// unrepresented EFLAGS bits are ignored; inactive payload and padding remain.
    pub fn set_eflags(&mut self, value: u32) {
        for (byte, bit) in flag_bits(&mut self.bytes) {
            *byte = ((value >> bit) & 1) as u8;
        }
        self.bytes.iopl = ((value >> 12) & 3) as u8;
        self.status_source.kind = 0;
    }
}

fn flag_bits(flags: &mut FlagBytes) -> [(&mut u8, u32); 12] {
    [
        (&mut flags.cf, 0),
        (&mut flags.pf, 2),
        (&mut flags.af, 4),
        (&mut flags.zf, 6),
        (&mut flags.sf, 7),
        (&mut flags.of, 11),
        (&mut flags.tf, 8),
        (&mut flags.if_, 9),
        (&mut flags.df, 10),
        (&mut flags.nt, 14),
        (&mut flags.ac, 18),
        (&mut flags.id, 21),
    ]
}

impl StatusSourceState {
    fn resolve(self) -> [u8; 6] {
        let bits = match self.kind {
            1..=3 => 8,
            5..=7 => 16,
            9..=11 => 32,
            _ => panic!("invalid stored status source kind: {}", self.kind),
        };
        let mask = u32::MAX >> (32 - bits);
        let sign = 1_u32 << (bits - 1);
        let left = self.left & mask;
        let right = self.right & mask;
        let (result, carry, overflow) = match self.kind & 3 {
            1 => {
                let result = left.wrapping_sub(right) & mask;
                (
                    result,
                    left < right,
                    (left ^ right) & (left ^ result) & sign != 0,
                )
            }
            2 => {
                let result = left.wrapping_add(right) & mask;
                (
                    result,
                    result < left,
                    (left ^ result) & (right ^ result) & sign != 0,
                )
            }
            3 => (left, false, false),
            _ => unreachable!(),
        };
        [
            u8::from(carry),
            u8::from((result as u8).count_ones().is_multiple_of(2)),
            u8::from(self.kind & 3 != 3 && (left ^ right ^ result) & 0x10 != 0),
            u8::from(result == 0),
            u8::from(result & sign != 0),
            u8::from(overflow),
        ]
    }
}
