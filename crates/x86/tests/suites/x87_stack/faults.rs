//! Masked stack results and deferred unmasked exception delivery.

use super::*;

fn masked_stack_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (name, code, tags, final_tags, final_top, destination, overflow) in [
        (
            "FLD overflow overwrites push destination",
            [0xd9, 0xc0],
            0,
            0x8000,
            7,
            7,
            true,
        ),
        (
            "FLD underflow pushes indefinite",
            [0xd9, 0xc2],
            0xc030,
            0x8030,
            7,
            7,
            false,
        ),
        (
            "FLD underflow precedes simultaneous overflow",
            [0xd9, 0xc2],
            0x0030,
            0x8030,
            7,
            7,
            false,
        ),
        (
            "FST empty source writes occupied destination",
            [0xdd, 0xd2],
            0x0003,
            0x0023,
            0,
            2,
            false,
        ),
        (
            "FST empty source can fill itself",
            [0xdd, 0xd0],
            0x0003,
            0x0002,
            0,
            0,
            false,
        ),
        (
            "FSTP underflow still pops",
            [0xdd, 0xda],
            0x0003,
            0x0023,
            1,
            2,
            false,
        ),
        (
            "FSTP self underflow writes then empties",
            [0xdd, 0xd8],
            0x0003,
            0x0003,
            1,
            0,
            false,
        ),
    ] {
        let image = super::initial_image(&code, 0, tags);
        let mut cpu = complete(
            image.cpu,
            2,
            (u16::from(code[0] & 7) << 8) | u16::from(code[1]),
        );
        cpu.x87.status = status(if overflow { 0x4761 } else { 0x4561 });
        cpu.x87.status.top = final_top;
        cpu.x87.tag_word = final_tags;
        write_register_bits(&mut cpu, destination, INDEFINITE);
        checks.check(name, &code, &image, &[dispatch(cpu)]);
    }
}

