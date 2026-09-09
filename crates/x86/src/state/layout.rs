//! CPU backing fields shared by generated code and host state snapshots.

mod codec;

#[cfg(test)]
mod tests;

use std::{
    mem::size_of,
    ops::{Index, IndexMut},
};

use crate::register::Gpr32;

/// Stored status bytes, including noncanonical or stale values.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatusFlags {
    pub cf: u8,
    pub pf: u8,
    pub af: u8,
    pub zf: u8,
    pub sf: u8,
    pub of: u8,
}

/// The stored flag record retains bytes that its current kind does not use.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredFlags {
    pub kind: u8,
    pub reserved: [u8; 3],
    pub left: u32,
    pub right: u32,
    pub status: StatusFlags,
    pub non_status: [u8; 6],
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
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuState {
    pub flags: StoredFlags,
    pub registers: Registers,
    pub eip: u32,
    pub reserved: [u8; 84],
    pub instruction_count: u32,
    pub reserved_tail: [u8; 4],
}

impl CpuState {
    pub const BYTE_LEN: usize = size_of::<Self>();
}

impl Default for CpuState {
    fn default() -> Self {
        Self::filled(0)
    }
}
