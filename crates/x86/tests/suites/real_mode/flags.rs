use super::*;
use wasm86_x86::FlagBytes;

fn push_images(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, width) in [(&[0x9c][..], 2), (&[0x66, 0x9c][..], 4)] {
        for (if_, iopl, image_bits) in [
            (0x80, 0xfc, 0x0024_4597_u32),
            (0x81, 0xfd, 0x0024_5797),
            (0x80, 0xfe, 0x0024_6597),
            (0x81, 0xff, 0x0024_7797),
        ] {
            let mut image = image(code);
            image.cpu.flags.status_source.kind = 9; // Pending 7 - 8.
            image.cpu.flags.status_source.left = 7;
            image.cpu.flags.status_source.right = 8;
            image.cpu.flags.bytes.if_ = if_;
            image.cpu.flags.bytes.iopl = iopl;
            image.cpu.segments.ss = cache(Segment::Ss, 0x2000);
            image.cpu.registers.esp = 0xabcd_0000;
            image.map(0x2f, 0x8000, true);
            let mut cpu = retired(&image, code.len());
            cpu.registers.esp = 0xabcd_0000 | (0x10000 - width);
            cases.check(
                "PUSHF reads IF/IOPL and lazy status without changing their raw backing",
                code,
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x9000 - width, &image_bits.to_le_bytes()[..width as usize])],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}
test_frontends!(push_flag_images, push_images);

fn pop_images(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, width) in [(&[0x9d][..], 2), (&[0x66, 0x9d][..], 4)] {
        for (image_bits, if_, iopl, other) in [
            (0xffdb_8028_u32, 0, 0, 0),
            (0xffdb_9228, 1, 1, 0),
            (0xffdb_a028, 0, 2, 0),
            (0xffdb_b228, 1, 3, 0),
            (u32::MAX, 1, 3, 1),
        ] {
            let mut image = image(code);
            image.cpu.flags.status_source.kind = 9;
            image.cpu.flags.status_source.left = 7;
            image.cpu.flags.status_source.right = 8;
            image.cpu.registers.esp = 0xabcd_0000 | (0x10000 - width);
            image.cpu.segments.ss = cache(Segment::Ss, 0x1000);
            image.map(0x1f, 0x8000, false);
            image.data(0x9000 - width, &image_bits.to_le_bytes()[..width as usize]);
            let mut cpu = retired(&image, code.len());
            cpu.registers.esp = 0xabcd_0000;
            cpu.flags.status_source.kind = 0;
            cpu.flags.bytes = FlagBytes {
                cf: other,
                pf: other,
                af: other,
                zf: other,
                sf: other,
                of: other,
                tf: other,
                df: other,
                nt: other,
                if_,
                iopl,
                ..cpu.flags.bytes
            };
            if width == 4 {
                cpu.flags.bytes.ac = other;
                cpu.flags.bytes.id = other;
            }
            cases.check(
                "POPF restores IF/IOPL, ignores reserved bits and preserves high flags for word images",
                code,
                &image,
                &[Step { cpu, ram: &[], exit: Exit::Dispatch(cpu.eip) }],
            );
        }
    }
}
test_frontends!(pop_flag_images, pop_images);

fn faults(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, sp) in [
        (&[0x9c][..], 1),
        (&[0x66, 0x9c][..], 3),
        (&[0x9d][..], 0xffff),
        (&[0x66, 0x9d][..], 0xfffe),
    ] {
        let mut image = image(code);
        image.cpu.flags.status_source.kind = 9;
        image.cpu.flags.status_source.left = 7;
        image.cpu.flags.status_source.right = 8;
        image.cpu.registers.esp = 0xabcd_0000 | sp;
        image.map(0xf, 0x8000, true);
        image.data(0x8ffc, &[0xff; 4]);
        cases.check(
            "an incomplete FLAGS span faults before changing ESP, flags or RAM",
            code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::StackFault { error: 0 },
            }],
        );
    }
}
test_frontends!(stack_bounds, faults);

fn histories(engine: Engine, frontend: Frontend) {
    // POPFD, POPF, PUSHFD, then a fault: the word transfer preserves AC/ID,
    // later image reads see the new IF/IOPL, and the fault publishes those writes.
    let code = [0x66, 0x9d, 0x9d, 0x66, 0x67, 0x9c, 0x67, 0xa1, 0, 0, 1, 0];
    let mut image = image(&code);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.registers.esp = 0xabcd_2000;
    image.map(2, 0x8000, true);
    image.data(0x8000, &[0xff, 0xff, 0xff, 0xff, 0, 0x20]);
    let mut dword = retired(&image, 2);
    dword.registers.esp = 0xabcd_2004;
    dword.flags.bytes = FlagBytes {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
        tf: 1,
        df: 1,
        nt: 1,
        ac: 1,
        id: 1,
        if_: 1,
        iopl: 3,
        ..dword.flags.bytes
    };
    let mut word = dword;
    word.eip += 1;
    word.instruction_count += 1;
    word.registers.esp = 0xabcd_2006;
    word.flags.bytes = FlagBytes {
        cf: 0,
        pf: 0,
        af: 0,
        zf: 0,
        sf: 0,
        of: 0,
        tf: 0,
        df: 0,
        nt: 0,
        if_: 0,
        iopl: 2,
        ..word.flags.bytes
    };
    let mut pushed = word;
    pushed.eip += 3;
    pushed.instruction_count += 1;
    pushed.registers.esp = 0xabcd_2002;
    sequences(engine, frontend).check(
        "mode-aware flag images compose across word/dword transfers and a later fault",
        &code,
        &image,
        &[
            Step {
                cpu: dword,
                ram: &[],
                exit: Exit::Dispatch(dword.eip),
            },
            Step {
                cpu: word,
                ram: &[],
                exit: Exit::Dispatch(word.eip),
            },
            Step {
                cpu: pushed,
                ram: &[(0x8002, &[2, 0x20, 0x24, 0])],
                exit: Exit::Dispatch(pushed.eip),
            },
            Step {
                cpu: pushed,
                ram: &[],
                exit: Exit::GeneralProtection { error: 0 },
            },
        ],
    );
}
test_frontends!(flag_transfer_histories, histories);
