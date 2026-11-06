//! Named registers for selective preservation and interrupt bindings.

use super::CpuStateAccess;
use relocade_x86::{CpuState, Segment, SegmentRegister};
use std::mem::offset_of;

/// A general-purpose register view or a complete segment register and cache.
/// Descriptors come from this module's constants and [`segment`]. Their widths
/// distinguish aliases: saving AX does not save the upper half of EAX.
#[derive(Clone, Copy, Debug)]
pub struct Register {
    pub(super) offset: usize,
    pub(super) bytes: usize,
}

macro_rules! register {
    ($get:ident, $set:ident, $type:ty, $field:ident, $byte:expr) => {
        #[allow(non_upper_case_globals)]
        pub const $get: Register = Register {
            offset: offset_of!(CpuState, registers.$field) + $byte,
            bytes: size_of::<$type>(),
        };

        impl CpuStateAccess {
            #[inline]
            pub fn $get(&self) -> $type {
                self.load($get.offset)
            }

            #[inline]
            pub fn $set(&self, value: $type) {
                self.store($get.offset, value);
            }
        }
    };
}

register!(eax, set_eax, u32, eax, 0);
register!(ecx, set_ecx, u32, ecx, 0);
register!(edx, set_edx, u32, edx, 0);
register!(ebx, set_ebx, u32, ebx, 0);
register!(esp, set_esp, u32, esp, 0);
register!(ebp, set_ebp, u32, ebp, 0);
register!(esi, set_esi, u32, esi, 0);
register!(edi, set_edi, u32, edi, 0);
register!(ax, set_ax, u16, eax, 0);
register!(cx, set_cx, u16, ecx, 0);
register!(dx, set_dx, u16, edx, 0);
register!(bx, set_bx, u16, ebx, 0);
register!(sp, set_sp, u16, esp, 0);
register!(bp, set_bp, u16, ebp, 0);
register!(si, set_si, u16, esi, 0);
register!(di, set_di, u16, edi, 0);
register!(al, set_al, u8, eax, 0);
register!(cl, set_cl, u8, ecx, 0);
register!(dl, set_dl, u8, edx, 0);
register!(bl, set_bl, u8, ebx, 0);
register!(ah, set_ah, u8, eax, 1);
register!(ch, set_ch, u8, ecx, 1);
register!(dh, set_dh, u8, edx, 1);
register!(bh, set_bh, u8, ebx, 1);

/// A segment result writes both the visible selector and its loaded cache.
pub const fn segment(segment: Segment) -> Register {
    Register {
        offset: super::segment_offset(segment),
        bytes: size_of::<SegmentRegister>(),
    }
}

/// Used by generated bindings in const evaluation, without a second alias table.
#[doc(hidden)]
pub const fn assert_disjoint(registers: &[Register]) {
    let mut i = 0;
    while i < registers.len() {
        let mut j = i + 1;
        while j < registers.len() {
            assert!(
                registers[i].offset + registers[i].bytes <= registers[j].offset
                    || registers[j].offset + registers[j].bytes <= registers[i].offset,
                "overlapping interrupt registers"
            );
            j += 1;
        }
        i += 1;
    }
}
