//! Comparison results and stack movement remain live for subsequent consumers.

use super::*;

fn double_pop_continuation(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (suppressed, code) in [
        (false, vec![0xde, 0xd9, 0xd9, 0xe4]), // FCOMPP; FTST
        (true, vec![0xde, 0xd9, 0xdb, 0xe2, 0xd9, 0xe4]), // FCOMPP; FNCLEX; FTST
    ] {
        let mut image = initial_image(&code);
        set_control(
            &mut image.cpu.x87.control,
            if suppressed { 0x037e } else { 0x037f },
        );
        write_value(&mut image.cpu, 7, ONE);
        write_value(&mut image.cpu, 0, if suppressed { SNAN } else { ONE });
        write_value(&mut image.cpu, 1, (LEADING, 0xbfff));
        let compared = completed(
            image.cpu,
            2,
            0x06d9,
            if suppressed { 0x4101 | PENDING } else { EQUAL },
            if suppressed { 0 } else { 2 },
        );
        let mut steps = vec![dispatch(compared)];
        let mut before_test = compared;
        if suppressed {
            before_test.eip += 2;
            before_test.instruction_count += 1;
            before_test.x87.status = status(0x7900);
            steps.push(dispatch(before_test));
        }
        let mut tested = completed(before_test, 2, 0x01e4, if suppressed { 0 } else { LESS }, 0);
        if suppressed {
            tested.x87.status.precision = 0;
        }
        steps.push(dispatch(tested));
        checks.check(
            "double pop and suppressed movement feed the next stack access",
            &code,
            &image,
            &steps,
        );
    }
}

fn loaded_value_provenance(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, value, load_flags) in [
        (1_u32, (LEADING, 0x3f6a), 2),
        (0x7f80_0001, (0xc000_0100_0000_0000, 0x7fff), 1),
    ] {
        let code = [0xd9, 0x05, 0, 0x40, 0, 0, 0xdb, 0xe2, 0xdd, 0xe0];
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status = status(0);
        image.map(4, 0x8000, false);
        image.data(0x8000, &source.to_le_bytes());
        let mut loaded = complete_x87(image.cpu, 6, 0x0105);
        loaded.x87.status = status(0x3800 | load_flags);
        loaded.x87.data_offset = 0x4000;
        loaded.x87.data_selector = 0x23;
        write_value(&mut loaded, 7, value);
        let mut cleared = loaded;
        cleared.eip += 2;
        cleared.instruction_count += 1;
        cleared.x87.status = status(0x3800);
        let mut compared = complete_x87(cleared, 2, 0x05e0);
        compared.x87.status = status(0x3800 | if load_flags == 2 { EQUAL } else { UNORDERED });
        checks.check(
            "FUCOM consumes the loaded value rather than the original memory class",
            &code,
            &image,
            &[dispatch(loaded), dispatch(cleared), dispatch(compared)],
        );
    }
}

fn pending_register_exception(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for code in [[0xd8, 0xd1], [0xda, 0xe9], [0xd9, 0xe4]] {
        let mut image = initial_image(&code);
        image.cpu.x87.status.invalid = 1;
        image.cpu.x87.status.error_summary = 1;
        image.cpu.x87.status.busy = 1;
        checks.check(
            "waiting comparison preserves prior state on entry fault",
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

fn status_drives_integer_conditions(engine: Engine, frontend: Frontend) {
    let code = [
        0xda, 0xe9, // FUCOMPP
        0xdf, 0xe0, // FNSTSW AX
        0x9e, // SAHF
        0x0f, 0x92, 0xc3, // SETB BL
        0x0f, 0x94, 0xc1, // SETE CL
        0x0f, 0x9a, 0xc2, // SETP DL
    ];
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, relation, below, equal, unordered) in [
        ((LEADING, 0x4000), 0, 0, 0, 0),
        ((LEADING, 0xbfff), LESS, 1, 0, 0),
        (ONE, EQUAL, 0, 1, 0),
        (QNAN, UNORDERED, 1, 1, 1),
    ] {
        let mut image = initial_image(&code);
        image.cpu.flags = Default::default();
        image.cpu.flags.bytes.of = 1;
        write_value(&mut image.cpu, 7, left);
        write_value(&mut image.cpu, 0, ONE);
        let compared = completed(image.cpu, 2, 0x02e9, relation, 2);
        let mut observed = compared;
        observed.eip += 2;
        observed.instruction_count += 1;
        observed.registers.eax = 0x1111_0000 | u32::from(0x0820 | relation);
        let mut flags = observed;
        flags.eip += 1;
        flags.instruction_count += 1;
        flags.flags.bytes.cf = below;
        flags.flags.bytes.zf = equal;
        flags.flags.bytes.pf = unordered;
        let mut set_below = flags;
        set_below.eip += 3;
        set_below.instruction_count += 1;
        set_below.registers.ebx = (set_below.registers.ebx & !0xff) | u32::from(below);
        let mut set_equal = set_below;
        set_equal.eip += 3;
        set_equal.instruction_count += 1;
        set_equal.registers.ecx = (set_equal.registers.ecx & !0xff) | u32::from(equal);
        let mut set_unordered = set_equal;
        set_unordered.eip += 3;
        set_unordered.instruction_count += 1;
        set_unordered.registers.edx = (set_unordered.registers.edx & !0xff) | u32::from(unordered);
        checks.check(
            "x87 status feeds unsigned integer conditions through SAHF",
            &code,
            &image,
            &[
                dispatch(compared),
                dispatch(observed),
                dispatch(flags),
                dispatch(set_below),
                dispatch(set_equal),
                dispatch(set_unordered),
            ],
        );
    }
}

test_frontends!(stack_continuation, double_pop_continuation);
test_frontends!(load_provenance, loaded_value_provenance);
test_frontends!(pending, pending_register_exception);
test_frontends!(integer_conditions, status_drives_integer_conditions);
