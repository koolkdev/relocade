use std::mem::{offset_of, size_of};

use crate::{CpuState, StoredX87, StoredX87Control, StoredX87Register, StoredX87Status};

#[test]
fn x87_backing_appends_the_environment_and_eight_physical_slots() {
    assert_eq!(offset_of!(CpuState, x87), 152);
    assert_eq!(size_of::<StoredX87>(), 176);
    assert_eq!(size_of::<StoredX87Register>(), 16);
    assert_eq!(size_of::<StoredX87Status>(), 14);
    assert_eq!(size_of::<StoredX87Control>(), 12);
    assert_eq!(
        [
            offset_of!(StoredX87, control),
            offset_of!(StoredX87, tag_word),
            offset_of!(StoredX87, opcode),
            offset_of!(StoredX87, instruction_offset),
            offset_of!(StoredX87, data_offset),
            offset_of!(StoredX87, instruction_selector),
            offset_of!(StoredX87, data_selector),
            offset_of!(StoredX87, status),
            offset_of!(StoredX87, reserved),
            offset_of!(StoredX87, registers),
        ],
        [0, 12, 14, 16, 20, 24, 26, 28, 42, 48]
    );
    assert_eq!(
        [
            offset_of!(StoredX87Control, invalid_mask),
            offset_of!(StoredX87Control, denormal_mask),
            offset_of!(StoredX87Control, zero_divide_mask),
            offset_of!(StoredX87Control, overflow_mask),
            offset_of!(StoredX87Control, underflow_mask),
            offset_of!(StoredX87Control, precision_mask),
            offset_of!(StoredX87Control, precision_control),
            offset_of!(StoredX87Control, rounding_control),
            offset_of!(StoredX87Control, infinity_control),
            offset_of!(StoredX87Control, reserved),
            offset_of!(StoredX87Control, reserved_bits),
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    );
    assert_eq!(
        [
            offset_of!(StoredX87Status, invalid),
            offset_of!(StoredX87Status, denormal),
            offset_of!(StoredX87Status, zero_divide),
            offset_of!(StoredX87Status, overflow),
            offset_of!(StoredX87Status, underflow),
            offset_of!(StoredX87Status, precision),
            offset_of!(StoredX87Status, stack_fault),
            offset_of!(StoredX87Status, top),
            offset_of!(StoredX87Status, c0),
            offset_of!(StoredX87Status, c1),
            offset_of!(StoredX87Status, c2),
            offset_of!(StoredX87Status, c3),
            offset_of!(StoredX87Status, error_summary),
            offset_of!(StoredX87Status, busy),
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]
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
    let mut expected = [0; 176];
    expected[..12].copy_from_slice(&[1, 1, 1, 1, 1, 1, 3, 0, 0, 0, 0x40, 0]);
    expected[12..14].fill(0xff);
    assert_eq!(&cpu.to_bytes()[152..], &expected);
    assert_eq!(cpu.x87, StoredX87::default());

    let literal = CpuState::filled(0);
    assert_eq!(literal.x87.control.invalid_mask, 0);
    assert_eq!(literal.x87.control.reserved_bits, 0);
    assert_eq!(literal.x87.tag_word, 0);
    assert_eq!(literal.to_bytes(), [0; CpuState::BYTE_LEN]);
}

#[test]
fn decoding_preserves_raw_environment_register_encodings_and_padding() {
    let bytes = std::array::from_fn(|index| index as u8);
    let cpu = CpuState::from_bytes(bytes);
    assert_eq!(
        cpu.x87.control,
        StoredX87Control {
            invalid_mask: 0x98,
            denormal_mask: 0x99,
            zero_divide_mask: 0x9a,
            overflow_mask: 0x9b,
            underflow_mask: 0x9c,
            precision_mask: 0x9d,
            precision_control: 0x9e,
            rounding_control: 0x9f,
            infinity_control: 0xa0,
            reserved: 0xa1,
            reserved_bits: 0xa3a2,
        }
    );
    assert_eq!(cpu.x87.tag_word, 0xa5a4);
    assert_eq!(cpu.x87.opcode, 0xa7a6);
    assert_eq!(cpu.x87.instruction_offset, 0xabaa_a9a8);
    assert_eq!(cpu.x87.data_offset, 0xafae_adac);
    assert_eq!(cpu.x87.instruction_selector, 0xb1b0);
    assert_eq!(cpu.x87.data_selector, 0xb3b2);
    assert_eq!(
        cpu.x87.status,
        StoredX87Status {
            invalid: 0xb4,
            denormal: 0xb5,
            zero_divide: 0xb6,
            overflow: 0xb7,
            underflow: 0xb8,
            precision: 0xb9,
            stack_fault: 0xba,
            top: 0xbb,
            c0: 0xbc,
            c1: 0xbd,
            c2: 0xbe,
            c3: 0xbf,
            error_summary: 0xc0,
            busy: 0xc1,
        }
    );
    assert_eq!(cpu.x87.reserved, [0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7]);
    assert_eq!(
        cpu.x87.registers[0],
        StoredX87Register {
            significand: 0xcfce_cdcc_cbca_c9c8,
            sign_exponent: 0xd1d0,
            reserved: [0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7],
        }
    );
    assert_eq!(
        cpu.x87.registers[7],
        StoredX87Register {
            significand: 0x3f3e_3d3c_3b3a_3938,
            sign_exponent: 0x4140,
            reserved: [0x42, 0x43, 0x44, 0x45, 0x46, 0x47],
        }
    );
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_environment_fields_keeps_registers_and_padding_untouched() {
    let mut cpu = CpuState::filled(0xa5);
    cpu.x87.control = StoredX87Control {
        invalid_mask: 0x81,
        denormal_mask: 0x82,
        zero_divide_mask: 0x83,
        overflow_mask: 0x84,
        underflow_mask: 0x85,
        precision_mask: 0x86,
        precision_control: 0x87,
        rounding_control: 0x88,
        infinity_control: 0x89,
        reserved: 0xa5,
        reserved_bits: 0x1234,
    };
    cpu.x87.tag_word = 0x9abc;
    cpu.x87.opcode = 0xdef0;
    cpu.x87.instruction_offset = 0x1122_3344;
    cpu.x87.data_offset = 0x5566_7788;
    cpu.x87.instruction_selector = 0x99aa;
    cpu.x87.data_selector = 0xbbcc;
    cpu.x87.status = StoredX87Status {
        invalid: 0x81,
        denormal: 0x82,
        zero_divide: 0x83,
        overflow: 0x84,
        underflow: 0x85,
        precision: 0x86,
        stack_fault: 0x87,
        top: 0x88,
        c0: 0x89,
        c1: 0x8a,
        c2: 0x8b,
        c3: 0x8c,
        error_summary: 0x8d,
        busy: 0x8e,
    };
    let mut expected = [0xa5; CpuState::BYTE_LEN];
    expected[152..180].copy_from_slice(&[
        0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0xa5, 0x34, 0x12, 0xbc, 0x9a, 0xf0,
        0xde, 0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 0xaa, 0x99, 0xcc, 0xbb,
    ]);
    expected[180..194].copy_from_slice(&[
        0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e,
    ]);
    assert_eq!(cpu.to_bytes(), expected);
    assert_eq!(CpuState::from_bytes(expected), cpu);
}

#[test]
fn encoding_each_physical_register_preserves_its_noncanonical_bits() {
    for (index, offset) in [200, 216, 232, 248, 264, 280, 296, 312]
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
