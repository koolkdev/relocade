use std::mem::{offset_of, size_of};

use crate::{CpuState, StoredX87, StoredX87Register};

#[test]
fn x87_backing_appends_the_environment_and_eight_physical_slots() {
    assert_eq!(offset_of!(CpuState, x87), 152);
    assert_eq!(size_of::<StoredX87>(), 152);
    assert_eq!(size_of::<StoredX87Register>(), 16);
    assert_eq!(
        [
            offset_of!(StoredX87, control_word),
            offset_of!(StoredX87, status_word),
            offset_of!(StoredX87, tag_word),
            offset_of!(StoredX87, opcode),
            offset_of!(StoredX87, instruction_offset),
            offset_of!(StoredX87, data_offset),
            offset_of!(StoredX87, instruction_selector),
            offset_of!(StoredX87, data_selector),
            offset_of!(StoredX87, reserved),
            offset_of!(StoredX87, registers),
        ],
        [0, 2, 4, 6, 8, 12, 16, 18, 20, 24]
    );
    assert_eq!(
        [
            offset_of!(StoredX87Register, significand),
            offset_of!(StoredX87Register, sign_exponent),
            offset_of!(StoredX87Register, reserved),
        ],
        [0, 8, 10]
    );
}

#[test]
fn default_x87_is_initialized_but_literal_zero_backing_remains_zero() {
    let cpu = CpuState::default();
    let mut expected = [0; 152];
    expected[0..2].copy_from_slice(&[0x7f, 0x03]);
    expected[4..6].copy_from_slice(&[0xff, 0xff]);
    assert_eq!(&cpu.to_bytes()[152..], &expected);
    assert_eq!(cpu.x87, StoredX87::default());

    let literal = CpuState::filled(0);
    assert_eq!(literal.x87.control_word, 0);
    assert_eq!(literal.x87.tag_word, 0);
    assert_eq!(literal.to_bytes(), [0; CpuState::BYTE_LEN]);
}

#[test]
fn decoding_preserves_raw_environment_register_encodings_and_padding() {
    let bytes = std::array::from_fn(|index| index as u8);
    let cpu = CpuState::from_bytes(bytes);
    assert_eq!(cpu.x87.control_word, 0x9998);
    assert_eq!(cpu.x87.status_word, 0x9b9a);
    assert_eq!(cpu.x87.tag_word, 0x9d9c);
    assert_eq!(cpu.x87.opcode, 0x9f9e);
    assert_eq!(cpu.x87.instruction_offset, 0xa3a2_a1a0);
    assert_eq!(cpu.x87.data_offset, 0xa7a6_a5a4);
    assert_eq!(cpu.x87.instruction_selector, 0xa9a8);
    assert_eq!(cpu.x87.data_selector, 0xabaa);
    assert_eq!(cpu.x87.reserved, [0xac, 0xad, 0xae, 0xaf]);
    assert_eq!(
        cpu.x87.registers[0],
        StoredX87Register {
            significand: 0xb7b6_b5b4_b3b2_b1b0,
            sign_exponent: 0xb9b8,
            reserved: [0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf],
        }
    );
    assert_eq!(
        cpu.x87.registers[7],
        StoredX87Register {
            significand: 0x2726_2524_2322_2120,
            sign_exponent: 0x2928,
            reserved: [0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f],
        }
    );
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_environment_fields_keeps_registers_and_padding_untouched() {
    let mut cpu = CpuState::filled(0xa5);
    cpu.x87.control_word = 0x1234;
    cpu.x87.status_word = 0x5678;
    cpu.x87.tag_word = 0x9abc;
    cpu.x87.opcode = 0xdef0;
    cpu.x87.instruction_offset = 0x1122_3344;
    cpu.x87.data_offset = 0x5566_7788;
    cpu.x87.instruction_selector = 0x99aa;
    cpu.x87.data_selector = 0xbbcc;
    let mut expected = [0xa5; CpuState::BYTE_LEN];
    expected[152..172].copy_from_slice(&[
        0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a, 0xf0, 0xde, 0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66,
        0x55, 0xaa, 0x99, 0xcc, 0xbb,
    ]);
    assert_eq!(cpu.to_bytes(), expected);
    assert_eq!(CpuState::from_bytes(expected), cpu);
}

#[test]
fn encoding_each_physical_register_preserves_its_noncanonical_bits() {
    for (index, offset) in [176, 192, 208, 224, 240, 256, 272, 288]
        .into_iter()
        .enumerate()
    {
        let mut cpu = CpuState::filled(0xa5);
        cpu.x87.registers[index] = StoredX87Register {
            significand: 0x0123_4567_89ab_cdef,
            sign_exponent: 0xffff,
            reserved: [1, 2, 3, 4, 5, 6],
        };
        let mut expected = [0xa5; CpuState::BYTE_LEN];
        expected[offset..offset + 16].copy_from_slice(&[
            0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 0xff, 0xff, 1, 2, 3, 4, 5, 6,
        ]);
        assert_eq!(cpu.to_bytes(), expected);
        assert_eq!(CpuState::from_bytes(expected), cpu);
    }
}
