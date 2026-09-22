//! Operand exceptions, stack priority and access boundaries for narrow FLD.

use super::*;

fn stack_overflow_precedes_operand_exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x7f80_0001),
        Source::Single(1),
        Source::Double(0x7ff0_0000_0000_0001),
        Source::Double(1),
    ] {
        for control in [0x037f, 0x037e, 0x037d] {
            let code = [source.instruction(0x4000), vec![0xdf, 0xe0, 0x9b]].concat();
            let mut image = initial_image(&code, source, 0);
            set_control(&mut image.cpu.x87.control, control);
            let unmasked = control == 0x037e;
            let mut produced = completed_load(image.cpu, source);
            let word = if unmasked { 0xc7c1 } else { 0x7f41 };
            produced.x87.status = status(word);
            if !unmasked {
                produced.x87.tag_word = 0x8000;
                write_register_bits(&mut produced, 7, INDEFINITE);
            }
            let mut observed = produced;
            observed.eip += 2;
            observed.instruction_count = observed.instruction_count.wrapping_add(1);
            observed.registers.eax = 0x1111_0000 | u32::from(word);
            let mut waited = observed;
            if !unmasked {
                waited.eip += 1;
                waited.instruction_count = waited.instruction_count.wrapping_add(1);
            }
            checks.check(
                &format!("stack overflow before {source:?}, control {control:04x}"),
                &code,
                &image,
                &[
                    dispatch(produced),
                    dispatch(observed),
                    Step {
                        cpu: waited,
                        ram: &[],
                        exit: if unmasked {
                            Exit::FloatingPoint
                        } else {
                            Exit::Dispatch(waited.eip)
                        },
                    },
                ],
            );
        }
    }
}

fn unmasked_operand_exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, value, denormal) in [
        (Source::Single(0x7f80_0123), (0, 0), false),
        (Source::Double(0xfff0_0000_0000_0123), (0, 0), false),
        (Source::Single(1), (0x8000_0000_0000_0000, 0x3f6a), true),
        (
            Source::Double(0x8000_0000_0000_0001),
            (0x8000_0000_0000_0000, 0xbbcd),
            true,
        ),
    ] {
        let code = [source.instruction(0x4000), vec![0x90, 0xdf, 0xe0, 0x9b]].concat();
        let mut image = initial_image(&code, source, 0xffff);
        set_control(
            &mut image.cpu.x87.control,
            if denormal { 0x037d } else { 0x037e },
        );
        image.cpu.x87.control.invalid_mask |= 0x80;
        image.cpu.x87.control.denormal_mask |= 0x80;
        image.cpu.x87.status.top = 0xa8;
        let mut produced = completed_load(image.cpu, source);
        let word = if denormal { 0xfd82 } else { 0xc581 };
        produced.x87.status = status(word);
        if denormal {
            // FLD's unmasked denormal response still pushes the exact value.
            produced.x87.tag_word = 0x3fff;
            write_register_bits(&mut produced, 7, value);
        } else {
            // Unmasked invalid suppresses the push and preserves raw TOP too.
            produced.x87.status.top = 0xa8;
        }
        let mut nop = produced;
        nop.eip += 1;
        nop.instruction_count = nop.instruction_count.wrapping_add(1);
        let mut observed = nop;
        observed.eip += 2;
        observed.instruction_count = observed.instruction_count.wrapping_add(1);
        observed.registers.eax = 0x1111_0000 | u32::from(word);
        checks.check(
            &format!("unmasked operand exception for {source:?}"),
            &code,
            &image,
            &[
                dispatch(produced),
                dispatch(nop),
                dispatch(observed),
                Step {
                    cpu: observed,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }
}

fn operand_fault_ordering(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x7f80_0001),
        Source::Double(0x7ff0_0000_0000_0001),
    ] {
        let bytes = source.bytes();
        let address = 0x5001 - bytes.len() as u32;
        let code = source.instruction(address);
        let mut image = stack_image(&code, 0, 0);
        image.cpu.x87.status.precision = 0;
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.map(4, 0x8000, false);
        image.data(address + 0x4000, &bytes[..bytes.len() - 1]);
        checks.check(
            "operand page fault precedes current stack and NaN exceptions",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            }],
        );
        image.cpu.x87.status.invalid = 1;
        image.cpu.x87.status.error_summary = 1;
        image.cpu.x87.status.busy = 1;
        checks.check(
            "a pending exception precedes the operand page fault",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
}

fn operand_size_prefix_keeps_source_width(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, value) in [
        (Source::Single(0xbf80_0000), (0x8000_0000_0000_0000, 0xbfff)),
        (
            Source::Double(0x3ff0_0000_0000_0001),
            (0x8000_0000_0000_0800, 0x3fff),
        ),
    ] {
        let bytes = source.bytes();
        let address = 0x5000 - bytes.len() as u32;
        let code = [vec![0x66], source.instruction(address)].concat();
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        image.map(4, 0xf000, false);
        image.data(address + 0xb000, &bytes);
        let mut loaded = complete_x87(image.cpu, 7, (u16::from(source.opcode() & 7) << 8) | 5);
        loaded.x87.status = status(0x7d00);
        loaded.x87.tag_word = 0x3fff;
        loaded.x87.data_offset = address;
        loaded.x87.data_selector = 0x23;
        write_register_bits(&mut loaded, 7, value);
        checks.check(
            "66 leaves the FLD source width unchanged at the backing boundary",
            &code,
            &image,
            &[dispatch(loaded)],
        );
    }
}

test_frontends!(stack_priority, stack_overflow_precedes_operand_exceptions);
test_frontends!(unmasked_operands, unmasked_operand_exceptions);
test_frontends!(fault_ordering, operand_fault_ordering);
test_frontends!(source_width, operand_size_prefix_keeps_source_width);
