use super::*;

fn registers_and_widths(engine: Engine) {
    for (segment, opcode) in FORMS {
        for (profile, word, destination) in [
            (SegmentProfile::Flat32, false, Gpr32::Ebx),
            (SegmentProfile::Segmented16, true, Gpr32::Edi),
        ] {
            let mut code = opcode.to_vec();
            code.push(((destination as u8) << 3) | if word { 0 } else { 3 });
            let mut image = Image::new(&code);
            code_defaults(&mut image, profile);
            image.cpu.registers.ebx = 0x4000;
            image.cpu.registers.esi = 0;
            image.map(4, 0x8000, false);
            image.data(
                0x8000,
                if word {
                    &[0x78, 0x56, 0x27, 0xf3]
                } else {
                    &[0x78, 0x56, 0x34, 0x92, 0x27, 0xf3]
                },
            );
            let mut tables = DescriptorTables::default();
            // The loaded offset is data, even when it exceeds the new limit.
            tables.insert(
                0xf327,
                SegmentDescriptor {
                    limit: crate::SegmentLimit::bytes(0x10).unwrap(),
                    ..descriptor(0x9000, SegmentDefaultSize::Bits16)
                },
            );
            let mut cpu = image.cpu;
            cpu.registers[destination] = if word { 0x8888_5678 } else { 0x9234_5678 };
            cpu.segments[segment] = loaded(0xf327, 0x9000, 0x10, 5);
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

#[test]
fn every_pointer_load_supports_both_offset_widths_and_uses_entry_addresses() {
    registers_and_widths(Engine::Wasmtime);
}

fn source_spans(engine: Engine) {
    for (word, split, second_frame) in [
        (true, 4, 0xa000),
        (false, 6, 0xa000),
        (true, 3, 0xa000),
        (false, 3, 0xa000),
        (false, 4, 0x9000),
        (true, 1, 0x9000),
    ] {
        let payload = if word {
            &[0x78, 0x56, 0x27, 0xf3][..]
        } else {
            &[0x78, 0x56, 0x34, 0x92, 0x27, 0xf3][..]
        };
        let mut code = vec![];
        if word {
            code.push(0x66);
        }
        code.extend([0x0f, 0xb4, 0x03]); // LFS AX/EAX,[EBX].
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = 0x5000 - split as u32;
        image.map(4, 0x8000, false);
        image.data(0x9000 - split as u32, &payload[..split]);
        // When the complete pointer fits, the next page is absent.
        if split < payload.len() {
            image.map(5, second_frame, false);
            image.data(second_frame, &payload[split..]);
        }
        let mut tables = DescriptorTables::default();
        tables.insert(0xf327, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut cpu = image.cpu;
        cpu.registers.eax = if word { 0x1111_5678 } else { 0x9234_5678 };
        cpu.segments.fs = loaded(0xf327, 0x9000, 0xffff, 5);
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &[SegmentResolution::new(&tables, Segment::Fs, 0xf327)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn pointer_sources_read_exactly_four_or_six_bytes_with_split_offset_or_selector_fields() {
    source_spans(Engine::Wasmtime);
}

fn old_address_state(engine: Engine) {
    // The source uses the old destination segment, and EBP participates in its address.
    for (segment, code) in [
        (Segment::Ds, &[0x3e, 0xc5, 0x6c, 0xb5, 7][..]), // LDS EBP,DS:[EBP+ESI*4+7].
        (Segment::Ss, &[0x0f, 0xb2, 0x6c, 0xb5, 7][..]), // LSS EBP,[EBP+ESI*4+7].
        (Segment::Gs, &[0x64, 0x65, 0x67, 0x0f, 0xb5, 0x6a, 7][..]), // LGS EBP,GS:[BP+SI+7].
    ] {
        let mut image = Image::new(code);
        image.cpu.registers.ebp = 0x100;
        image.cpu.registers.esi = 3;
        image.cpu.segments[segment] = data(0x4000, 0xffff);
        image.cpu.segments.fs = StoredSegment::unusable(3);
        image.map(4, 0x8000, false);
        image.data(
            if segment == Segment::Gs {
                0x810a
            } else {
                0x8113
            },
            &[0x78, 0x56, 0x34, 0x92, 0x27, 0xf3],
        );
        let mut tables = DescriptorTables::default();
        tables.insert(0xf327, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut cpu = image.cpu;
        cpu.registers.ebp = 0x9234_5678;
        cpu.segments[segment] = loaded(0xf327, 0x9000, 0xffff, 5);
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            code,
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
fn both_fields_use_entry_address_registers_and_the_old_overridden_segment_cache() {
    old_address_state(Engine::Wasmtime);
}

fn address_boundaries(engine: Engine) {
    for wrap_linear in [false, true] {
        let code = [0x67, 0x0f, 0xb5, 0]; // LGS EAX,[BX+SI].
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = 0xffff;
        image.cpu.registers.esi = 0;
        image.cpu.segments.ds = data(if wrap_linear { 0xffff_0000 } else { 0 }, 0x1ffff);
        image.map(if wrap_linear { 0xfffff } else { 0xf }, 0x8000, false);
        image.map(if wrap_linear { 0 } else { 0x10 }, 0xa000, false);
        image.data(0x8fff, &[0x78]);
        image.data(0xa000, &[0x56, 0x34, 0x92, 0x27, 0xf3]);
        let mut tables = DescriptorTables::default();
        tables.insert(0xf327, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut cpu = image.cpu;
        cpu.registers.eax = 0x9234_5678;
        cpu.segments.gs = loaded(0xf327, 0x9000, 0xffff, 5);
        cpu.eip += 4;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            &code,
            &image,
            &[SegmentResolution::new(&tables, Segment::Gs, 0xf327)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn fields_continue_past_a_16_bit_starting_offset_and_wrap_only_at_linear_32_bits() {
    address_boundaries(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_pointer_load_values_widths_and_sources() {
    registers_and_widths(Engine::V8);
    source_spans(Engine::V8);
    old_address_state(Engine::V8);
    address_boundaries(Engine::V8);
}
