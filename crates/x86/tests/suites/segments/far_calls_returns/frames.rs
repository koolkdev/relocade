use super::*;

fn call_widths(engine: Engine) {
    // Each immediate/indirect form covers the four operand-width/SS.B combinations.
    for (profile, code, word, stack_big) in [
        (
            SegmentProfile::Flat32,
            &[0x9a, 0x78, 0x56, 0x34, 0x92, 0x24, 0][..],
            false,
            true,
        ),
        (
            SegmentProfile::Segmented32,
            &[0x66, 0x9a, 0x78, 0x56, 0x24, 0],
            true,
            false,
        ),
        (
            SegmentProfile::Segmented16,
            &[0x9a, 0x78, 0x56, 0x24, 0],
            true,
            true,
        ),
        (
            SegmentProfile::Segmented16,
            &[0x66, 0x9a, 0x78, 0x56, 0x34, 0x92, 0x24, 0],
            false,
            false,
        ),
        (SegmentProfile::Flat32, &[0xff, 0x1b], false, true),
        (
            SegmentProfile::Segmented32,
            &[0x66, 0xff, 0x1b],
            true,
            false,
        ),
        (SegmentProfile::Segmented16, &[0xff, 0x18], true, true),
        (
            SegmentProfile::Segmented16,
            &[0x66, 0xff, 0x18],
            false,
            false,
        ),
    ] {
        let mut image = Image::new(code);
        image.cpu.segments.cs.selector = 0x1b;
        code_defaults(&mut image, profile);
        image.cpu.registers.esp = 0x1234_9008;
        image.cpu.registers.ebx = 0x4000;
        image.cpu.registers.esi = 0;
        let base = if profile == SegmentProfile::Flat32 {
            0
        } else {
            0x10000
        };
        image.cpu.segments.ss = loaded(0x23, base, u32::MAX, if stack_big { 21 } else { 5 });
        image.map(
            if stack_big { 0x12349 } else { 9 } + (base >> 12),
            0x8000,
            true,
        );
        image.data(0x8000, &[0xa5; 16]);
        image.map(4, 0x5000, false);
        image.data(0x5000, &pointer(word, 0x9234_5678, 0x24));
        let mut table = tables(u32::MAX);
        table.insert(
            0x24,
            descriptor(
                0xc000,
                u32::MAX,
                if word {
                    SegmentDefaultSize::Bits32
                } else {
                    SegmentDefaultSize::Bits16
                },
            ),
        );
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0xc000, u32::MAX, if word { 23 } else { 7 });
        cpu.eip = if word { 0x5678 } else { 0x9234_5678 };
        cpu.registers.esp = if word { 0x1234_9004 } else { 0x1234_9000 };
        cpu.instruction_count = 0;
        let saved = pointer(word, 0x1000 + code.len() as u32, 0x1b);
        check_one(
            engine,
            profile,
            code,
            &image,
            &[SegmentResolution::new(&table, Segment::Cs, 0x24)],
            Step {
                cpu,
                ram: &[(if word { 0x8004 } else { 0x8000 }, &saved)],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn call_forms_keep_operand_width_code_defaults_and_stack_width_independent() {
    call_widths(Engine::Wasmtime);
}

fn return_widths(engine: Engine) {
    for (profile, operand_override, word, stack_big) in [
        (SegmentProfile::Flat32, false, false, true),
        (SegmentProfile::Segmented32, true, true, false),
        (SegmentProfile::Segmented16, false, true, true),
        (SegmentProfile::Segmented16, true, false, false),
    ] {
        for cleanup in [None, Some(0xffff)] {
            let code = ret(operand_override, cleanup);
            let mut image = Image::new(&code);
            image.cpu.segments.cs.selector = 0x1b;
            code_defaults(&mut image, profile);
            image.cpu.registers.esp = 0x1234_9000;
            image.cpu.segments.ss = loaded(0x23, 0, u32::MAX, if stack_big { 21 } else { 5 });
            image.map(if stack_big { 0x12349 } else { 9 }, 0x8000, false);
            image.data(0x8000, &[0xa5; 8]);
            image.data(0x8000, &pointer(word, 0x9234_5678, 0x27));
            let mut cpu = image.cpu;
            cpu.segments.cs = loaded(0x27, 0xc000, u32::MAX, 23);
            cpu.eip = if word { 0x5678 } else { 0x9234_5678 };
            cpu.registers.esp = match (word, cleanup, stack_big) {
                (true, None, _) => 0x1234_9004,
                (false, None, _) => 0x1234_9008,
                (true, Some(_), true) => 0x1235_9003,
                (false, Some(_), true) => 0x1235_9007,
                (true, Some(_), false) => 0x1234_9003,
                (false, Some(_), false) => 0x1234_9007,
            };
            cpu.instruction_count = 0;
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[SegmentResolution::new(&tables(u32::MAX), Segment::Cs, 0x27)],
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
fn return_forms_pop_two_slots_and_discard_unsigned_cleanup_without_accessing_it() {
    return_widths(Engine::Wasmtime);
}

fn frame_boundaries(engine: Engine) {
    // Capacity covers both complete slots, while the unused selector padding is
    // excluded from paging. Fields remain consecutive across a 16-bit boundary.
    for returning in [false, true] {
        for (start, limit, stack_big, accepted) in [
            (0x0ffau32, 0x1001, true, true), // Selector ends at the page boundary.
            (0x0ffa, 0x0fff, true, false),   // Its reserved padding exceeds SS.limit.
            (0xfff8, 0xffff, false, true),   // Successful pop wraps SP to zero.
            (0xfffc, 0xffff, false, false),  // Frame cannot wrap at 16 bits.
            (0xfffc, 0x10003, false, true),  // A larger B=0 segment covers the frame.
            (0xffff_fffc, u32::MAX, true, true), // Full-size segments allow 32-bit wrap.
        ] {
            let code = if returning {
                ret(false, None)
            } else {
                immediate(false, 0x200, 0x27)
            };
            let mut image = Image::new(&code);
            image.cpu.segments.cs.selector = 0x1b;
            image.cpu.segments.ss = loaded(0x23, 0, limit, if stack_big { 21 } else { 5 });
            let initial_sp = if returning {
                start
            } else {
                start.wrapping_add(8)
            };
            image.cpu.registers.esp = if stack_big {
                initial_sp
            } else {
                0xbeef_0000 | (initial_sp & 0xffff)
            };
            // On CALL, subtracting eight must reproduce the chosen start after wrapping.
            image.map(start >> 12, 0x8000, !returning);
            let physical = 0x8000 + (start & 0xfff);
            image.data(
                physical,
                &vec![0xa5; 8.min(0x1000 - (start & 0xfff)) as usize],
            );
            let payload = pointer(false, 0x200, 0x27);
            let split = (0x1000 - (start & 0xfff)).min(6) as usize;
            if returning {
                image.data(physical, &payload[..split]);
            }
            if split < 6 {
                image.map(start.wrapping_add(0x1000) >> 12, 0x5000, !returning);
                image.data(0x5000, &[0xa5; 8]);
                if returning {
                    image.data(0x5000, &payload[split..]);
                }
            }
            let mut cpu = image.cpu;
            let mut ram: Vec<(u32, &[u8])> = vec![];
            let saved = pointer(false, 0x1007, 0x1b);
            let exit = if accepted {
                cpu.segments.cs = loaded(0x27, 0xc000, 0xffff, 23);
                cpu.eip = 0x200;
                cpu.instruction_count = 0;
                cpu.registers.esp = if returning {
                    if stack_big {
                        start.wrapping_add(8)
                    } else {
                        0xbeef_0000 | ((start.wrapping_add(8)) & 0xffff)
                    }
                } else if stack_big {
                    start
                } else {
                    0xbeef_0000 | start
                };
                if !returning {
                    ram.push((physical, &saved[..split]));
                    if split < 6 {
                        ram.push((0x5000, &saved[split..]));
                    }
                }
                Exit::Dispatch(cpu.eip)
            } else {
                Exit::StackFault { error: 0 }
            };
            let resolutions = if !returning || accepted {
                vec![SegmentResolution::new(&tables(0xffff), Segment::Cs, 0x27)]
            } else {
                vec![]
            };
            check_one(
                engine,
                SegmentProfile::Segmented32,
                &code,
                &image,
                &resolutions,
                Step {
                    cpu,
                    ram: &ram,
                    exit,
                },
            );
        }
    }
}

#[test]
fn complete_frame_capacity_and_selector_transfer_extent_are_distinct() {
    frame_boundaries(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_call_return_frames_and_widths() {
    call_widths(Engine::V8);
    return_widths(Engine::V8);
    frame_boundaries(Engine::V8);
}
