use super::*;

fn jumps(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, offset, selector) in [
        (&[0xea, 0x34, 0x12, 0, 0][..], 0x1234, 0),
        (
            &[0x66, 0xea, 0xff, 0xff, 0, 0, 0xff, 0xff][..],
            0xffff,
            0xffff,
        ),
        (&[0xff, 0x2e, 0, 0x20][..], 0x5678, 4),
    ] {
        let mut image = image(code);
        image.map(2, 0x8000, false);
        image.data(0x8000, &[0x78, 0x56, 4, 0]);
        let mut cpu = retired(&image, code.len());
        cpu.eip = offset;
        cpu.segments.cs = cache(Segment::Cs, selector);
        cases.check(
            "far JMP commits any segment value and dispatches before target fetch",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(offset),
            }],
        );
    }
    for code in [&[0x66, 0xea, 0, 0, 1, 0, 0, 0][..], &[0x66, 0xff, 0xe0][..]] {
        let mut image = image(code);
        image.cpu.registers.eax = 0x10000;
        cases.check(
            "dword near and far targets must fit CS",
            code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            }],
        );
    }
}
test_frontends!(jump_targets, jumps);

fn calls(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, sp, frame, saved) in [
        (&[0x9a, 0x34, 0x12, 0, 0][..], 6, 2, &[5, 0x10, 0, 2][..]),
        (
            &[0x66, 0x9a, 0x34, 0x12, 0, 0, 0, 0][..],
            2,
            0xfffa,
            &[8, 0x10, 0, 0, 0, 2][..],
        ),
        (&[0xff, 0x1e, 0, 0x20][..], 6, 2, &[4, 0x10, 0, 2][..]),
    ] {
        let mut image = image(code);
        image.cpu.segments.cs = cache(Segment::Cs, 0x200);
        image.map(3, 0x3000, false);
        image.cpu.segments.ss = cache(Segment::Ss, 0x1000);
        image.cpu.registers.esp = 0xabcd_0000 | sp;
        image.map((0x10000 + frame) >> 12, 0x8000, true);
        image.map(2, 0x9000, false);
        image.data(0x9000, &[0x34, 0x12, 0, 0]);
        let mut cpu = retired(&image, code.len());
        cpu.eip = 0x1234;
        cpu.segments.cs = cache(Segment::Cs, 0);
        cpu.registers.esp = 0xabcd_0000 | frame;
        cases.check(
            "CALL reserves slots and checks only its real-mode transfer span",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8000 + (frame & 0xfff), saved)],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
    let code = [0x66, 0x9a, 0, 0, 1, 0, 0, 0];
    for (sp, exit) in [
        (7, Exit::StackFault { error: 0 }),
        (0x2000, Exit::GeneralProtection { error: 0 }),
    ] {
        let mut image = image(&code);
        image.cpu.registers.esp = sp;
        cases.check(
            "CALL checks stack capacity before target and target before backing",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
}
test_frontends!(far_call_frames, calls);

fn returns(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, frame, sp, next_sp, target, selector) in [
        (
            &[0xcb][..],
            &[0x34, 0x12, 4, 0][..],
            0x2000,
            0x2004,
            0x1234,
            4,
        ),
        (
            &[0xca, 0xff, 0xff][..],
            &[0x34, 0x12, 0, 0][..],
            0x2000,
            0x2003,
            0x1234,
            0,
        ),
        (
            &[0x66, 0xcb][..],
            &[0x78, 0x56, 0, 0, 0xff, 0xff, 0xa5, 0xa5][..],
            0xfff8,
            0,
            0x5678,
            0xffff,
        ),
    ] {
        let mut image = image(code);
        image.cpu.registers.esp = 0xabcd_0000 | sp;
        image.map(sp >> 12, 0x8000, false);
        image.data(0x8000 + (sp & 0xfff), frame);
        let mut cpu = retired(&image, code.len());
        cpu.eip = target;
        cpu.registers.esp = 0xabcd_0000 | next_sp;
        cpu.segments.cs = cache(Segment::Cs, selector);
        cases.check(
            "RETF accepts low privilege bits and wraps only SP arithmetic",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(target),
            }],
        );
    }
    let code = [0x66, 0xcb];
    for (sp, exit) in [
        (0xfffc, Exit::StackFault { error: 0 }),
        (0x2000, Exit::GeneralProtection { error: 0 }),
    ] {
        let mut image = image(&code);
        image.cpu.registers.esp = sp;
        image.map(2, 0x8000, false);
        image.data(0x8000, &[0, 0, 1, 0, 0, 0, 0, 0]);
        cases.check(
            "RETF rejects incomplete frames and oversized targets before commit",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
}
test_frontends!(far_return_frames, returns);
