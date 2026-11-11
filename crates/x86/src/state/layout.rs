//! CPU backing fields shared by generated code and host state snapshots.

mod codec;
mod flags;
mod segments;
mod simd;
mod tables;
mod x87;

pub use segments::{SegmentRegister, Segments};
pub use simd::SimdState;
pub use tables::{DescriptorTableRegister, SystemSegmentRegister};
pub use x87::{X87Control, X87Register, X87State, X87Status};

#[cfg(test)]
mod tests;

use std::{
    mem::size_of,
    ops::{Index, IndexMut},
};

use crate::{register::Gpr32, PrivilegeLevel};

/// Architectural operating mode derived from CR0.PE and EFLAGS.VM.
/// Representing a mode does not imply that generated entries can execute it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuMode {
    Real,
    Protected,
    /// Represented in snapshots; virtual-8086 execution is not implemented.
    Virtual8086,
}

impl CpuMode {
    pub const fn from_control(cr0: u32, virtual_8086: bool) -> Self {
        if cr0 & 1 == 0 {
            Self::Real
        } else if virtual_8086 {
            Self::Virtual8086
        } else {
            Self::Protected
        }
    }

    /// The loaded CS selector reflects CPL in protected mode, including when
    /// executing conforming code. Descriptor DPL is not the current privilege.
    pub const fn privilege_level(self, cs_selector: u16) -> PrivilegeLevel {
        match self {
            Self::Real => PrivilegeLevel::Ring0,
            Self::Virtual8086 => PrivilegeLevel::Ring3,
            Self::Protected => match cs_selector & 3 {
                0 => PrivilegeLevel::Ring0,
                1 => PrivilegeLevel::Ring1,
                2 => PrivilegeLevel::Ring2,
                _ => PrivilegeLevel::Ring3,
            },
        }
    }
}

/// Backing descriptor for the six arithmetic status flags.
/// Kind zero reads their stored bytes. Other supported kinds derive their values
/// from the payload; control and system flags always use their stored bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatusSourceState {
    /// Zero selects stored bits. Subtraction uses 1/5/9, addition 2/6/10,
    /// and logic 3/7/11 for byte/word/dword values, respectively.
    pub kind: u8,
    /// Snapshot padding, retained without interpretation.
    pub reserved: [u8; 3],
    /// Left arithmetic operand or logical result; unused for kind zero.
    pub left: u32,
    /// Right arithmetic operand; unused for kind zero and logic.
    pub right: u32,
}

/// Named backing bytes for the represented x86 flags, in snapshot order.
/// Each flag uses its low bit; IOPL uses its low two bits. Status bytes can be
/// stale while a status source is active; snapshots retain every byte without
/// interpreting or normalizing it.
/// This byte record is not the architectural EFLAGS bit encoding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagBytes {
    pub cf: u8,
    pub pf: u8,
    pub af: u8,
    pub zf: u8,
    pub sf: u8,
    pub of: u8,
    pub tf: u8,
    pub df: u8,
    pub nt: u8,
    pub ac: u8,
    pub id: u8,
    pub if_: u8,
    pub iopl: u8,
    /// EFLAGS.VM. Retained even though virtual-8086 execution is unsupported.
    pub vm: u8,
    /// Snapshot padding, not architectural flags.
    pub reserved: [u8; 2],
}

/// Flag backing state, including inactive source operands and stale flag bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagsState {
    pub status_source: StatusSourceState,
    pub bytes: FlagBytes,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Registers {
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esp: u32,
    pub ebp: u32,
    pub esi: u32,
    pub edi: u32,
}

impl Index<Gpr32> for Registers {
    type Output = u32;

    fn index(&self, register: Gpr32) -> &Self::Output {
        match register {
            Gpr32::Eax => &self.eax,
            Gpr32::Ecx => &self.ecx,
            Gpr32::Edx => &self.edx,
            Gpr32::Ebx => &self.ebx,
            Gpr32::Esp => &self.esp,
            Gpr32::Ebp => &self.ebp,
            Gpr32::Esi => &self.esi,
            Gpr32::Edi => &self.edi,
        }
    }
}

impl IndexMut<Gpr32> for Registers {
    fn index_mut(&mut self, register: Gpr32) -> &mut Self::Output {
        match register {
            Gpr32::Eax => &mut self.eax,
            Gpr32::Ecx => &mut self.ecx,
            Gpr32::Edx => &mut self.edx,
            Gpr32::Ebx => &mut self.ebx,
            Gpr32::Esp => &mut self.esp,
            Gpr32::Ebp => &mut self.ebp,
            Gpr32::Esi => &mut self.esi,
            Gpr32::Edi => &mut self.edi,
        }
    }
}

/// CPU backing state. Byte conversion is explicitly little endian on every host.
/// `Default` installs flat segment caches and initialized x87 and SIMD environments;
/// `filled` and `from_bytes` preserve literal backing images without initialization.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuState {
    pub flags: FlagsState,
    pub registers: Registers,
    pub eip: u32,
    pub segments: Segments,
    /// Architectural CR0 encoding. Snapshots preserve unsupported bits too.
    pub cr0: u32,
    /// Page-fault linear address. Stored without fault-delivery behavior.
    pub cr2: u32,
    /// Page-table base and control bits. Stored without address-translation behavior.
    pub cr3: u32,
    /// Architectural CR4 encoding; CPU capabilities remain separate.
    pub cr4: u32,
    /// Retired guest instructions, wrapping modulo 2^64; also the virtual TSC.
    pub instruction_count: u64,
    pub x87: X87State,
    pub simd: SimdState,
    pub gdtr: DescriptorTableRegister,
    pub idtr: DescriptorTableRegister,
    pub ldtr: SystemSegmentRegister,
    pub tr: SystemSegmentRegister,
}

impl CpuState {
    pub const BYTE_LEN: usize = size_of::<Self>();

    pub const fn mode(&self) -> CpuMode {
        CpuMode::from_control(self.cr0, self.flags.bytes.vm & 1 != 0)
    }

    pub const fn privilege_level(&self) -> PrivilegeLevel {
        self.mode().privilege_level(self.segments.cs.selector)
    }
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            segments: Segments::flat32(),
            // PE, ET and NE; x87 execution has no emulation or task-switch trap.
            cr0: 0x31,
            // OSFXSR and OSXMMEXCPT enable the represented SIMD environment.
            cr4: 0x600,
            x87: X87State::default(),
            simd: SimdState::default(),
            idtr: DescriptorTableRegister::REAL_MODE_IDTR,
            ..Self::filled(0)
        }
    }
}
