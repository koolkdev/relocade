mod segments;

use std::mem::{offset_of, size_of};

use super::{CpuState, FlagBytes, Registers, StoredFlags, StoredStatusSource};
use crate::Gpr32;

#[test]
fn cpu_layout_matches_the_external_byte_contract() {
    assert_eq!(CpuState::BYTE_LEN, 152);
    assert_eq!(size_of::<StoredFlags>(), 24);
    assert_eq!(size_of::<StoredStatusSource>(), 12);
    assert_eq!(size_of::<FlagBytes>(), 12);
    assert_eq!(offset_of!(StoredFlags, status_source), 0);
    assert_eq!(offset_of!(StoredFlags, bytes), 12);
    assert_eq!(size_of::<Registers>(), 32);
    assert_eq!(
        [
            offset_of!(CpuState, flags.status_source.kind),
            offset_of!(CpuState, flags.status_source.reserved),
            offset_of!(CpuState, flags.status_source.left),
            offset_of!(CpuState, flags.status_source.right),
            offset_of!(CpuState, flags.bytes.cf),
            offset_of!(CpuState, flags.bytes.pf),
            offset_of!(CpuState, flags.bytes.af),
            offset_of!(CpuState, flags.bytes.zf),
            offset_of!(CpuState, flags.bytes.sf),
            offset_of!(CpuState, flags.bytes.of),
            offset_of!(CpuState, flags.bytes.tf),
            offset_of!(CpuState, flags.bytes.df),
            offset_of!(CpuState, flags.bytes.nt),
            offset_of!(CpuState, flags.bytes.ac),
            offset_of!(CpuState, flags.bytes.id),
            offset_of!(CpuState, flags.bytes.reserved),
            offset_of!(CpuState, registers.eax),
            offset_of!(CpuState, registers.ecx),
            offset_of!(CpuState, registers.edx),
            offset_of!(CpuState, registers.ebx),
            offset_of!(CpuState, registers.esp),
            offset_of!(CpuState, registers.ebp),
            offset_of!(CpuState, registers.esi),
            offset_of!(CpuState, registers.edi),
            offset_of!(CpuState, eip),
            offset_of!(CpuState, reserved),
            offset_of!(CpuState, instruction_count),
            offset_of!(CpuState, reserved_tail),
        ],
        [
            0, 1, 4, 8, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 28, 32, 36, 40, 44, 48,
            52, 56, 132, 144, 148
        ],
    );
}

#[test]
fn decoding_preserves_little_endian_values_and_every_reserved_byte() {
    let bytes = std::array::from_fn(|index| index as u8);
    let cpu = CpuState::from_bytes(bytes);
    assert_eq!(cpu.flags.status_source.kind, 0);
    assert_eq!(cpu.flags.status_source.reserved, [1, 2, 3]);
    assert_eq!(cpu.flags.status_source.left, 0x0706_0504);
    assert_eq!(cpu.flags.status_source.right, 0x0b0a_0908);
    assert_eq!(
        [
            cpu.flags.bytes.cf,
            cpu.flags.bytes.pf,
            cpu.flags.bytes.af,
            cpu.flags.bytes.zf,
            cpu.flags.bytes.sf,
            cpu.flags.bytes.of
        ],
        [12, 13, 14, 15, 16, 17]
    );
    assert_eq!(
        [
            cpu.flags.bytes.tf,
            cpu.flags.bytes.df,
            cpu.flags.bytes.nt,
            cpu.flags.bytes.ac,
            cpu.flags.bytes.id,
            cpu.flags.bytes.reserved
        ],
        [18, 19, 20, 21, 22, 23]
    );
    assert_eq!(
        cpu.registers,
        Registers {
            eax: 0x1b1a_1918,
            ecx: 0x1f1e_1d1c,
            edx: 0x2322_2120,
            ebx: 0x2726_2524,
            esp: 0x2b2a_2928,
            ebp: 0x2f2e_2d2c,
            esi: 0x3332_3130,
            edi: 0x3736_3534,
        }
    );
    assert_eq!(cpu.eip, 0x3b3a_3938);
    assert_eq!(
        cpu.reserved,
        std::array::from_fn(|index| (132 + index) as u8)
    );
    assert_eq!(cpu.instruction_count, 0x9392_9190);
    assert_eq!(cpu.reserved_tail, [148, 149, 150, 151]);
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_changes_only_the_named_fields_including_noncanonical_flags() {
    let mut cpu = CpuState::filled(0xa5);
    cpu.registers.ebx = 0x9234_5678;
    cpu.eip = 0x1002;
    cpu.instruction_count = 0;
    cpu.flags.status_source.kind = 0xff;
    cpu.flags.bytes.cf = 0x80;
    cpu.flags.bytes.df = 0xfe;
    let mut expected = [0xa5; 152];
    expected[0] = 0xff;
    expected[12] = 0x80;
    expected[19] = 0xfe;
    expected[36..40].copy_from_slice(&[0x78, 0x56, 0x34, 0x92]);
    expected[56..60].copy_from_slice(&[2, 0x10, 0, 0]);
    expected[144..148].fill(0);
    assert_eq!(cpu.to_bytes(), expected);
    assert_eq!(CpuState::from_bytes(expected), cpu);
    assert_eq!(CpuState::filled(0).to_bytes(), [0; 152]);
}

#[test]
fn encoded_register_indices_address_the_named_registers() {
    let mut registers = Registers::default();
    for (index, register) in Gpr32::ALL.into_iter().enumerate() {
        assert_eq!(register as usize, index);
        assert_eq!(Gpr32::from_code(index as u8), register);
        assert_eq!(Gpr32::from_code(index as u8 + 8), register);
        registers[register] = 0x1111_1111 * (index as u32 + 1);
    }
    assert_eq!(
        registers,
        Registers {
            eax: 0x1111_1111,
            ecx: 0x2222_2222,
            edx: 0x3333_3333,
            ebx: 0x4444_4444,
            esp: 0x5555_5555,
            ebp: 0x6666_6666,
            esi: 0x7777_7777,
            edi: 0x8888_8888,
        }
    );
    assert_eq!(
        Gpr32::ALL.map(|register| registers[register]),
        [
            0x1111_1111,
            0x2222_2222,
            0x3333_3333,
            0x4444_4444,
            0x5555_5555,
            0x6666_6666,
            0x7777_7777,
            0x8888_8888,
        ]
    );
}
