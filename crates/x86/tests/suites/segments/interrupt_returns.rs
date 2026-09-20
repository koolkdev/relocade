//! Protected interrupt returns commit a complete frame or retain the restart state.

#[path = "interrupt_returns/faults.rs"]
mod faults;
#[path = "interrupt_returns/progress.rs"]
mod progress;

use super::selector_cases::{check_one, code_defaults, code_descriptor as descriptor, loaded};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use crate::{
    CpuState, DescriptorTables, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, SegmentProfile,
};

fn frame(word: bool, offset: u32, selector: u16, flags: u32) -> Vec<u8> {
    let width = if word { 2 } else { 4 };
    let mut bytes = offset.to_le_bytes()[..width].to_vec();
    bytes.extend(selector.to_le_bytes());
    if !word {
        bytes.extend([0xa5, 0x5a]); // Ignored selector padding.
    }
    bytes.extend(&flags.to_le_bytes()[..width]);
    bytes
}

fn image(code: &[u8], profile: SegmentProfile) -> Image {
    let mut image = Image::new(code);
    code_defaults(&mut image, profile);
    image.cpu.segments.cs.selector = 0x1b;
    image.cpu.registers.esp = 0x9000;
    // Only the low bit carries NT.
    image.cpu.flags.bytes.nt = 0xfe;
    // A failed return preserves this pending recipe and every raw flag byte.
    image.cpu.flags.status_source.kind = 10;
    image.cpu.flags.status_source.left = 0x7fff_ffff;
    image.cpu.flags.status_source.right = 1;
    image.map(9, 0x8000, false);
    image
}

fn tables(limit: u32) -> DescriptorTables {
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0xc000, limit, SegmentDefaultSize::Bits32));
    tables
}

/// Clear represented flags, retaining inactive payload and, for word images, high flags.
fn cleared_flags(mut cpu: CpuState, word: bool) -> CpuState {
    cpu.flags.status_source.kind = 0;
    let flags = &mut cpu.flags.bytes;
    flags.cf = 0;
    flags.pf = 0;
    flags.af = 0;
    flags.zf = 0;
    flags.sf = 0;
    flags.of = 0;
    flags.tf = 0;
    flags.df = 0;
    flags.nt = 0;
    if !word {
        flags.ac = 0;
        flags.id = 0;
    }
    cpu
}

