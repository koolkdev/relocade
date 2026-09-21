use super::*;

fn source_spans(engine: Engine) {
    for (word, split) in [
        (true, 4),
        (false, 6),
        (true, 1),
        (true, 3),
        (false, 3),
        (false, 5),
    ] {
        let payload = pointer(word, 0x9234_5678, 0x27);
        let code = if word {
            &[0x66, 0xff, 0x2b][..]
        } else {
            &[0xff, 0x2b]
        };
        let mut image = Image::new(code);
        image.cpu.registers.ebx = 0x5000 - split as u32;
        image.map(4, 0x8000, false);
        image.data(0x9000 - split as u32, &payload[..split]);
        // Exact page-end reads need no next page; split offset/selector fields use scattered pages.
        if split < payload.len() {
            image.map(5, 0xa000, false);
            image.data(0xa000, &payload[split..]);
        }
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x27,
            descriptor(0x9000, u32::MAX, SegmentDefaultSize::Bits32),
        );
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0x9000, u32::MAX, 23);
        cpu.eip = if word { 0x5678 } else { 0x9234_5678 };
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Flat32,
            code,
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

#[test]
fn memory_pointers_read_exact_spans_and_split_offset_or_selector_fields() {
    source_spans(Engine::Wasmtime);
}

fn old_address_state(engine: Engine) {
    for (segment, code, source) in [
        (Segment::Cs, &[0x2e, 0xff, 0x6d, 7][..], 0x8107), // CS:[EBP+7].
        (Segment::Ss, &[0xff, 0x6c, 0xb5, 7][..], 0x8113), // [EBP+ESI*4+7].
        (Segment::Gs, &[0x64, 0x65, 0x67, 0xff, 0x6a, 7][..], 0x810a), // GS:[BP+SI+7].
    ] {
        let mut image = Image::new(code);
        image.cpu.registers.ebp = 0x100;
        image.cpu.registers.esi = 3;
        image.cpu.segments.fs = StoredSegment::unusable(3);
        if segment == Segment::Cs {
            image.cpu.segments.cs.base = 0x4000;
            image.cpu.segments.cs.limit = 0xffff;
            image.map(5, 0x3000, false); // Fetch through the old CS as well.
        } else {
            image.cpu.segments[segment] = data(0x4000, 0xffff);
        }
        image.map(4, 0x8000, false);
        image.data(source, &pointer(false, 0x200, 0x27));
        let mut tables = DescriptorTables::default();
        tables.insert(0x27, descriptor(0x9000, 0xffff, SegmentDefaultSize::Bits32));
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0x9000, 0xffff, 23);
        cpu.eip = 0x200;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            code,
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

#[test]
fn indirect_jumps_resolve_the_source_using_the_old_address_and_segment_state() {
    old_address_state(Engine::Wasmtime);
}

fn address_boundaries(engine: Engine) {
    for wrap_linear in [false, true] {
        let code = [0x67, 0xff, 0x28]; // [BX+SI], dword offset.
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = 0xffff;
        image.cpu.registers.esi = 0;
        image.cpu.segments.ds = data(if wrap_linear { 0xffff_0000 } else { 0 }, 0x1ffff);
        image.map(if wrap_linear { 0xfffff } else { 0xf }, 0x8000, false);
        image.map(if wrap_linear { 0 } else { 0x10 }, 0xa000, false);
        image.data(0x8fff, &[0x78]);
        image.data(0xa000, &[0x56, 0x34, 0x92, 0x27, 0]);
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x27,
            descriptor(0x9000, u32::MAX, SegmentDefaultSize::Bits32),
        );
        let mut cpu = image.cpu;
        cpu.segments.cs = loaded(0x27, 0x9000, u32::MAX, 23);
        cpu.eip = 0x9234_5678;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
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

#[test]
fn pointer_fields_continue_beyond_the_starting_16_bit_offset_and_wrap_linear_32_bits() {
    address_boundaries(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_far_jump_memory_sources() {
    source_spans(Engine::V8);
    old_address_state(Engine::V8);
    address_boundaries(Engine::V8);
}
