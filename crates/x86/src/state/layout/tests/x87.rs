use std::mem::{offset_of, size_of};

use crate::{CpuState, StoredX87, StoredX87Register, StoredX87Status};

#[test]
fn x87_backing_appends_the_environment_and_eight_physical_slots() {
    assert_eq!(offset_of!(CpuState, x87), 152);
    assert_eq!(size_of::<StoredX87>(), 160);
    assert_eq!(size_of::<StoredX87Register>(), 16);
    assert_eq!(size_of::<StoredX87Status>(), 8);
    assert_eq!(
        [
            offset_of!(StoredX87, control_word),
            offset_of!(StoredX87, tag_word),
            offset_of!(StoredX87, opcode),
            offset_of!(StoredX87, reserved_control),
            offset_of!(StoredX87, instruction_offset),
            offset_of!(StoredX87, data_offset),
            offset_of!(StoredX87, instruction_selector),
            offset_of!(StoredX87, data_selector),
            offset_of!(StoredX87, status),
            offset_of!(StoredX87, reserved),
            offset_of!(StoredX87, registers),
        ],
        [0, 2, 4, 6, 8, 12, 16, 18, 20, 28, 32]
    );
    assert_eq!(
        [
            offset_of!(StoredX87Status, exception_flags),
            offset_of!(StoredX87Status, top),
            offset_of!(StoredX87Status, c0),
            offset_of!(StoredX87Status, c1),
            offset_of!(StoredX87Status, c2),
            offset_of!(StoredX87Status, c3),
            offset_of!(StoredX87Status, error_summary),
            offset_of!(StoredX87Status, busy),
        ],
        [0, 1, 2, 3, 4, 5, 6, 7]
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
    let mut expected = [0; 160];
    expected[0..2].copy_from_slice(&[0x7f, 0x03]);
    expected[2..4].copy_from_slice(&[0xff, 0xff]);
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
    assert_eq!(cpu.x87.tag_word, 0x9b9a);
    assert_eq!(cpu.x87.opcode, 0x9d9c);
    assert_eq!(cpu.x87.reserved_control, [0x9e, 0x9f]);
    assert_eq!(cpu.x87.instruction_offset, 0xa3a2_a1a0);
    assert_eq!(cpu.x87.data_offset, 0xa7a6_a5a4);
    assert_eq!(cpu.x87.instruction_selector, 0xa9a8);
    assert_eq!(cpu.x87.data_selector, 0xabaa);
    assert_eq!(
        cpu.x87.status,
        StoredX87Status {
            exception_flags: 0xac,
            top: 0xad,
            c0: 0xae,
            c1: 0xaf,
            c2: 0xb0,
            c3: 0xb1,
            error_summary: 0xb2,
            busy: 0xb3,
        }
    );
    assert_eq!(cpu.x87.reserved, [0xb4, 0xb5, 0xb6, 0xb7]);
    assert_eq!(
        cpu.x87.registers[0],
        StoredX87Register {
            significand: 0xbfbe_bdbc_bbba_b9b8,
            sign_exponent: 0xc1c0,
            reserved: [0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7],
        }
    );
    assert_eq!(
        cpu.x87.registers[7],
        StoredX87Register {
            significand: 0x2f2e_2d2c_2b2a_2928,
            sign_exponent: 0x3130,
            reserved: [0x32, 0x33, 0x34, 0x35, 0x36, 0x37],
        }
    );
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_environment_fields_keeps_registers_and_padding_untouched() {
    let mut cpu = CpuState::filled(0xa5);
    cpu.x87.control_word = 0x1234;
    cpu.x87.tag_word = 0x9abc;
    cpu.x87.opcode = 0xdef0;
    cpu.x87.instruction_offset = 0x1122_3344;
    cpu.x87.data_offset = 0x5566_7788;
    cpu.x87.instruction_selector = 0x99aa;
    cpu.x87.data_selector = 0xbbcc;
    cpu.x87.status = StoredX87Status {
        exception_flags: 0x81,
        top: 0x82,
        c0: 0x83,
        c1: 0x84,
        c2: 0x85,
        c3: 0x86,
        error_summary: 0x87,
        busy: 0x88,
    };
    let mut expected = [0xa5; CpuState::BYTE_LEN];
    expected[152..172].copy_from_slice(&[
        0x34, 0x12, 0xbc, 0x9a, 0xf0, 0xde, 0xa5, 0xa5, 0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66,
        0x55, 0xaa, 0x99, 0xcc, 0xbb,
    ]);
    expected[172..180].copy_from_slice(&[0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88]);
    assert_eq!(cpu.to_bytes(), expected);
    assert_eq!(CpuState::from_bytes(expected), cpu);
}

#[test]
fn encoding_each_physical_register_preserves_its_noncanonical_bits() {
    for (index, offset) in [184, 200, 216, 232, 248, 264, 280, 296]
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
