use super::*;
use crate::register::Gpr32;

fn register_loads(engine: Engine, all_registers: bool) {
    for profile in [SegmentProfile::Flat32, SegmentProfile::Segmented16] {
        for prefix in [false, true] {
            for segment in [
                Segment::Es,
                Segment::Ss,
                Segment::Ds,
                Segment::Fs,
                Segment::Gs,
            ] {
                for (index, register) in Gpr32::ALL.into_iter().enumerate() {
                    if !all_registers && index != 4 {
                        continue;
                    }
                    let mut code = vec![];
                    if prefix {
                        code.push(0x66);
                    }
                    code.extend([0x8e, 0xc0 | ((segment as u8) << 3) | index as u8]);
                    let mut image = Image::new(&code);
                    code_defaults(&mut image, profile);
                    image.cpu.registers[register] = 0xabcd_f327;
                    let mut tables = DescriptorTables::default();
                    tables.insert(0xf327, descriptor(0x8765_0000, SegmentDefaultSize::Bits16));
                    let mut cpu = image.cpu;
                    cpu.segments[segment] = StoredSegment {
                        base: 0x8765_0000,
                        limit: 0xffff,
                        selector: 0xf327,
                        attributes: SegmentAttributes::from_bits(5),
                    };
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    check_one(
                        engine,
                        profile,
                        &code,
                        &image,
                        &[SegmentResolution::new(&tables, segment, 0xf327)],
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

#[test]
fn segment_loads_take_the_low_word_of_every_gpr_and_commit_each_cache() {
    register_loads(Engine::Wasmtime, true);
}

fn old_memory_cache(engine: Engine) {
    for (segment, suffix) in [(Segment::Ds, &[0x1b][..]), (Segment::Ss, &[0x14, 0x24][..])] {
        // MOV DS,[EBX] or MOV SS,[ESP], with a redundant operand-size override.
        let mut code = vec![0x66, 0x8e];
        code.extend(suffix);
        let mut image = Image::new(&code);
        image.cpu.segments[segment] = data(0x4000, 0xfff);
        image.cpu.registers.ebx = 0xffe;
        image.cpu.registers.esp = 0xffe;
        image.map(4, 0x8000, false);
        image.data(0x8ffe, &[0x27, 0xf3]);
        let mut tables = DescriptorTables::default();
        tables.insert(0xf327, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut cpu = image.cpu;
        cpu.segments[segment] = StoredSegment {
            base: 0x9000,
            limit: 0xffff,
            selector: 0xf327,
            attributes: SegmentAttributes::from_bits(5),
        };
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            &code,
            &image,
            &[SegmentResolution::new(&tables, segment, 0xf327)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn memory_sources_use_two_bytes_through_the_old_ds_or_ss_cache() {
    old_memory_cache(Engine::Wasmtime);
}

fn overridden_source(engine: Engine) {
    for profile in [SegmentProfile::Flat32, SegmentProfile::Segmented16] {
        // MOV FS,GS:[BX+SI] / GS:[EBX+ESI], with GS replacing an earlier DS override.
        let mut code = vec![0x3e, 0x65, 0x67, 0x8e];
        code.extend(if profile == SegmentProfile::Flat32 {
            &[0x20][..]
        } else {
            &[0x24, 0x33]
        });
        let mut image = Image::new(&code);
        code_defaults(&mut image, profile);
        image.cpu.registers.ebx = 0xf00;
        image.cpu.registers.esi = 0xfe;
        image.cpu.segments.gs = data(0x4000, 0xfff);
        image.map(4, 0x8000, false);
        image.data(0x8ffe, &[0x27, 0]);
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x27,
            SegmentDescriptor {
                kind: SegmentDescriptorKind::Data {
                    writable: false,
                    expand_down: false,
                },
                ..descriptor(0x9000, SegmentDefaultSize::Bits16)
            },
        );
        let mut cpu = image.cpu;
        cpu.segments.fs = StoredSegment {
            base: 0x9000,
            limit: 0xffff,
            selector: 0x27,
            attributes: SegmentAttributes::from_bits(1),
        };
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        check_one(
            engine,
            profile,
            &code,
            &image,
            &[SegmentResolution::new(&tables, Segment::Fs, 0x27)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn segment_load_sources_honor_address_size_and_the_last_segment_override() {
    overridden_source(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_load_sources_and_cache_commit() {
    register_loads(Engine::V8, false);
    old_memory_cache(Engine::V8);
    overridden_source(Engine::V8);
}
