use std::mem::offset_of;

use super::{CpuState, FlagBytes, Registers, StoredFlags, StoredStatusSource};

impl CpuState {
    /// Reads the backing bytes without interpreting or normalizing stored flags.
    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            flags: StoredFlags {
                status_source: StoredStatusSource {
                    kind: bytes[offset_of!(CpuState, flags.status_source.kind)],
                    reserved: read(&bytes, offset_of!(CpuState, flags.status_source.reserved)),
                    left: read_u32(&bytes, offset_of!(CpuState, flags.status_source.left)),
                    right: read_u32(&bytes, offset_of!(CpuState, flags.status_source.right)),
                },
                bytes: FlagBytes {
                    cf: bytes[offset_of!(CpuState, flags.bytes.cf)],
                    pf: bytes[offset_of!(CpuState, flags.bytes.pf)],
                    af: bytes[offset_of!(CpuState, flags.bytes.af)],
                    zf: bytes[offset_of!(CpuState, flags.bytes.zf)],
                    sf: bytes[offset_of!(CpuState, flags.bytes.sf)],
                    of: bytes[offset_of!(CpuState, flags.bytes.of)],
                    tf: bytes[offset_of!(CpuState, flags.bytes.tf)],
                    df: bytes[offset_of!(CpuState, flags.bytes.df)],
                    nt: bytes[offset_of!(CpuState, flags.bytes.nt)],
                    ac: bytes[offset_of!(CpuState, flags.bytes.ac)],
                    id: bytes[offset_of!(CpuState, flags.bytes.id)],
                    reserved: bytes[offset_of!(CpuState, flags.bytes.reserved)],
                },
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
        bytes[offset_of!(CpuState, flags.status_source.kind)] = self.flags.status_source.kind;
        write(
            &mut bytes,
            offset_of!(CpuState, flags.status_source.reserved),
            &self.flags.status_source.reserved,
        );
        write(
            &mut bytes,
            offset_of!(CpuState, flags.status_source.left),
            &self.flags.status_source.left.to_le_bytes(),
        );
        write(
            &mut bytes,
            offset_of!(CpuState, flags.status_source.right),
            &self.flags.status_source.right.to_le_bytes(),
        );
        bytes[offset_of!(CpuState, flags.bytes.cf)] = self.flags.bytes.cf;
        bytes[offset_of!(CpuState, flags.bytes.pf)] = self.flags.bytes.pf;
        bytes[offset_of!(CpuState, flags.bytes.af)] = self.flags.bytes.af;
        bytes[offset_of!(CpuState, flags.bytes.zf)] = self.flags.bytes.zf;
        bytes[offset_of!(CpuState, flags.bytes.sf)] = self.flags.bytes.sf;
        bytes[offset_of!(CpuState, flags.bytes.of)] = self.flags.bytes.of;
        bytes[offset_of!(CpuState, flags.bytes.tf)] = self.flags.bytes.tf;
        bytes[offset_of!(CpuState, flags.bytes.df)] = self.flags.bytes.df;
        bytes[offset_of!(CpuState, flags.bytes.nt)] = self.flags.bytes.nt;
        bytes[offset_of!(CpuState, flags.bytes.ac)] = self.flags.bytes.ac;
        bytes[offset_of!(CpuState, flags.bytes.id)] = self.flags.bytes.id;
        bytes[offset_of!(CpuState, flags.bytes.reserved)] = self.flags.bytes.reserved;
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
