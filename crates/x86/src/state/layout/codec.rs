use std::mem::offset_of;

use super::{CpuState, Registers, StatusFlags, StoredFlags};

impl CpuState {
    /// Reads the backing bytes without interpreting or normalizing stored flags.
    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            flags: StoredFlags {
                kind: bytes[offset_of!(CpuState, flags.kind)],
                reserved: read(&bytes, offset_of!(CpuState, flags.reserved)),
                left: read_u32(&bytes, offset_of!(CpuState, flags.left)),
                right: read_u32(&bytes, offset_of!(CpuState, flags.right)),
                status: StatusFlags {
                    cf: bytes[offset_of!(CpuState, flags.status.cf)],
                    pf: bytes[offset_of!(CpuState, flags.status.pf)],
                    af: bytes[offset_of!(CpuState, flags.status.af)],
                    zf: bytes[offset_of!(CpuState, flags.status.zf)],
                    sf: bytes[offset_of!(CpuState, flags.status.sf)],
                    of: bytes[offset_of!(CpuState, flags.status.of)],
                },
                non_status: read(&bytes, offset_of!(CpuState, flags.non_status)),
            },
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
            reserved: read(&bytes, offset_of!(CpuState, reserved)),
            instruction_count: read_u32(&bytes, offset_of!(CpuState, instruction_count)),
            reserved_tail: read(&bytes, offset_of!(CpuState, reserved_tail)),
        }
    }

    /// Writes every stored field, including reserved bytes and inactive flag data.
    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        bytes[offset_of!(CpuState, flags.kind)] = self.flags.kind;
        write(
            &mut bytes,
            offset_of!(CpuState, flags.reserved),
            &self.flags.reserved,
        );
        write(
            &mut bytes,
            offset_of!(CpuState, flags.left),
            &self.flags.left.to_le_bytes(),
        );
        write(
            &mut bytes,
            offset_of!(CpuState, flags.right),
            &self.flags.right.to_le_bytes(),
        );
        bytes[offset_of!(CpuState, flags.status.cf)] = self.flags.status.cf;
        bytes[offset_of!(CpuState, flags.status.pf)] = self.flags.status.pf;
        bytes[offset_of!(CpuState, flags.status.af)] = self.flags.status.af;
        bytes[offset_of!(CpuState, flags.status.zf)] = self.flags.status.zf;
        bytes[offset_of!(CpuState, flags.status.sf)] = self.flags.status.sf;
        bytes[offset_of!(CpuState, flags.status.of)] = self.flags.status.of;
        write(
            &mut bytes,
            offset_of!(CpuState, flags.non_status),
            &self.flags.non_status,
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
            (
                offset_of!(CpuState, instruction_count),
                self.instruction_count,
            ),
        ] {
            write(&mut bytes, offset, &value.to_le_bytes());
        }
        write(&mut bytes, offset_of!(CpuState, reserved), &self.reserved);
        write(
            &mut bytes,
            offset_of!(CpuState, reserved_tail),
            &self.reserved_tail,
        );
        bytes
    }

    /// Creates a backing image whose bytes all have the supplied value.
    pub fn filled(byte: u8) -> Self {
        Self::from_bytes([byte; Self::BYTE_LEN])
    }
}

fn read<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    bytes[offset..offset + N]
        .try_into()
        .expect("a CPU field must fit in its backing image")
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(read(bytes, offset))
}

fn write(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}
