use std::mem::offset_of;

use super::{CpuState, Registers, Segments, StoredFlags, StoredSimd, StoredX87};

impl CpuState {
    /// Reads every backing field without interpreting or normalizing its bits.
    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            flags: StoredFlags::from_bytes(read(&bytes, offset_of!(CpuState, flags))),
            registers: Registers {
                eax: read_u32(&bytes, offset_of!(CpuState, registers.eax)),
                ecx: read_u32(&bytes, offset_of!(CpuState, registers.ecx)),
                edx: read_u32(&bytes, offset_of!(CpuState, registers.edx)),
                ebx: read_u32(&bytes, offset_of!(CpuState, registers.ebx)),
                esp: read_u32(&bytes, offset_of!(CpuState, registers.esp)),
                ebp: read_u32(&bytes, offset_of!(CpuState, registers.ebp)),
                esi: read_u32(&bytes, offset_of!(CpuState, registers.esi)),
                edi: read_u32(&bytes, offset_of!(CpuState, registers.edi)),
            },
            eip: read_u32(&bytes, offset_of!(CpuState, eip)),
            segments: Segments::from_bytes(read(&bytes, offset_of!(CpuState, segments))),
            reserved: read(&bytes, offset_of!(CpuState, reserved)),
            instruction_count: read_u64(&bytes, offset_of!(CpuState, instruction_count)),
            x87: StoredX87::read(&bytes[offset_of!(CpuState, x87)..]),
            simd: StoredSimd::read(&bytes[offset_of!(CpuState, simd)..]),
        }
    }

    /// Writes every stored field, including reserved bytes, segment property
    /// bytes, and inactive flag data.
    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write(
            &mut bytes,
            offset_of!(CpuState, flags),
            &self.flags.to_bytes(),
        );
        for (offset, value) in [
            (offset_of!(CpuState, registers.eax), self.registers.eax),
            (offset_of!(CpuState, registers.ecx), self.registers.ecx),
            (offset_of!(CpuState, registers.edx), self.registers.edx),
            (offset_of!(CpuState, registers.ebx), self.registers.ebx),
            (offset_of!(CpuState, registers.esp), self.registers.esp),
            (offset_of!(CpuState, registers.ebp), self.registers.ebp),
            (offset_of!(CpuState, registers.esi), self.registers.esi),
            (offset_of!(CpuState, registers.edi), self.registers.edi),
            (offset_of!(CpuState, eip), self.eip),
        ] {
            write(&mut bytes, offset, &value.to_le_bytes());
        }
        write(
            &mut bytes,
            offset_of!(CpuState, segments),
            &self.segments.to_bytes(),
        );
        write(&mut bytes, offset_of!(CpuState, reserved), &self.reserved);
        write(
            &mut bytes,
            offset_of!(CpuState, instruction_count),
            &self.instruction_count.to_le_bytes(),
        );
        self.x87.write(&mut bytes[offset_of!(CpuState, x87)..]);
        self.simd.write(&mut bytes[offset_of!(CpuState, simd)..]);
        bytes
    }

    /// Creates a backing image whose bytes all have the supplied value.
    pub fn filled(byte: u8) -> Self {
        Self::from_bytes([byte; Self::BYTE_LEN])
    }
}

pub(super) fn read<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    bytes[offset..offset + N]
        .try_into()
        .expect("a CPU field must fit in its backing image")
}

pub(super) fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(read(bytes, offset))
}

pub(super) fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(read(bytes, offset))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(read(bytes, offset))
}

pub(super) fn write(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}
