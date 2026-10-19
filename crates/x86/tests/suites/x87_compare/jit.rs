//! Native ordering retains exact values; operand exceptions restart before effects.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use relocade_x86::compile_block_from_bytes;

fn loaded(mut cpu: CpuState, address: u32, value: (u64, u16), opcode: u16) -> CpuState {
    cpu = complete_x87(cpu, 6, opcode);
    cpu.x87.status.top = cpu.x87.status.top.wrapping_sub(1) & 7;
    cpu.x87.status.c1 = 0;
    cpu.x87.data_offset = address;
    cpu.x87.data_selector = 0x23;
    let top = cpu.x87.status.top;
    write_value(&mut cpu, usize::from(top), value);
    cpu
}

fn native_values() {
    // Each native encoding has an independently specified extended snapshot.
    let values = [
        (0x3ff0_0000_0000_0000_u64, ONE),
        (0x3ff0_0000_0000_0001, (LEADING + 0x800, 0x3fff)),
        (0xbff0_0000_0000_0000, (LEADING, 0xbfff)),
        (0xbff0_0000_0000_0001, (LEADING + 0x800, 0xbfff)),
        (0, (0, 0)),
        (0x8000_0000_0000_0000, (0, 0x8000)),
        (0x0010_0000_0000_0000, (LEADING, 0x3c01)),
        (0x7fef_ffff_ffff_ffff, (0xffff_ffff_ffff_f800, 0x43fe)),
        (0x7ff0_0000_0000_0000, (LEADING, 0x7fff)),
        (0xfff0_0000_0000_0000, (LEADING, 0xffff)),
        (0x7ff8_0000_0000_0001, (0xc000_0000_0000_0800, 0x7fff)),
    ];
    for memory in [true, false] {
        let code: &[u8] = if memory {
            // FLD m64; FCOMP m64
            &[0xdd, 0x05, 0, 0x40, 0, 0, 0xdc, 0x1d, 8, 0x40, 0, 0]
        } else {
            // Load the right operand first, then the left; FUCOMPP.
            &[
                0xdd, 0x05, 8, 0x40, 0, 0, 0xdd, 0x05, 0, 0x40, 0, 0, 0xda, 0xe9,
            ]
        };
        let count = if memory { 2 } else { 3 };
        let block = TestModule::new(&compile_block_from_bytes(0x1000, code, count).unwrap());
        for (left, right, flags) in [
            (0, 1, LESS),
            (1, 0, 0),
            (0, 0, EQUAL),
            (2, 3, 0),
            (3, 2, LESS),
            (4, 5, EQUAL),
            (5, 4, EQUAL),
            (6, 7, LESS),
            (7, 8, LESS),
            (9, 2, LESS),
            (8, 8, EQUAL),
            (10, 0, UNORDERED),
            (0, 10, UNORDERED),
        ] {
            if memory && flags == UNORDERED {
                // Ordered NaNs take the operand-exception restart tested below.
                continue;
            }
            let mut image = stack_image(code, 0, 0xffff);
            image.cpu.x87.status = status(0x20);
            // Comparing does not require a precision, rounding or mask assumption.
            set_control(&mut image.cpu.x87.control, 0x0c40);
            image.map(4, 0x8000, false);
            image.data(0x8000, &values[left].0.to_le_bytes());
            image.data(0x8008, &values[right].0.to_le_bytes());
            let mut steps = Vec::new();
            let mut cpu = image.cpu;
            if !memory {
                cpu = loaded(cpu, 0x4008, values[right].1, 0x0505);
                steps.push(dispatch(cpu));
            }
            cpu = loaded(cpu, 0x4000, values[left].1, 0x0505);
            steps.push(dispatch(cpu));
            cpu = completed(
                cpu,
                if memory { 6 } else { 2 },
                if memory { 0x041d } else { 0x02e9 },
                flags,
                if memory { 1 } else { 2 },
            );
            if memory {
                cpu.x87.data_offset = 0x4008;
            }
            steps.push(dispatch(cpu));
            assert_eq!(
                block.observe(&image.input(), 1),
                expected(&image, &[dispatch(cpu)]),
                "native comparison stays compiled: memory={memory}, {left} versus {right}"
            );
            assert_eq!(
                TestModule::interpreter().observe(&image.input(), count as usize),
                expected(&image, &steps)
            );
        }
    }
}

