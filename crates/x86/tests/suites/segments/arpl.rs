//! ARPL adjusts selector values without consulting descriptors or loading caches.

#[path = "arpl/memory.rs"]
mod memory;
#[path = "arpl/progress.rs"]
mod progress;

use super::{data, selector_cases::code_defaults};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use crate::{CpuState, Gpr32, SegmentProfile};

const PROFILES: [SegmentProfile; 3] = [
    SegmentProfile::Flat32,
    SegmentProfile::Segmented32,
    SegmentProfile::Segmented16,
];

fn image(code: &[u8], profile: SegmentProfile) -> Image {
    let mut image = Image::new(code);
    code_defaults(&mut image, profile);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.cf = 1;
    image.cpu.flags.bytes.pf = 0;
    image.cpu.flags.bytes.af = 1;
    image.cpu.flags.bytes.zf = 0;
    image.cpu.flags.bytes.sf = 0;
    image.cpu.flags.bytes.of = 1;
    image
}

fn completed(image: &Image, len: usize, adjusted: bool) -> CpuState {
    let mut cpu = image.cpu;
    cpu.flags.bytes.zf = u8::from(adjusted);
    cpu.eip += len as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn check_one(engine: Engine, profile: SegmentProfile, code: &[u8], image: &Image, step: Step<'_>) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, profile);
    let wanted = expected(image, &[step]);
    for module in [block, TestModule::interpreter_with_profile(profile)] {
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            wanted,
            "{} {profile:?} {code:02x?}",
            module.entry,
        );
    }
}

#[test]
fn every_rpl_pair_preserves_selector_index_and_table_bits_and_changes_only_zf() {
    // Rows are destination RPL, columns source RPL; cells are (new RPL, ZF).
    let outcomes = [
        [(0, false), (1, true), (2, true), (3, true)],
        [(1, false), (1, false), (2, true), (3, true)],
        [(2, false), (2, false), (2, false), (3, true)],
        [(3, false), (3, false), (3, false), (3, false)],
    ];
    let code = [0x63, 0xc8]; // ARPL AX,CX.
    for (destination_rpl, row) in outcomes.into_iter().enumerate() {
        for (source_rpl, (rpl, adjusted)) in row.into_iter().enumerate() {
            let profile = SegmentProfile::Flat32;
            let mut image = image(&code, profile);
            image.cpu.registers.eax = 0xabcd_8004 | destination_rpl as u32;
            image.cpu.registers.ecx = 0x9876_fff8 | source_rpl as u32;
            image.cpu.flags.bytes.zf = u8::from(!adjusted);
            let mut cpu = completed(&image, code.len(), adjusted);
            cpu.registers.eax = 0xabcd_8004 | rpl;
            check_one(
                Engine::Wasmtime,
                profile,
                &code,
                &image,
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn all_registers_use_word_operands_in_both_code_sizes_even_with_an_override() {
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
    for profile in PROFILES {
        for operand_override in [false, true] {
            for (index, destination) in registers.into_iter().enumerate() {
                for alias in [false, true] {
                    let source_index = if alias { index } else { (index + 3) % 8 };
                    let mut code = if operand_override { vec![0x66] } else { vec![] };
                    code.extend([0x63, 0xc0 | ((source_index as u8) << 3) | index as u8]);
                    let mut image = image(&code, profile);
                    image.cpu.registers[registers[source_index]] = 0x9876_0003;
                    // A null selector is still an ordinary value to ARPL.
                    image.cpu.registers[destination] = 0xabcd_0000;
                    image.cpu.flags.bytes.zf = u8::from(alias);
                    let mut cpu = completed(&image, code.len(), !alias);
                    cpu.registers[destination] = if alias { 0xabcd_0000 } else { 0xabcd_0003 };
                    check_one(
                        Engine::Wasmtime,
                        profile,
                        &code,
                        &image,
                        Step {
                            cpu,
                            ram: &[],
                            exit: Exit::Dispatch(cpu.eip),
                        },
                    );
                }
            }
        }
    }
}
