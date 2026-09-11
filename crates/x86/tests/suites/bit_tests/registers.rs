use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, prior_flags, Operation, OPERATIONS};

#[test]
fn register_destinations_mask_register_and_immediate_indexes_to_operand_width() {
    for bits in [16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let upper = if bits == 16 { 0x4433_0000 } else { 0 };
        for operation in OPERATIONS {
            for value in [0, mask, (1 << (bits - 1)) | 1, 0xa55a_5aa5 & mask] {
                for (immediate, indexes) in [
                    (true, &[0, 1, 15, 16, 31, 32, 63, 127, 128, 255][..]),
                    (
                        false,
                        &[
                            0,
                            1,
                            15,
                            16,
                            17,
                            31,
                            32,
                            33,
                            0x7fff,
                            0x8000,
                            0xffff,
                            0x4321_8000,
                            0x1234_7fff,
                            0xffff_0001,
                            0x8000_0000,
                            u32::MAX,
                        ][..],
                    ),
                ] {
                    for &index in indexes {
                        let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                        if immediate {
                            code.extend_from_slice(&[
                                0x0f,
                                0xba,
                                0xc0 | (operation.extension() << 3),
                                index as u8,
                            ]);
                        } else {
                            code.extend_from_slice(&[0x0f, operation.register_opcode(), 0xd0]);
                        }
                        let mut image = image(&code);
                        image.cpu.registers.eax = upper | value;
                        image.cpu.registers.edx = index;
                        let result = expected(operation, bits, value, index);
                        let mut cpu = image.cpu;
                        cpu.registers.eax = upper | result.value;
                        result.apply_flags(&mut cpu, prior_flags());
                        cpu.eip += code.len() as u32;
                        cpu.instruction_count = 0;
                        both(
                            TestModule::interpreter(),
                            &format!("{operation:?} {bits}-bit {value:x}, index {index:x}, immediate {immediate}"),
                            &code, 1, &image,
                            &[Step { cpu, ram: &[], exit: Exit::Dispatch(cpu.eip) }],
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn literal_results_publish_the_old_bit_even_when_the_write_has_no_effect() {
    for (operation, value, index, result, carry) in [
        (Operation::Bt, 0x8000_0001, 31, 0x8000_0001, 1),
        (Operation::Bt, 0x8000_0001, 30, 0x8000_0001, 0),
        (Operation::Bts, 1, 0, 1, 1),
        (Operation::Bts, 0, 31, 0x8000_0000, 0),
        (Operation::Btr, 0, 0, 0, 0),
        (Operation::Btr, u32::MAX, 31, 0x7fff_ffff, 1),
        (Operation::Btc, 0, 0, 1, 0),
        (Operation::Btc, 1, 32, 0, 1),
    ] {
        let code = [0x0f, 0xba, 0xc0 | (operation.extension() << 3), index];
        let mut image = image(&code);
        image.cpu.registers.eax = value;
        let mut cpu = image.cpu;
        cpu.registers.eax = result;
        cpu.flags.kind = 0;
        cpu.flags.status = wasm86_x86::StatusFlags {
            cf: carry,
            ..prior_flags()
        };
        cpu.eip += 4;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            &format!("literal {operation:?} {value:x}, index {index}"),
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn an_index_aliases_its_destination_before_any_partial_or_full_register_write() {
    for operation in OPERATIONS {
        for bits in [16, 32] {
            for (register, modrm, value) in [
                (Gpr32::Eax, 0xc0, 0x4433_8003),
                (Gpr32::Ecx, 0xc9, 0x8877_ffff),
                (Gpr32::Esp, 0xe4, 0x8765_0010),
            ] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.register_opcode(), modrm]);
                let mut image = image(&code);
                image.cpu.registers[register] = value;
                let result = expected(operation, bits, value, value);
                let mask = u32::MAX >> (32 - bits);
                let mut cpu = image.cpu;
                cpu.registers[register] = (value & !mask) | result.value;
                result.apply_flags(&mut cpu, prior_flags());
                cpu.eip += code.len() as u32;
                cpu.instruction_count = 0;
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit destination and index share {register:?}"),
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip),
                    }],
                );
            }
        }
    }
}
