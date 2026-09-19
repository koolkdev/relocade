use super::*;
use crate::register::Gpr32;

#[test]
fn all_visible_selectors_store_to_every_gpr_at_both_operand_sizes() {
    for profile in [SegmentProfile::Flat32, SegmentProfile::Segmented16] {
        for prefix in [false, true] {
            for segment in Segment::ALL {
                for (index, register) in Gpr32::ALL.into_iter().enumerate() {
                    let mut code = vec![];
                    if prefix {
                        code.push(0x66);
                    }
                    code.extend([0x8c, 0xc0 | ((segment as u8) << 3) | index as u8]);
                    let mut image = Image::new(&code);
                    code_defaults(&mut image, profile);
                    image.cpu.segments[segment].selector = 0xf327;
                    let mut cpu = image.cpu;
                    let word = (profile == SegmentProfile::Segmented16) != prefix;
                    cpu.registers[register] = if word {
                        (cpu.registers[register] & 0xffff_0000) | 0xf327
                    } else {
                        0xf327
                    };
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    check_one(
                        Engine::Wasmtime,
                        profile,
                        &code,
                        &image,
                        &[],
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

fn unusable_selectors(engine: Engine) {
    for (segment, selector) in [(Segment::Ds, 3), (Segment::Fs, 0xf327)] {
        let code = [0x8c, 0xc0 | ((segment as u8) << 3)];
        let mut image = Image::new(&code);
        image.cpu.segments[segment] = StoredSegment::unusable(selector);
        let mut cpu = image.cpu;
        cpu.registers.eax = u32::from(selector);
        cpu.eip += 2;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            &code,
            &image,
            &[],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn reading_a_selector_does_not_require_a_usable_cache() {
    unusable_selectors(Engine::Wasmtime);
}

fn memory_width(engine: Engine) {
    for profile in [SegmentProfile::Flat32, SegmentProfile::Segmented16] {
        for operand_override in [false, true] {
            // GS overrides DS; address override selects the opposite address size.
            let mut code = vec![0x3e, 0x65, 0x67];
            if operand_override {
                code.push(0x66);
            }
            code.extend([
                0x8c,
                if profile == SegmentProfile::Segmented16 {
                    0x03
                } else {
                    0x07
                },
            ]);
            let mut image = Image::new(&code);
            code_defaults(&mut image, profile);
            image.cpu.registers.ebx = 0x0ffe;
            image.cpu.segments.gs = data(0x4000, 0x0fff);
            image.cpu.segments.es.selector = 0xf327;
            image.map(4, 0x8000, true);
            image.data(0x8ffd, &[0xaa, 0xbb, 0xcc, 0xdd]);
            let mut cpu = image.cpu;
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            // A four-byte write would exceed both GS and the mapped page.
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[],
                Step {
                    cpu,
                    ram: &[(0x8ffe, &[0x27, 0xf3])],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn selector_memory_stores_use_two_bytes_with_either_operand_size() {
    memory_width(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_selector_reads_and_memory_widths() {
    unusable_selectors(Engine::V8);
    memory_width(Engine::V8);
}