fn exchange_empty_operands(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xd9, 0xca];
    for (name, tags, final_tags, first_empty, second_empty) in [
        (
            "empty ST0 moves indefinite to ST2",
            0x0003,
            0x0020,
            true,
            false,
        ),
        (
            "empty ST2 moves indefinite to ST0",
            0x0030,
            0x0002,
            false,
            true,
        ),
        (
            "two empty operands become two indefinites",
            0x0033,
            0x0022,
            true,
            true,
        ),
    ] {
        let image = super::initial_image(&code, 0, tags);
        let mut cpu = complete(image.cpu, 2, 0x01ca);
        cpu.x87.status = status(0x4561);
        cpu.x87.tag_word = final_tags;
        write_register_bits(
            &mut cpu,
            0,
            if second_empty {
                INDEFINITE
            } else {
                register_bits(&image.cpu, 2)
            },
        );
        write_register_bits(
            &mut cpu,
            2,
            if first_empty {
                INDEFINITE
            } else {
                register_bits(&image.cpu, 0)
            },
        );
        checks.check(name, &code, &image, &[dispatch(cpu)]);
    }
    let code = [0xd9, 0xc8];
    let image = super::initial_image(&code, 0, 0x0003);
    let mut cpu = complete(image.cpu, 2, 0x01c8);
    cpu.x87.status = status(0x4561);
    cpu.x87.tag_word = 0x0002;
    write_register_bits(&mut cpu, 0, INDEFINITE);
    checks.check(
        "FXCH ST0 empty self exchange",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

fn unmasked_stack_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (name, instruction, tags, overflow) in [
        ("FLD overflow", [0xd9, 0xc0], 0, true),
        ("FLD underflow", [0xd9, 0xc2], 0xc030, false),
        ("FSTP underflow", [0xdd, 0xda], 3, false),
        ("FXCH underflow", [0xd9, 0xca], 0x30, false),
    ] {
        // The producer retires, but suppresses its value and stack effects.
        // An integer NOP and the no-wait status store run before FWAIT delivers #MF.
        let code = [instruction.as_slice(), &[0x90, 0xdf, 0xe0, 0x9b]].concat();
        let mut image = super::initial_image(&code, 0, tags);
        set_control(&mut image.cpu.x87.control, 0x037e);
        // Suppressed stack movement retains the complete imported TOP byte.
        image.cpu.x87.status.top = 0xa8;
        let mut produced = complete(
            image.cpu,
            2,
            (u16::from(instruction[0] & 7) << 8) | u16::from(instruction[1]),
        );
        let produced_status_word = if overflow { 0xc7e1 } else { 0xc5e1 };
        produced.x87.status = status(produced_status_word);
        produced.x87.status.top = 0xa8;
        let mut nop = produced;
        nop.eip += 1;
        nop.instruction_count = nop.instruction_count.wrapping_add(1);
        let mut observed = nop;
        observed.eip += 2;
        observed.instruction_count = observed.instruction_count.wrapping_add(1);
        observed.registers.eax = 0x1111_0000 | u32::from(produced_status_word);
        checks.check(
            name,
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

fn pending_exception_blocks_stack_operations(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for code in [
        &[0xd9, 0xc0][..],
        &[0xdd, 0xd1],
        &[0xdd, 0xd9],
        &[0xd9, 0xc9],
        &[0xdd, 0xc1],
        &[0xd9, 0xf6],
        &[0xd9, 0xf7],
        &[0xdb, 0x2d, 0, 0x40, 0, 0],
        &[0xdb, 0x3d, 0, 0x40, 0, 0],
    ] {
        let mut image = super::initial_image(code, 0, 0xc000);
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.cpu.x87.status = status(0xc7e1);
        image.map(4, 0x8000, true);
        image.data(0x8000, &[0xa7; 10]);
        checks.check(
            "a pending exception blocks every waiting stack operation",
            code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
}

fn live_status_controls(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xd9, 0xf7, 0xdb, 0xe2, 0xdf, 0xe0];
    let mut image = super::initial_image(&code, 7, 0);
    image.cpu.x87.status = status(0x7f61);
    let mut rotated = complete(image.cpu, 2, 0x01f7);
    rotated.x87.status = status(0x4561);
    let mut cleared = rotated;
    cleared.eip += 2;
    cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
    cleared.x87.status = status(0x4500);
    let mut observed = cleared;
    observed.eip += 2;
    observed.instruction_count = observed.instruction_count.wrapping_add(1);
    observed.registers.eax = 0x1111_4500;
    checks.check(
        "FNCLEX preserves the live TOP established by FINCSTP",
        &code,
        &image,
        &[dispatch(rotated), dispatch(cleared), dispatch(observed)],
    );

    let code = [0xd9, 0xc0, 0xd9, 0x2d, 0, 0x40, 0, 0, 0x9b];
    let mut image = super::initial_image(&code, 0, 0xffff);
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0x7e, 0x03]);
    let mut pushed = complete(image.cpu, 2, 0x01c0);
    pushed.x87.status = status(0x7d61);
    pushed.x87.tag_word = 0xbfff;
    write_register_bits(&mut pushed, 7, INDEFINITE);
    let mut unmasked = pushed;
    unmasked.eip += 6;
    unmasked.instruction_count = unmasked.instruction_count.wrapping_add(1);
    set_control(&mut unmasked.x87.control, 0x037e);
    unmasked.x87.status = status(0xfde1);
    checks.check(
        "FLDCW unmasks an invalid flag produced earlier in the same block",
        &code,
        &image,
        &[
            dispatch(pushed),
            dispatch(unmasked),
            Step {
                cpu: unmasked,
                ram: &[],
                exit: Exit::FloatingPoint,
            },
        ],
    );
}

test_frontends!(masked, masked_stack_faults);
test_frontends!(exchange_empty, exchange_empty_operands);
test_frontends!(unmasked, unmasked_stack_faults);
test_frontends!(
    pending_exceptions,
    pending_exception_blocks_stack_operations
);
test_frontends!(status_controls, live_status_controls);
