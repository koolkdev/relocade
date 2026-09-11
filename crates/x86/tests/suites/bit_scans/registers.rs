use wasm86_x86::Gpr32;

use crate::support::{machine::both, step::TestModule};

use super::{expected, image, retire, OPERATIONS};

#[test]
fn scans_find_every_bit_position_and_ignore_source_bits_above_the_operand_width() {
    for bits in [16, 32] {
        let mut sources = vec![
            0,
            u32::MAX,
            0xffff_0000,
            0x8000_0000,
            0x8001_0000,
            0x8000_8000,
            0x8000_8001,
            0x8000_2408,
        ];
        let upper = if bits == 16 { 0xa55a_0000 } else { 0 };
        sources.extend((0..bits).map(|bit| upper | (1 << bit)));
        for operation in OPERATIONS {
            for &source in &sources {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.opcode(), 0xc2]);
                let mut image = image(&code);
                image.cpu.registers.edx = source;
                let mut cpu = image.cpu;
                expected(operation, bits, source, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
                let step = retire(&mut cpu, code.len() as u32);
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit source {source:08x}"),
                    &code,
                    1,
                    &image,
                    &[step],
                );
            }
        }
    }
}

#[test]
fn scan_parity_counts_the_full_source_operand_before_writing_the_index() {
    for (bits, source, parity) in [
        (16, 0, 1),
        (32, 0, 1),
        (16, 0x100, 0),
        (32, 0x100, 0),
        (16, 0x101, 1),
        (32, 0x101, 1),
        (16, 0x1_0000, 1),
        (32, 0x1_0000, 0),
        (16, 0x1_0001, 0),
        (32, 0x1_0001, 1),
        (16, 0x1_0100, 0),
        (32, 0x1_0100, 1),
    ] {
        for operation in OPERATIONS {
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, operation.opcode(), 0xc2]);
            let mut image = image(&code);
            image.cpu.registers.edx = source;
            let mut cpu = image.cpu;
            expected(operation, bits, source, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
            cpu.flags.status.pf = parity;
            let step = retire(&mut cpu, code.len() as u32);
            both(
                TestModule::interpreter(),
                &format!("{operation:?} {bits}-bit full source parity for {source:08x}"),
                &code,
                1,
                &image,
                &[step],
            );
        }
    }
}

#[test]
fn every_destination_register_reads_distinct_and_self_sources_before_writing() {
    let registers = [
        Gpr32::Eax,
        Gpr32::Ecx,
        Gpr32::Edx,
        Gpr32::Ebx,
        Gpr32::Esp,
        Gpr32::Ebp,
        Gpr32::Esi,
        Gpr32::Edi,
    ];
    for bits in [16, 32] {
        for operation in OPERATIONS {
            for (destination_code, &destination) in registers.iter().enumerate() {
                for source_code in [destination_code, (destination_code + 3) % registers.len()] {
                    for value in [0, 0xffff_0000, 0x8001_0080] {
                        let source = registers[source_code];
                        let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                        code.extend_from_slice(&[
                            0x0f,
                            operation.opcode(),
                            0xc0 | ((destination_code as u8) << 3) | source_code as u8,
                        ]);
                        let mut image = image(&code);
                        image.cpu.registers[destination] = 0x4433_a55b;
                        image.cpu.registers[source] = value;
                        let mut cpu = image.cpu;
                        expected(operation, bits, value, cpu.registers[destination])
                            .apply(&mut cpu, destination);
                        let step = retire(&mut cpu, code.len() as u32);
                        both(
                            TestModule::interpreter(),
                            &format!(
                                "{operation:?} {bits}-bit {destination:?}, {source:?}={value:x}"
                            ),
                            &code,
                            1,
                            &image,
                            &[step],
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn scans_replace_every_incoming_flag_record_even_when_the_destination_is_preserved() {
    for kind in [0, 1, 5, 9, 2, 6, 10, 3, 7, 11] {
        for bits in [16, 32] {
            for operation in OPERATIONS {
                // High source bits contribute to parity. Zero has even parity
                // regardless of the preserved destination.
                for source in [0, 0x100, 0x101] {
                    let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                    code.extend_from_slice(&[0x0f, operation.opcode(), 0xc2]);
                    let mut image = image(&code);
                    image.cpu.flags.kind = kind;
                    image.cpu.registers.edx = source;
                    let mut cpu = image.cpu;
                    expected(operation, bits, source, cpu.registers.eax)
                        .apply(&mut cpu, Gpr32::Eax);
                    let step = retire(&mut cpu, code.len() as u32);
                    both(
                        TestModule::interpreter(),
                        &format!(
                            "{operation:?} {bits}-bit source {source} replaces flag kind {kind}"
                        ),
                        &code,
                        1,
                        &image,
                        &[step],
                    );
                }
            }
        }
    }
}
