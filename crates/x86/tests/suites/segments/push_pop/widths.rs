use super::*;

fn selector_widths(engine: Engine) {
    // Every fixed form appears once; shared stack tests own the full size matrix.
    for (push, forms) in [(true, &PUSH[..]), (false, &POP[..])] {
        for (index, &(segment, opcode)) in forms.iter().enumerate() {
            let (profile, big, override_size, slot) = [
                (SegmentProfile::Segmented32, true, false, 4),
                (SegmentProfile::Segmented32, false, true, 2),
                (SegmentProfile::Segmented16, true, false, 2),
                (SegmentProfile::Segmented16, false, true, 4),
            ][index % 4];
            let mut code = vec![];
            if override_size {
                // Neither address size nor the last segment override changes SS.
                code.extend([0x66, 0x67, 0x64, 0x65]);
            }
            code.extend(opcode);
            let mut image = Image::new(&code);
            code_defaults(&mut image, profile);
            image.cpu.segments.ss = data(0x4000, 0xffff);
            image.cpu.segments.ss.attributes = stack_attributes(big);
            image.cpu.segments.fs = StoredSegment::unusable(0xf327);
            image.cpu.segments.gs = StoredSegment::unusable(0xf327);
            if segment == Segment::Ds && push {
                image.cpu.segments.ds = StoredSegment::unusable(0xf327);
            }
            image.cpu.segments[segment].selector = 0xf327;
            image.cpu.registers.esp = if big { 0x104 } else { 0xabcd_0104 };
            image.map(4, 0x8000, true);
            image.data(0x8100, &[0xa5; 12]);
            let mut cpu = image.cpu;
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            if push {
                cpu.registers.esp -= slot;
                check_one(
                    engine,
                    profile,
                    &code,
                    &image,
                    &[],
                    Step {
                        cpu,
                        ram: &[(0x8104 - slot, &[0x27, 0xf3])],
                        exit: Exit::Dispatch(cpu.eip),
                    },
                );
            } else {
                image.data(0x8104, &[0x27, 0xf3]);
                let size = if big {
                    SegmentDefaultSize::Bits16
                } else {
                    SegmentDefaultSize::Bits32
                };
                let mut tables = DescriptorTables::default();
                tables.insert(0xf327, descriptor(0x9000, size));
                cpu.registers.esp += slot;
                cpu.segments[segment] = StoredSegment {
                    base: 0x9000,
                    limit: 0xffff,
                    selector: 0xf327,
                    attributes: SegmentAttributes::from_bits(if big { 5 } else { 0x15 }),
                };
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

#[test]
fn every_segment_form_separates_selector_width_operand_size_and_stack_width() {
    selector_widths(Engine::Wasmtime);
}

fn unread_slot_bytes(engine: Engine) {
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        for push in [false, true] {
            let mut code = vec![];
            if profile == SegmentProfile::Segmented16 {
                code.push(0x66);
            }
            code.push(if push { 0x1e } else { 0x1f }); // PUSH/POP DS, dword slot.
            let mut image = Image::new(&code);
            code_defaults(&mut image, profile);
            let flat = profile == SegmentProfile::Flat32;
            if !flat {
                image.cpu.segments.ss = data(0x4000, 0xfff);
            }
            image.cpu.segments.ds.selector = 0xf327;
            let stack_page = if flat { 0x4000 } else { 0 };
            image.cpu.registers.esp = stack_page + if push { 0x1002 } else { 0xffe };
            image.map(4, 0x8000, true);
            image.data(0x8ffe, if push { &[0xa5, 0x5a] } else { &[0x27, 0xf3] });
            let mut tables = DescriptorTables::default();
            tables.insert(0xf327, descriptor(0x9000, SegmentDefaultSize::Bits16));
            let mut cpu = image.cpu;
            cpu.registers.esp = stack_page + if push { 0xffe } else { 0x1002 };
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            let resolutions = if push {
                vec![]
            } else {
                cpu.segments.ds = StoredSegment {
                    base: 0x9000,
                    limit: 0xffff,
                    selector: 0xf327,
                    attributes: SegmentAttributes::from_bits(5),
                };
                vec![SegmentResolution::new(&tables, Segment::Ds, 0xf327)]
            };
            check_one(
                engine,
                profile,
                &code,
                &image,
                &resolutions,
                Step {
                    cpu,
                    ram: if push {
                        &[(0x8ffe, &[0x27, 0xf3])]
                    } else {
                        &[]
                    },
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn a_dword_slot_does_not_access_its_third_or_fourth_byte() {
    unread_slot_bytes(Engine::Wasmtime);
}

fn split_selector(engine: Engine) {
    for push in [false, true] {
        let code = [0x0f, if push { 0xa8 } else { 0xa9 }]; // PUSH/POP GS.
        let mut image = Image::new(&code);
        image.cpu.registers.esp = if push { 0x5003 } else { 0x4fff };
        image.cpu.segments.gs = StoredSegment::unusable(0xf327);
        image.map(4, 0x8000, true);
        image.map(5, 0xa000, true);
        image.data(0x8fff, &[if push { 0xa5 } else { 0x27 }]);
        image.data(0xa000, &[if push { 0x5a } else { 0xf3 }, 0x11, 0x22]);
        let mut tables = DescriptorTables::default();
        tables.insert(0xf327, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut cpu = image.cpu;
        cpu.registers.esp = if push { 0x4fff } else { 0x5003 };
        cpu.eip += 2;
        cpu.instruction_count = 0;
        let resolutions = if push {
            vec![]
        } else {
            cpu.segments.gs = StoredSegment {
                base: 0x9000,
                limit: 0xffff,
                selector: 0xf327,
                attributes: SegmentAttributes::from_bits(5),
            };
            vec![SegmentResolution::new(&tables, Segment::Gs, 0xf327)]
        };
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &resolutions,
            Step {
                cpu,
                ram: if push {
                    &[(0x8fff, &[0x27]), (0xa000, &[0xf3])]
                } else {
                    &[]
                },
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn selector_words_can_cross_discontiguous_physical_pages() {
    split_selector(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_stack_widths_and_access_spans() {
    selector_widths(Engine::V8);
    unread_slot_bytes(Engine::V8);
    split_selector(Engine::V8);
}