fn widths(engine: Engine) {
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        for operand_override in [false, true] {
            let word = (profile == SegmentProfile::Segmented16) != operand_override;
            for stack_big in [false, true] {
                if profile == SegmentProfile::Flat32 && !stack_big {
                    continue;
                }
                for next_word in [false, true] {
                    let mut code = if operand_override { vec![0x66] } else { vec![] };
                    code.extend([0x64, 0x67, 0xcf]); // Neither override changes implicit SS access.
                    let mut image = image(&code, profile);
                    image.cpu.registers.esp = 0xabcd_9000;
                    image.cpu.segments.ss =
                        loaded(0x23, 0, u32::MAX, if stack_big { 21 } else { 5 });
                    image.cpu.segments.fs = crate::StoredSegment::unusable(0);
                    image.map(if stack_big { 0xabcd9 } else { 9 }, 0x8000, false);
                    image.data(0x8000, &frame(word, 0x9234_5678, 0x27, 0));
                    let mut tables = tables(u32::MAX);
                    tables.insert(
                        0x27,
                        descriptor(
                            0xc000,
                            u32::MAX,
                            if next_word {
                                SegmentDefaultSize::Bits16
                            } else {
                                SegmentDefaultSize::Bits32
                            },
                        ),
                    );
                    let mut cpu = cleared_flags(image.cpu, word);
                    cpu.segments.cs =
                        loaded(0x27, 0xc000, u32::MAX, if next_word { 7 } else { 23 });
                    cpu.eip = if word { 0x5678 } else { 0x9234_5678 };
                    cpu.registers.esp = if word { 0xabcd_9006 } else { 0xabcd_900c };
                    cpu.instruction_count = 0;
                    check_one(
                        engine,
                        profile,
                        &code,
                        &image,
                        &[SegmentResolution::new(&tables, Segment::Cs, 0x27)],
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
fn operand_width_stack_width_and_new_code_defaults_are_independent() {
    widths(Engine::Wasmtime);
}

fn flag_images(engine: Engine) {
    for word in [false, true] {
        let code = if word { vec![0x66, 0xcf] } else { vec![0xcf] };
        // Literal bit positions exercise every modeled flag and ignored bits.
        for bits in [
            0,
            1,
            4,
            0x10,
            0x40,
            0x80,
            0x100,
            0x400,
            0x800,
            0x4000,
            0x40000,
            0x200000,
            0xffdb_b22a,
            u32::MAX,
        ] {
            let profile = SegmentProfile::Flat32;
            let mut image = image(&code, profile);
            image.data(0x8000, &frame(word, 0x200, 0x27, bits));
            let mut cpu = cleared_flags(image.cpu, word);
            let flags = &mut cpu.flags.bytes;
            flags.cf = u8::from(bits & 1 != 0);
            flags.pf = u8::from(bits & 4 != 0);
            flags.af = u8::from(bits & 0x10 != 0);
            flags.zf = u8::from(bits & 0x40 != 0);
            flags.sf = u8::from(bits & 0x80 != 0);
            flags.tf = u8::from(bits & 0x100 != 0);
            flags.df = u8::from(bits & 0x400 != 0);
            flags.of = u8::from(bits & 0x800 != 0);
            flags.nt = u8::from(bits & 0x4000 != 0);
            if !word {
                flags.ac = u8::from(bits & 0x40000 != 0);
                flags.id = u8::from(bits & 0x200000 != 0);
            }
            cpu.eip = 0x200;
            cpu.registers.esp = if word { 0x9006 } else { 0x900c };
            cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
            cpu.instruction_count = 0;
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)],
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
fn each_modeled_flag_restores_at_its_architectural_bit_and_word_preserves_high_flags() {
    flag_images(Engine::Wasmtime);
}

fn frame_boundaries(engine: Engine) {
    for (word, start, limit, stack_big, accepted, next_esp) in [
        (true, 0xfffau32, 0xffff, false, true, 0xbeef_0000),
        (false, 0xfff4, 0xffff, false, true, 0xbeef_0000),
        (false, 0xfffc, 0xffff, false, false, 0xbeef_fffc),
        (false, 0xfffc, 0x10007, false, true, 0xbeef_0008),
        (false, 0xffff_fffc, u32::MAX, true, true, 8),
    ] {
        let code = if word { vec![0x66, 0xcf] } else { vec![0xcf] };
        let profile = SegmentProfile::Segmented32;
        let mut image = image(&code, profile);
        image.cpu.segments.ss = loaded(0x23, 0, limit, if stack_big { 21 } else { 5 });
        image.cpu.registers.esp = if stack_big {
            start
        } else {
            0xbeef_0000 | start
        };
        let bytes = frame(word, 0x200, 0x27, 0);
        let split = bytes.len().min((0x1000 - (start & 0xfff)) as usize);
        image.map(start >> 12, 0xa000, false);
        image.data(0xa000 + (start & 0xfff), &bytes[..split]);
        if split < bytes.len() {
            image.map(start.wrapping_add(0x1000) >> 12, 0xc000, false);
            image.data(0xc000, &bytes[split..]);
        }
        let mut cpu = image.cpu;
        let mut resolutions = vec![];
        let exit = if accepted {
            cpu = cleared_flags(cpu, word);
            cpu.eip = 0x200;
            cpu.registers.esp = next_esp;
            cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
            cpu.instruction_count = 0;
            resolutions.push(SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27));
            Exit::Dispatch(cpu.eip)
        } else {
            Exit::StackFault { error: 0 }
        };
        check_one(
            engine,
            profile,
            &code,
            &image,
            &resolutions,
            Step {
                cpu,
                ram: &[],
                exit,
            },
        );
    }
}

#[test]
fn frames_are_consecutive_and_only_pointer_adjustment_wraps_at_stack_width() {
    frame_boundaries(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interrupt_return_widths_flags_and_frames() {
    widths(Engine::V8);
    flag_images(Engine::V8);
    frame_boundaries(Engine::V8);
}