#[test]
fn operand_exception_restart() {
    for (compare, quiet_nan, signaling_nan) in [
        (
            0xdc_u8,
            0x7ff8_0000_0000_0001_u64,
            0x7ff0_0000_0000_0001_u64,
        ),
        (0xd8, 0x7fc0_0001, 0x7f80_0001),
    ] {
        let opcode = (u16::from(compare & 7) << 8) | 0x1d;
        let code = [0xdd, 0x05, 0, 0x40, 0, 0, compare, 0x1d, 8, 0x40, 0, 0];
        let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
        let block = TestModule::new(&compiled);
        let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
        for (source, exception, relation) in [
            (quiet_nan, 1, UNORDERED),
            (signaling_nan, 1, UNORDERED),
            (1, 2, 0),
        ] {
            for masked in [false, true] {
                let mut image = stack_image(&code, 0, 0xffff);
                image.cpu.x87.status = status(0x4320);
                set_control(
                    &mut image.cpu.x87.control,
                    if masked { 0x037f } else { 0x037f & !exception },
                );
                image.map(4, 0x8000, false);
                image.data(0x8000, &1.0_f64.to_bits().to_le_bytes());
                image.data(0x8008, &source.to_le_bytes());
                let first = loaded(image.cpu, 0x4000, ONE, 0x0505);
                assert_eq!(
                    block.observe(&image.input(), 1),
                    expected(
                        &image,
                        &[Step {
                            cpu: first,
                            ram: &[],
                            exit: Exit::Interpret,
                        }]
                    ),
                    "restart retains the preceding load and its saved pointers"
                );
                let flags = exception | if masked { relation } else { 0x4100 | PENDING };
                let mut result = completed(first, 6, opcode, flags, u8::from(masked));
                result.x87.data_offset = 0x4008;
                assert_eq!(
                    linked.observe(&image.input(), 1),
                    expected(&image, &[dispatch(result)]),
                    "interpreter completes the excluded operand response"
                );
            }
        }
    }
}

