//! Registers selected by the 16-bit ModRM memory layout.

use crate::register::Gpr32;

// BP is always represented as the base so it also owns default SS selection.
// Mod=00, r/m=110 omits the base and encodes an absolute disp16 instead.
pub(super) const BASES: [Gpr32; 8] = [
    Gpr32::Ebx,
    Gpr32::Ebx,
    Gpr32::Ebp,
    Gpr32::Ebp,
    Gpr32::Esi,
    Gpr32::Edi,
    Gpr32::Ebp,
    Gpr32::Ebx,
];
pub(super) const INDICES: [Gpr32; 4] = [Gpr32::Esi, Gpr32::Edi, Gpr32::Esi, Gpr32::Edi];
