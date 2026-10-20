use std::mem::{offset_of, size_of};

use crate::{CpuState, X87Control, X87Register, X87State, X87Status};

#[test]
fn x87_backing_appends_the_environment_and_eight_physical_slots() {
    assert_eq!(offset_of!(CpuState, x87), 176);
    assert_eq!(size_of::<X87State>(), 176);
    assert_eq!(size_of::<X87Register>(), 16);
    assert_eq!(size_of::<X87Status>(), 14);
    assert_eq!(size_of::<X87Control>(), 12);
    assert_eq!(
        [
            offset_of!(X87State, control),
            offset_of!(X87State, tag_word),
            offset_of!(X87State, opcode),
            offset_of!(X87State, instruction_offset),
            offset_of!(X87State, data_offset),
            offset_of!(X87State, instruction_selector),
            offset_of!(X87State, data_selector),
            offset_of!(X87State, status),
            offset_of!(X87State, reserved),
            offset_of!(X87State, registers),
        ],
        [0, 12, 14, 16, 20, 24, 26, 28, 42, 48]
    );
    assert_eq!(
        [
            offset_of!(X87Control, invalid_mask),
            offset_of!(X87Control, denormal_mask),
            offset_of!(X87Control, zero_divide_mask),
            offset_of!(X87Control, overflow_mask),
            offset_of!(X87Control, underflow_mask),
            offset_of!(X87Control, precision_mask),
            offset_of!(X87Control, precision_control),
            offset_of!(X87Control, rounding_control),
            offset_of!(X87Control, infinity_control),
            offset_of!(X87Control, reserved),
            offset_of!(X87Control, reserved_bits),
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    );
    assert_eq!(
        [
            offset_of!(X87Status, invalid),
            offset_of!(X87Status, denormal),
            offset_of!(X87Status, zero_divide),
            offset_of!(X87Status, overflow),
            offset_of!(X87Status, underflow),
            offset_of!(X87Status, precision),
            offset_of!(X87Status, stack_fault),
            offset_of!(X87Status, top),
            offset_of!(X87Status, c0),
            offset_of!(X87Status, c1),
            offset_of!(X87Status, c2),
            offset_of!(X87Status, c3),
            offset_of!(X87Status, error_summary),
            offset_of!(X87Status, busy),
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]
    );
    assert_eq!(
        [
            offset_of!(X87Register, significand),
            offset_of!(X87Register, sign_exponent),
            offset_of!(X87Register, reserved),
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
    assert_eq!(&cpu.to_bytes()[176..352], &expected);
    assert_eq!(cpu.x87, X87State::default());

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
        X87Control {
            invalid_mask: 0xb0,
            denormal_mask: 0xb1,
            zero_divide_mask: 0xb2,
            overflow_mask: 0xb3,
            underflow_mask: 0xb4,
            precision_mask: 0xb5,
            precision_control: 0xb6,
            rounding_control: 0xb7,
            infinity_control: 0xb8,
            reserved: 0xb9,
            reserved_bits: 0xbbba,
        }
    );
    assert_eq!(cpu.x87.tag_word, 0xbdbc);
    assert_eq!(cpu.x87.opcode, 0xbfbe);
    assert_eq!(cpu.x87.instruction_offset, 0xc3c2c1c0);
    assert_eq!(cpu.x87.data_offset, 0xc7c6c5c4);
    assert_eq!(cpu.x87.instruction_selector, 0xc9c8);
    assert_eq!(cpu.x87.data_selector, 0xcbca);
    assert_eq!(
        cpu.x87.status,
        X87Status {
            invalid: 0xcc,
            denormal: 0xcd,
            zero_divide: 0xce,
            overflow: 0xcf,
            underflow: 0xd0,
            precision: 0xd1,
            stack_fault: 0xd2,
            top: 0xd3,
            c0: 0xd4,
            c1: 0xd5,
            c2: 0xd6,
            c3: 0xd7,
            error_summary: 0xd8,
            busy: 0xd9,
        }
    );
    assert_eq!(cpu.x87.reserved, [0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf]);
    assert_eq!(
        cpu.x87.registers[0],
        X87Register {
            significand: 0xe7e6e5e4e3e2e1e0,
            sign_exponent: 0xe9e8,
            reserved: [0xea, 0xeb, 0xec, 0xed, 0xee, 0xef],
        }
    );
    assert_eq!(
        cpu.x87.registers[7],
        X87Register {
            significand: 0x5756555453525150,
            sign_exponent: 0x5958,
            reserved: [0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f],
        }
    );
    assert_eq!(cpu.to_bytes(), bytes);
}

#[test]
fn encoding_environment_fields_keeps_registers_and_padding_untouched() {
    let mut cpu = CpuState::filled(0xa5);
    cpu.x87.control = X87Control {
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
    cpu.x87.status = X87Status {
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
    expected[176..204].copy_from_slice(&[
        0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0xa5, 0x34, 0x12, 0xbc, 0x9a, 0xf0,
        0xde, 0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 0xaa, 0x99, 0xcc, 0xbb,
    ]);
    expected[204..218].copy_from_slice(&[
        0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e,
    ]);
    assert_eq!(cpu.to_bytes(), expected);
    assert_eq!(CpuState::from_bytes(expected), cpu);
}

#[test]
fn encoding_each_physical_register_preserves_its_noncanonical_bits() {
    for (index, offset) in [224, 240, 256, 272, 288, 304, 320, 336]
        .into_iter()
        .enumerate()
    {
        let mut cpu = CpuState::filled(0xa5);
        cpu.x87.registers[index] = X87Register {
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