fn wide_values_stay_compiled() {
    for (code, left, right, flags) in [
        ([0xd8, 0xd1], (LEADING + 1, 0x3fff), ONE, 0),
        ([0xd8, 0xd1], (LEADING, 1), (LEADING, 0x7ffe), LESS),
        ([0xdd, 0xe1], QNAN, (1, 0), UNORDERED),
        ([0xdd, 0xe1], (LEADING, 0x7fff), ONE, 0),
    ] {
        let mut image = initial_image(&code);
        set_control(&mut image.cpu.x87.control, 0x0c40);
        write_value(&mut image.cpu, 7, left);
        write_value(&mut image.cpu, 0, right);
        let opcode = (u16::from(code[0] & 7) << 8) | u16::from(code[1]);
        let result = completed(image.cpu, 2, opcode, flags, 0);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        assert_eq!(
            block.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

#[test]
fn native_and_extended_ordering() {
    native_values();
    wide_values_stay_compiled();
}

#[test]
fn binary32_and_integer_comparisons_stay_compiled() {
    // Source bits, the independently specified loaded value, and the final
    // relation cover exact promotion, integer limits, signed zero and wide FILD.
    for (load, modrm, source, value, compare, right, flags) in [
        (
            0xd9,
            0x05,
            0x3f80_0000_u64,
            ONE,
            0xd8,
            0x3f80_0001_u64,
            LESS,
        ),
        (
            0xd9,
            0x05,
            0x3f80_0001,
            (LEADING + (1 << 40), 0x3fff),
            0xdc,
            0x3ff0_0000_0000_0000,
            0,
        ),
        (
            0xdd,
            0x05,
            0x3ff0_0000_0000_0001,
            (LEADING + 0x800, 0x3fff),
            0xd8,
            0x3f80_0000,
            0,
        ),
        (0xd9, 0x05, 0x8000_0000, (0, 0x8000), 0xdc, 0, EQUAL),
        (0xd9, 0x05, 0x8080_0000, (LEADING, 0xbf81), 0xd9, 0, LESS),
        (
            0xdd,
            0x05,
            0x8000_0000_0000_0000,
            (0, 0x8000),
            0xd9,
            0,
            EQUAL,
        ),
        (
            0xdd,
            0x05,
            0x41df_ffff_ffe0_0000,
            (0xffff_ffff_0000_0000, 0x401d),
            0xda,
            0x7fff_ffff,
            0,
        ),
        (
            0xd9,
            0x05,
            0xc700_0000,
            (LEADING, 0xc00e),
            0xde,
            0x8000,
            EQUAL,
        ),
        (
            0xdb,
            0x05,
            0x8000_0000,
            (LEADING, 0xc01e),
            0xda,
            0x7fff_ffff,
            LESS,
        ),
        (0xdf, 0x05, 0x8000, (LEADING, 0xc00e), 0xd9, 0, LESS),
        (
            0xdf,
            0x2d,
            0x0020_0000_0000_0001,
            (LEADING + 0x400, 0x4034),
            0xdc,
            0x4340_0000_0000_0000,
            0,
        ),
    ] {
        let mut code = vec![load, modrm, 0, 0x40, 0, 0];
        let test_zero = compare == 0xd9;
        let tail: &[u8] = if test_zero {
            &[0xd9, 0xe4]
        } else {
            &[compare, 0x1d, 8, 0x40, 0, 0]
        };
        code.extend_from_slice(tail);
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status = status(0x20);
        set_control(&mut image.cpu.x87.control, 0x0c40);
        image.map(4, 0x8000, false);
        image.data(0x8000, &source.to_le_bytes());
        image.data(0x8008, &right.to_le_bytes());
        let first = loaded(
            image.cpu,
            0x4000,
            value,
            (u16::from(load & 7) << 8) | u16::from(modrm),
        );
        let mut result = completed(
            first,
            tail.len() as u32,
            (u16::from(compare & 7) << 8) | u16::from(tail[1]),
            flags,
            u8::from(!test_zero),
        );
        if !test_zero {
            result.x87.data_offset = 0x4008;
        }
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
        assert_eq!(
            block.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)]),
            "compiled {code:02x?}"
        );
        assert_eq!(
            TestModule::interpreter().observe(&image.input(), 2),
            expected(&image, &[dispatch(first), dispatch(result)]),
            "interpreted {code:02x?}"
        );
    }
}

#[test]
fn sign_changes_update_native_integer_comparisons() {
    for (operation, input, before, after, flags) in [
        (0xe0_u8, 1_i32, ONE, (LEADING, 0xbfff), LESS),
        (0xe1, -1, (LEADING, 0xbfff), ONE, 0),
        (0xe0, 0, (0, 0), (0, 0x8000), EQUAL),
    ] {
        let code = [0xdb, 0x05, 0, 0x40, 0, 0, 0xd9, operation, 0xd9, 0xe4];
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status = status(0x20);
        image.map(4, 0x8000, false);
        image.data(0x8000, &input.to_le_bytes());
        let first = loaded(image.cpu, 0x4000, before, 0x0305);
        let mut changed = complete_x87(first, 2, 0x0100 | u16::from(operation));
        let top = changed.x87.status.top;
        write_value(&mut changed, usize::from(top), after);
        let result = completed(changed, 2, 0x01e4, flags, 0);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
        assert_eq!(
            block.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
        assert_eq!(
            TestModule::interpreter().observe(&image.input(), 3),
            expected(
                &image,
                &[dispatch(first), dispatch(changed), dispatch(result)]
            )
        );
    }
}
